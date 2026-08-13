use std::io::{self, Read, Write};
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream, ToSocketAddrs};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::thread::{self, JoinHandle};

use super::{LinkConnectionState, LinkTransport, LinkTransportError};

const MAX_TCP_PACKET_LEN: usize = u16::MAX as usize;
const CONNECT_RETRY_ATTEMPTS: usize = 40;
const CONNECT_RETRY_DELAY: std::time::Duration = std::time::Duration::from_millis(50);

pub(crate) const DEFAULT_TCP_LINK_ADDR: &str = "127.0.0.1:8765";

pub(crate) struct TcpLinkTransport {
    writer_stream: TcpStream,
    inbound: Receiver<Vec<u8>>,
    connected: Arc<AtomicBool>,
    shutdown_stream: TcpStream,
}

impl TcpLinkTransport {
    pub(crate) fn connect(addr: impl ToSocketAddrs) -> io::Result<Self> {
        let addrs = resolve_addrs(addr)?;
        Self::from_stream(connect_with_retries(&addrs)?)
    }

    pub(crate) fn host_once(addr: impl ToSocketAddrs) -> io::Result<Self> {
        let addrs = resolve_addrs(addr)?;
        let listener = bind_with_retries(&addrs)?;
        Self::accept_once(listener)
    }

    pub(crate) fn accept_once(listener: TcpListener) -> io::Result<Self> {
        let (stream, _) = listener.accept()?;
        Self::from_stream(stream)
    }

    pub(crate) fn from_stream(stream: TcpStream) -> io::Result<Self> {
        stream.set_nodelay(true)?;
        let reader_stream = stream.try_clone()?;
        let writer_stream = stream.try_clone()?;
        let shutdown_stream = stream;
        let (inbound_tx, inbound) = mpsc::channel();
        let connected = Arc::new(AtomicBool::new(true));

        let reader_connected = Arc::clone(&connected);
        let _reader: JoinHandle<()> = thread::spawn(move || {
            read_packets(reader_stream, inbound_tx, reader_connected);
        });

        Ok(Self {
            writer_stream,
            inbound,
            connected,
            shutdown_stream,
        })
    }
}

fn resolve_addrs(addr: impl ToSocketAddrs) -> io::Result<Vec<SocketAddr>> {
    let addrs: Vec<_> = addr.to_socket_addrs()?.collect();
    if addrs.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "link address did not resolve to any socket address",
        ));
    }
    Ok(addrs)
}

fn connect_with_retries(addrs: &[SocketAddr]) -> io::Result<TcpStream> {
    retry_transient_link_open(|| try_connect_once(addrs))
}

fn bind_with_retries(addrs: &[SocketAddr]) -> io::Result<TcpListener> {
    retry_transient_link_open(|| try_bind_once(addrs))
}

fn retry_transient_link_open<T>(mut attempt: impl FnMut() -> io::Result<T>) -> io::Result<T> {
    let mut last_err = None;
    for attempt_index in 0..CONNECT_RETRY_ATTEMPTS {
        match attempt() {
            Ok(value) => return Ok(value),
            Err(err) => {
                let transient = is_transient_link_open_error(&err);
                last_err = Some(err);
                if !transient || attempt_index + 1 == CONNECT_RETRY_ATTEMPTS {
                    break;
                }
                thread::sleep(CONNECT_RETRY_DELAY);
            }
        }
    }
    Err(last_err.unwrap_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "link address did not resolve to any socket address",
        )
    }))
}

fn try_connect_once(addrs: &[SocketAddr]) -> io::Result<TcpStream> {
    let mut last_err = None;
    for addr in addrs {
        match TcpStream::connect(addr) {
            Ok(stream) => return Ok(stream),
            Err(err) => last_err = Some(err),
        }
    }
    Err(last_err.unwrap_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "link address did not resolve to any socket address",
        )
    }))
}

fn try_bind_once(addrs: &[SocketAddr]) -> io::Result<TcpListener> {
    let mut last_err = None;
    for addr in addrs {
        match TcpListener::bind(addr) {
            Ok(listener) => return Ok(listener),
            Err(err) => last_err = Some(err),
        }
    }
    Err(last_err.unwrap_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "link address did not resolve to any socket address",
        )
    }))
}

fn is_transient_link_open_error(err: &io::Error) -> bool {
    matches!(
        err.kind(),
        io::ErrorKind::AddrInUse | io::ErrorKind::ConnectionRefused | io::ErrorKind::TimedOut
    )
}

impl LinkTransport for TcpLinkTransport {
    fn state(&self) -> LinkConnectionState {
        if self.connected.load(Ordering::Relaxed) {
            LinkConnectionState::Connected
        } else {
            LinkConnectionState::Disconnected
        }
    }

    fn send(&mut self, packet: &[u8]) -> Result<(), LinkTransportError> {
        if self.state() == LinkConnectionState::Disconnected {
            return Err(LinkTransportError::Disconnected);
        }

        if packet.len() > MAX_TCP_PACKET_LEN {
            return Err(LinkTransportError::Disconnected);
        }

        let len = u16::try_from(packet.len()).map_err(|_| LinkTransportError::Disconnected)?;
        if self.writer_stream.write_all(&len.to_le_bytes()).is_err()
            || self.writer_stream.write_all(packet).is_err()
        {
            self.connected.store(false, Ordering::Relaxed);
            return Err(LinkTransportError::Disconnected);
        }

        Ok(())
    }

    fn try_receive(&mut self) -> Result<Option<Vec<u8>>, LinkTransportError> {
        match self.inbound.try_recv() {
            Ok(packet) => Ok(Some(packet)),
            Err(TryRecvError::Empty) => {
                if self.state() == LinkConnectionState::Connected {
                    Ok(None)
                } else {
                    Err(LinkTransportError::Disconnected)
                }
            }
            Err(TryRecvError::Disconnected) => {
                self.connected.store(false, Ordering::Relaxed);
                Err(LinkTransportError::Disconnected)
            }
        }
    }

    fn disconnect(&mut self) {
        self.connected.store(false, Ordering::Relaxed);
        let _ = self.shutdown_stream.shutdown(Shutdown::Both);
    }
}

impl Drop for TcpLinkTransport {
    fn drop(&mut self) {
        self.disconnect();
    }
}

fn read_packets(mut stream: TcpStream, inbound: Sender<Vec<u8>>, connected: Arc<AtomicBool>) {
    while connected.load(Ordering::Relaxed) {
        let mut len_bytes = [0; 2];
        if stream.read_exact(&mut len_bytes).is_err() {
            connected.store(false, Ordering::Relaxed);
            break;
        }

        let len = usize::from(u16::from_le_bytes(len_bytes));
        let mut packet = vec![0; len];
        if stream.read_exact(&mut packet).is_err() {
            connected.store(false, Ordering::Relaxed);
            break;
        }

        if inbound.send(packet).is_err() {
            connected.store(false, Ordering::Relaxed);
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn recv_with_timeout(link: &mut TcpLinkTransport) -> Vec<u8> {
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        loop {
            if let Some(packet) = link.try_receive().unwrap() {
                return packet;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "timed out waiting for TCP link packet"
            );
            thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn tcp_link_transport_moves_framed_packets_between_endpoints() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let host_thread = thread::spawn(move || TcpLinkTransport::accept_once(listener).unwrap());

        let mut client = TcpLinkTransport::connect(addr).unwrap();
        let mut host = host_thread.join().unwrap();

        client.send(&[0x01, 0x02, 0x03]).unwrap();
        host.send(&[0xA0]).unwrap();

        assert_eq!(recv_with_timeout(&mut host), vec![0x01, 0x02, 0x03]);
        assert_eq!(recv_with_timeout(&mut client), vec![0xA0]);
    }
}
