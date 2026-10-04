use std::io;
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use super::{TcpLinkTransport, is_transient_link_open_error};

const CONNECT_BUDGET: Duration = Duration::from_secs(30);
const ATTEMPT_LIMIT: Duration = Duration::from_millis(100);
const RETRY_INTERVAL: Duration = Duration::from_millis(10);

pub(crate) struct TcpLinkConnector {
    cancelled: Arc<AtomicBool>,
    worker: Option<JoinHandle<io::Result<TcpLinkTransport>>>,
}

impl TcpLinkConnector {
    pub(crate) fn host(addr: &str) -> io::Result<Self> {
        let listener = TcpListener::bind(numeric_address(addr)?)?;
        Self::host_listener(listener, CONNECT_BUDGET)
    }

    pub(crate) fn join(addr: &str) -> io::Result<Self> {
        let addr = numeric_address(addr)?;
        Self::spawn(move |cancelled| {
            wait_for_stream(cancelled, CONNECT_BUDGET, |remaining| {
                TcpStream::connect_timeout(&addr, remaining.min(ATTEMPT_LIMIT))
            })
        })
    }

    fn host_listener(listener: TcpListener, budget: Duration) -> io::Result<Self> {
        listener.set_nonblocking(true)?;
        Self::spawn(move |cancelled| {
            wait_for_stream(cancelled, budget, |_| {
                listener.accept().map(|(stream, _)| stream)
            })
        })
    }

    fn spawn(
        open: impl FnOnce(&AtomicBool) -> io::Result<TcpStream> + Send + 'static,
    ) -> io::Result<Self> {
        let cancelled = Arc::new(AtomicBool::new(false));
        let worker_cancelled = Arc::clone(&cancelled);
        let worker = thread::Builder::new()
            .name("tcp-link-connect".into())
            .spawn(move || {
                let stream = open(&worker_cancelled)?;
                if worker_cancelled.load(Ordering::Acquire) {
                    return Err(cancelled_error());
                }
                TcpLinkTransport::from_stream(stream)
            })?;
        Ok(Self {
            cancelled,
            worker: Some(worker),
        })
    }

    pub(crate) fn poll(&mut self) -> io::Result<Option<TcpLinkTransport>> {
        if self
            .worker
            .as_ref()
            .is_none_or(|worker| !worker.is_finished())
        {
            return Ok(None);
        }
        join_worker(self.worker.take().unwrap()).map(Some)
    }

    pub(crate) fn cancel(&mut self) {
        self.cancelled.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            worker.thread().unpark();
            // Drop any completed transport only after its connector has exited.
            drop(join_worker(worker));
        }
    }
}

impl Drop for TcpLinkConnector {
    fn drop(&mut self) {
        self.cancel();
    }
}

fn numeric_address(addr: &str) -> io::Result<SocketAddr> {
    let parsed = if let Some(port) = addr.strip_prefix("localhost:") {
        port.parse::<u16>()
            .map(|port| SocketAddr::from(([127, 0, 0, 1], port)))
            .ok()
    } else {
        addr.parse().ok()
    };
    parsed.ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "TCP link requires a numeric IP:port (or localhost:port)",
        )
    })
}

fn wait_for_stream(
    cancelled: &AtomicBool,
    budget: Duration,
    mut attempt: impl FnMut(Duration) -> io::Result<TcpStream>,
) -> io::Result<TcpStream> {
    let deadline = Instant::now() + budget;
    loop {
        if cancelled.load(Ordering::Acquire) {
            return Err(cancelled_error());
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "TCP link connection deadline expired",
            ));
        }
        match attempt(remaining) {
            Ok(stream) => return Ok(stream),
            Err(error)
                if error.kind() == io::ErrorKind::WouldBlock
                    || error.kind() == io::ErrorKind::Interrupted
                    || is_transient_link_open_error(&error) => {}
            Err(error) => return Err(error),
        }
        if !cancelled.load(Ordering::Acquire) {
            thread::park_timeout(RETRY_INTERVAL.min(remaining));
        }
    }
}

fn cancelled_error() -> io::Error {
    io::Error::new(io::ErrorKind::Interrupted, "TCP link connection cancelled")
}

fn join_worker(worker: JoinHandle<io::Result<TcpLinkTransport>>) -> io::Result<TcpLinkTransport> {
    worker
        .join()
        .unwrap_or_else(|_| Err(io::Error::other("TCP link connection worker panicked")))
}

#[cfg(test)]
mod tests;
