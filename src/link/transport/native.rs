use std::io::{self, Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream, ToSocketAddrs};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::thread::{self, JoinHandle};

use super::{LinkConnectionState, LinkTransport, LinkTransportError};

const MAX_TCP_PACKET_LEN: usize = u16::MAX as usize;

pub(crate) const DEFAULT_TCP_LINK_ADDR: &str = "127.0.0.1:8765";

pub(crate) struct TcpLinkTransport {
    writer_stream: TcpStream,
    inbound: Receiver<Vec<u8>>,
    connected: Arc<AtomicBool>,
    shutdown_stream: TcpStream,
}

impl TcpLinkTransport {
    pub(crate) fn connect(addr: impl ToSocketAddrs) -> io::Result<Self> {
        Self::from_stream(TcpStream::connect(addr)?)
    }

    pub(crate) fn host_once(addr: impl ToSocketAddrs) -> io::Result<Self> {
        let listener = TcpListener::bind(addr)?;
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
