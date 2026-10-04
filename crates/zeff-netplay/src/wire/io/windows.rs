use std::io::{Error, ErrorKind, Read, Write};
use std::net::TcpStream;
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use mio::{Events, Interest, Poll, Token};

use super::{CANCELLATION_QUANTUM, check_deadline};

pub(in crate::wire) struct Driver {
    // Drop the registered socket while its poller is still alive.
    socket: mio::net::TcpStream,
    poll: Poll,
    events: Events,
    interest: Interest,
}

impl Driver {
    pub(in crate::wire) fn new(stream: &TcpStream) -> Result<Self> {
        let poll = Poll::new()?;
        // All I/O uses Mio; the original socket supplies options and cancellation.
        stream.set_nonblocking(true)?;
        let mut socket = mio::net::TcpStream::from_std(stream.try_clone()?);
        let interest = Interest::READABLE;
        poll.registry().register(&mut socket, Token(0), interest)?;
        Ok(Self {
            socket,
            poll,
            events: Events::with_capacity(4),
            interest,
        })
    }

    fn set_interest(&mut self, interest: Interest) -> Result<()> {
        if self.interest != interest {
            self.poll
                .registry()
                .reregister(&mut self.socket, Token(0), interest)?;
            self.interest = interest;
            self.service_updates()?;
        }
        Ok(())
    }

    fn service_updates(&mut self) -> Result<()> {
        self.poll
            .poll(&mut self.events, Some(Duration::ZERO))
            .context("servicing netplay readiness registration")
    }

    fn wait_ready(
        &mut self,
        deadline: Instant,
        attempt_deadline: Instant,
        configured: Option<Duration>,
        cancellation: Option<&AtomicBool>,
    ) -> Result<()> {
        let remaining = check_deadline(deadline, cancellation)?;
        let attempt_remaining = attempt_deadline
            .checked_duration_since(Instant::now())
            .filter(|duration| !duration.is_zero())
            .ok_or_else(|| Error::new(ErrorKind::TimedOut, "configured netplay I/O timeout"))?;
        let mut duration = remaining.min(attempt_remaining);
        if let Some(configured) = configured {
            duration = duration.min(configured);
        }
        if cancellation.is_some() {
            duration = duration.min(CANCELLATION_QUANTUM);
        }
        match self.poll.poll(&mut self.events, Some(duration)) {
            Ok(()) => {}
            Err(error) if error.kind() == ErrorKind::Interrupted => {}
            Err(error) => return Err(error).context("waiting for netplay socket readiness"),
        }
        check_deadline(deadline, cancellation)?;
        Ok(())
    }

    fn transfer_part(
        &mut self,
        deadline: Instant,
        configured: Option<Duration>,
        cancellation: Option<&AtomicBool>,
        mut operation: impl FnMut(&mut mio::net::TcpStream) -> std::io::Result<usize>,
    ) -> Result<usize> {
        let attempt_deadline = if cancellation.is_none() {
            configured
                .and_then(|duration| Instant::now().checked_add(duration))
                .map_or(deadline, |timeout| timeout.min(deadline))
        } else {
            deadline
        };
        loop {
            check_deadline(deadline, cancellation)?;
            if Instant::now() >= attempt_deadline {
                return Err(
                    Error::new(ErrorKind::TimedOut, "configured netplay I/O timeout").into(),
                );
            }
            match operation(&mut self.socket) {
                Ok(count) => return Ok(count),
                Err(error) if error.kind() == ErrorKind::Interrupted => {}
                Err(error) if error.kind() == ErrorKind::WouldBlock => {
                    self.wait_ready(deadline, attempt_deadline, configured, cancellation)?;
                }
                Err(error) => return Err(error).context("transferring nonblocking netplay bytes"),
            }
        }
    }

    pub(in crate::wire) fn read(
        &mut self,
        stream: &mut TcpStream,
        bytes: &mut [u8],
        deadline: Instant,
        cancellation: Option<&AtomicBool>,
    ) -> Result<()> {
        let configured = stream.read_timeout()?;
        self.set_interest(Interest::READABLE)?;
        let mut offset = 0;
        while offset < bytes.len() {
            let count = self.transfer_part(deadline, configured, cancellation, |socket| {
                socket.read(&mut bytes[offset..])
            })?;
            if count == 0 {
                return Err(
                    Error::new(ErrorKind::UnexpectedEof, "connection closed during read").into(),
                );
            }
            offset += count;
            check_deadline(deadline, cancellation)?;
        }
        Ok(())
    }

    pub(in crate::wire) fn write(
        &mut self,
        stream: &mut TcpStream,
        bytes: &[u8],
        deadline: Instant,
        cancellation: Option<&AtomicBool>,
    ) -> Result<()> {
        let configured = stream.write_timeout()?;
        self.set_interest(Interest::WRITABLE)?;
        let mut offset = 0;
        while offset < bytes.len() {
            let count = self.transfer_part(deadline, configured, cancellation, |socket| {
                socket.write(&bytes[offset..])
            })?;
            if count == 0 {
                return Err(
                    Error::new(ErrorKind::WriteZero, "connection closed during write").into(),
                );
            }
            offset += count;
            check_deadline(deadline, cancellation)?;
        }
        Ok(())
    }

    pub(in crate::wire) fn peer_disconnected(&mut self, _stream: &TcpStream) -> Result<bool> {
        self.set_interest(Interest::READABLE)?;
        let result = match self.socket.peek(&mut [0]) {
            Ok(count) => Ok(count == 0),
            Err(error)
                if matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::Interrupted) =>
            {
                Ok(false)
            }
            Err(error) => Err(error).context("peeking idle netplay socket"),
        };
        let serviced = self.service_updates();
        match (result, serviced) {
            (Err(error), _) | (_, Err(error)) => Err(error),
            (Ok(disconnected), Ok(())) => Ok(disconnected),
        }
    }
}
