use std::io::ErrorKind;
#[cfg(any(not(windows), test))]
use std::io::{Read, Write};
#[cfg(any(not(windows), test))]
use std::net::TcpStream;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

#[cfg(not(windows))]
use anyhow::Context;
use anyhow::Result;

const CANCELLATION_QUANTUM: Duration = Duration::from_millis(50);

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub(super) use windows::Driver;

#[cfg(not(windows))]
fn with_nonblocking<T>(stream: &TcpStream, operation: impl FnOnce() -> Result<T>) -> Result<T> {
    struct RestoreBlocking<'a>(&'a TcpStream, bool);
    impl Drop for RestoreBlocking<'_> {
        fn drop(&mut self) {
            if self.1 {
                let _ = self.0.set_nonblocking(false);
            }
        }
    }

    // The connection owns I/O exclusively; socket clones only cancel via shutdown.
    stream
        .set_nonblocking(true)
        .context("enabling nonblocking netplay I/O")?;
    let mut restore = RestoreBlocking(stream, true);
    let result = operation();
    let restored = stream
        .set_nonblocking(false)
        .context("restoring blocking netplay I/O");
    restore.1 = restored.is_err();
    match (result, restored) {
        (Err(error), _) => Err(error),
        (Ok(_), Err(error)) => Err(error),
        (Ok(value), Ok(())) => Ok(value),
    }
}

#[cfg(not(windows))]
fn peer_disconnected(stream: &TcpStream) -> Result<bool> {
    with_nonblocking(stream, || match stream.peek(&mut [0]) {
        Ok(count) => Ok(count == 0),
        Err(error) if matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::Interrupted) => {
            Ok(false)
        }
        Err(error) => Err(error).context("peeking idle netplay socket"),
    })
}

#[cfg(not(windows))]
pub(super) struct Driver;

#[cfg(not(windows))]
impl Driver {
    pub(super) fn new(_stream: &TcpStream) -> Result<Self> {
        Ok(Self)
    }
    pub(super) fn read(
        &mut self,
        stream: &mut TcpStream,
        bytes: &mut [u8],
        deadline: Instant,
        cancellation: Option<&AtomicBool>,
    ) -> Result<()> {
        read_cancellable(stream, bytes, deadline, cancellation)
    }
    pub(super) fn write(
        &mut self,
        stream: &mut TcpStream,
        bytes: &[u8],
        deadline: Instant,
        cancellation: Option<&AtomicBool>,
    ) -> Result<()> {
        write_cancellable(stream, bytes, deadline, cancellation)
    }
    pub(super) fn peer_disconnected(&mut self, stream: &TcpStream) -> Result<bool> {
        peer_disconnected(stream)
    }
}

pub(super) fn check_deadline(
    deadline: Instant,
    cancellation: Option<&AtomicBool>,
) -> Result<Duration> {
    if cancellation.is_some_and(|flag| flag.load(Ordering::Acquire)) {
        return Err(std::io::Error::new(ErrorKind::Interrupted, "netplay I/O cancelled").into());
    }
    deadline
        .checked_duration_since(Instant::now())
        .filter(|duration| !duration.is_zero())
        .ok_or_else(|| std::io::Error::new(ErrorKind::TimedOut, "I/O deadline expired").into())
}

#[cfg(not(windows))]
fn timeout(
    deadline: Instant,
    configured: Option<Duration>,
    cancellation: Option<&AtomicBool>,
) -> Result<Duration> {
    let remaining = check_deadline(deadline, cancellation)?;
    let timeout = configured.map_or(remaining, |cap| cap.min(remaining));
    Ok(if cancellation.is_some() {
        timeout.min(CANCELLATION_QUANTUM)
    } else {
        timeout
    })
}

#[cfg(not(windows))]
fn retry_timeout(error: &std::io::Error, cancellation: Option<&AtomicBool>) -> bool {
    cancellation.is_some() && matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut)
}

#[cfg(test)]
pub(super) fn read_deadline(
    driver: &mut Driver,
    stream: &mut TcpStream,
    bytes: &mut [u8],
    deadline: Instant,
) -> Result<()> {
    driver.read(stream, bytes, deadline, None)
}

#[cfg(not(windows))]
pub(super) fn read_cancellable(
    stream: &mut TcpStream,
    bytes: &mut [u8],
    deadline: Instant,
    cancellation: Option<&AtomicBool>,
) -> Result<()> {
    let configured = stream.read_timeout()?;
    let result = (|| -> Result<()> {
        let mut offset = 0;
        while offset < bytes.len() {
            stream.set_read_timeout(Some(timeout(deadline, configured, cancellation)?))?;
            match stream.read(&mut bytes[offset..]) {
                Ok(0) => {
                    return Err(std::io::Error::new(
                        ErrorKind::UnexpectedEof,
                        "connection closed during read",
                    )
                    .into());
                }
                Ok(count) => offset += count,
                Err(error)
                    if error.kind() == ErrorKind::Interrupted
                        || retry_timeout(&error, cancellation) =>
                {
                    continue;
                }
                Err(error) => return Err(error.into()),
            }
            check_deadline(deadline, cancellation)?;
        }
        Ok(())
    })();
    let restored = stream
        .set_read_timeout(configured)
        .context("restoring read timeout");
    result.and(restored)
}

#[cfg(test)]
pub(super) fn write_deadline(
    driver: &mut Driver,
    stream: &mut TcpStream,
    bytes: &[u8],
    deadline: Instant,
) -> Result<()> {
    driver.write(stream, bytes, deadline, None)
}

#[cfg(not(windows))]
pub(super) fn write_cancellable(
    stream: &mut TcpStream,
    bytes: &[u8],
    deadline: Instant,
    cancellation: Option<&AtomicBool>,
) -> Result<()> {
    let configured = stream.write_timeout()?;
    let result = (|| -> Result<()> {
        let mut offset = 0;
        while offset < bytes.len() {
            stream.set_write_timeout(Some(timeout(deadline, configured, cancellation)?))?;
            match stream.write(&bytes[offset..]) {
                Ok(0) => {
                    return Err(std::io::Error::new(
                        ErrorKind::WriteZero,
                        "connection closed during write",
                    )
                    .into());
                }
                Ok(count) => offset += count,
                Err(error)
                    if error.kind() == ErrorKind::Interrupted
                        || retry_timeout(&error, cancellation) =>
                {
                    continue;
                }
                Err(error) => return Err(error.into()),
            }
            check_deadline(deadline, cancellation)?;
        }
        Ok(())
    })();
    let restored = stream
        .set_write_timeout(configured)
        .context("restoring write timeout");
    result.and(restored)
}

#[cfg(test)]
mod socket_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;
    use std::sync::{Arc, mpsc};
    use std::thread;

    fn streams() -> (TcpStream, TcpStream) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let client = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let (host, _) = listener.accept().unwrap();
        (host, client)
    }

    #[test]
    fn cancellation_interrupts_pending_read_without_socket_shutdown_and_restores_timeout() {
        let (mut host, _silent_peer) = streams();
        let configured = Duration::from_secs(2);
        host.set_read_timeout(Some(configured)).unwrap();
        let cancellation = Arc::new(AtomicBool::new(false));
        let reader_flag = Arc::clone(&cancellation);
        let (started, waiting) = mpsc::channel();
        let reader = thread::spawn(move || {
            let mut driver = Driver::new(&host).unwrap();
            started.send(()).unwrap();
            let error = driver
                .read(
                    &mut host,
                    &mut [0],
                    Instant::now() + configured,
                    Some(&reader_flag),
                )
                .unwrap_err();
            assert!(error.to_string().contains("cancelled"));
            assert_eq!(host.read_timeout().unwrap(), Some(configured));
        });
        waiting.recv_timeout(Duration::from_secs(1)).unwrap();
        thread::sleep(Duration::from_millis(100));
        let start = Instant::now();
        cancellation.store(true, Ordering::Release);
        reader.join().unwrap();
        assert!(start.elapsed() < Duration::from_millis(500));
    }

    #[test]
    fn cancellable_short_timeout_retries_only_until_absolute_deadline() {
        let (mut host, _silent_peer) = streams();
        let configured = Duration::from_millis(20);
        host.set_read_timeout(Some(configured)).unwrap();
        let mut driver = Driver::new(&host).unwrap();
        let start = Instant::now();
        let deadline = start + Duration::from_millis(150);
        let flag = AtomicBool::new(false);
        let error = driver
            .read(&mut host, &mut [0], deadline, Some(&flag))
            .unwrap_err();
        assert!(error.to_string().contains("deadline"));
        assert!(start.elapsed() >= Duration::from_millis(150));
        assert!(start.elapsed() < Duration::from_secs(1));
        assert_eq!(host.read_timeout().unwrap(), Some(configured));
    }

    #[test]
    fn cancelled_admission_returns_before_waiting_for_peer() {
        let (host, _silent_peer) = streams();
        let identity = crate::wire::Identity {
            build: [0; 32],
            build_info: Default::default(),
            source: [0; 32],
            effective: [0; 32],
            media_len: 0,
            config: [0; 32],
            initial: [0; 32],
            persistent: [0; 32],
            state_format: 11,
        };
        let start = Instant::now();
        let error = crate::wire::admit_cancellable(
            host,
            crate::lockstep::Player::One,
            &identity,
            &[0; 32],
            Arc::new(AtomicBool::new(true)),
        )
        .err()
        .unwrap();
        assert!(error.to_string().contains("cancelled"));
        assert!(start.elapsed() < Duration::from_millis(500));
    }

    #[test]
    fn cancellation_rejects_buffered_packet_and_makes_connection_terminal() {
        use crate::lockstep::Player;
        use crate::wire::{Identity, Message, admit, admit_cancellable};

        let (host, client) = streams();
        let identity = Identity {
            build: [0; 32],
            build_info: Default::default(),
            source: [0; 32],
            effective: [0; 32],
            media_len: 0,
            config: [0; 32],
            initial: [0; 32],
            persistent: [0; 32],
            state_format: 11,
        };
        let cancellation = Arc::new(AtomicBool::new(false));
        let reader_flag = Arc::clone(&cancellation);
        let host_identity = identity.clone();
        let worker = thread::spawn(move || {
            admit_cancellable(host, Player::One, &host_identity, &[9; 32], reader_flag).unwrap()
        });
        let mut client = admit(client, Player::Two, &identity, &[9; 32]).unwrap();
        let mut host = worker.join().unwrap();
        client
            .send(&Message::Input {
                player: Player::Two,
                frame: 2,
                buttons: 1,
            })
            .unwrap();
        assert_eq!(
            host.stream.read_timeout().unwrap(),
            Some(Duration::from_secs(2))
        );
        cancellation.store(true, Ordering::Release);
        assert!(
            host.receive()
                .unwrap_err()
                .to_string()
                .contains("cancelled")
        );
        assert!(host.receive().unwrap_err().to_string().contains("terminal"));
        assert!(host.terminal);
        assert_eq!(host.receive_sequence, 0);
    }
}
