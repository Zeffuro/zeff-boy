use axum::{
    extract::connect_info::Connected,
    serve::{IncomingStream, Listener},
};
use std::{
    future::Future,
    io,
    net::SocketAddr,
    pin::Pin,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    task::{Context, Poll},
    time::Duration,
};
use tokio::{
    io::{AsyncRead, AsyncWrite, ReadBuf},
    net::{TcpListener, TcpStream},
    sync::{OwnedSemaphorePermit, Semaphore},
    time::{Sleep, sleep},
};

pub struct BoundedListener {
    listener: TcpListener,
    slots: Arc<Semaphore>,
    lifetime: Duration,
}

impl BoundedListener {
    pub fn new(listener: TcpListener, connections: usize, lifetime: Duration) -> Self {
        Self {
            listener,
            slots: Arc::new(Semaphore::new(connections)),
            lifetime,
        }
    }
}

pub struct BoundedStream {
    stream: TcpStream,
    deadline: Pin<Box<Sleep>>,
    gate: ConnectionGate,
    admitted: bool,
    lifetime: Duration,
    _slot: OwnedSemaphorePermit,
}

#[derive(Clone)]
pub struct ConnectionGate(Arc<AtomicBool>);
impl ConnectionGate {
    pub(crate) fn admit(&self) {
        self.0.store(true, Ordering::Relaxed);
    }
}
impl Connected<IncomingStream<'_, BoundedListener>> for ConnectionGate {
    fn connect_info(stream: IncomingStream<'_, BoundedListener>) -> Self {
        stream.io().gate.clone()
    }
}

impl Listener for BoundedListener {
    type Io = BoundedStream;
    type Addr = SocketAddr;
    async fn accept(&mut self) -> (Self::Io, Self::Addr) {
        loop {
            let slot = self.slots.clone().acquire_owned().await.unwrap();
            match self.listener.accept().await {
                Ok((stream, addr)) => {
                    return (
                        BoundedStream {
                            stream,
                            deadline: Box::pin(sleep(Duration::from_secs(5))),
                            _slot: slot,
                            gate: ConnectionGate(Arc::new(AtomicBool::new(false))),
                            admitted: false,
                            lifetime: self.lifetime,
                        },
                        addr,
                    );
                }
                Err(_) => sleep(Duration::from_millis(100)).await,
            }
        }
    }
    fn local_addr(&self) -> io::Result<Self::Addr> {
        self.listener.local_addr()
    }
}

impl BoundedStream {
    fn expired(&mut self, cx: &mut Context<'_>) -> bool {
        if !self.admitted && self.gate.0.load(Ordering::Relaxed) {
            self.admitted = true;
            self.deadline = Box::pin(sleep(self.lifetime));
        }
        self.deadline.as_mut().poll(cx).is_ready()
    }
}

fn expired() -> io::Error {
    io::Error::new(io::ErrorKind::TimedOut, "connection lifetime exceeded")
}

impl AsyncRead for BoundedStream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        if self.expired(cx) {
            return Poll::Ready(Err(expired()));
        }
        Pin::new(&mut self.stream).poll_read(cx, buf)
    }
}
impl AsyncWrite for BoundedStream {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        if self.expired(cx) {
            return Poll::Ready(Err(expired()));
        }
        Pin::new(&mut self.stream).poll_write(cx, buf)
    }
    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        if self.expired(cx) {
            return Poll::Ready(Err(expired()));
        }
        Pin::new(&mut self.stream).poll_flush(cx)
    }
    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.stream).poll_shutdown(cx)
    }
}
