use super::*;
use std::io::Read;
use std::time::{Duration, Instant};

fn pair() -> (TcpLinkTransport, TcpStream) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let peer = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
    let link = TcpLinkTransport::accept_once(listener).unwrap();
    peer.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
    (link, peer)
}

fn recv_with_timeout(link: &mut TcpLinkTransport) -> Vec<u8> {
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        if let Some(packet) = link.try_receive().unwrap() {
            return packet;
        }
        assert!(Instant::now() < deadline, "TCP link packet deadline");
        thread::yield_now();
    }
}

fn assert_closed(peer: &mut TcpStream) {
    match peer.read(&mut [0; 1]) {
        Ok(0) => {}
        Err(error)
            if matches!(
                error.kind(),
                io::ErrorKind::ConnectionReset | io::ErrorKind::ConnectionAborted
            ) => {}
        result => panic!("peer remains open: {result:?}"),
    }
}

fn wait_progress(link: &TcpLinkTransport, predicate: impl Fn(&read::ReaderProgress) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(2);
    while !predicate(&link.reader_progress) {
        assert!(Instant::now() < deadline, "reader progress deadline");
        thread::yield_now();
    }
}

fn wait_partial(link: &TcpLinkTransport, bytes: usize) {
    wait_progress(link, |progress| {
        progress.bytes.load(Ordering::Acquire) == bytes
    });
    let timeouts = link.reader_progress.timeouts.load(Ordering::Acquire);
    wait_progress(link, |progress| {
        progress.timeouts.load(Ordering::Acquire) >= timeouts + 2
    });
}

#[test]
fn tcp_link_transport_moves_framed_packets_between_endpoints() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let host_thread = thread::spawn(move || TcpLinkTransport::accept_once(listener).unwrap());
    let mut client = TcpLinkTransport::connect(addr).unwrap();
    let mut host = host_thread.join().unwrap();
    for packet in [&[][..], &[0x01, 0x02, 0x03], &[0xa0]] {
        client.send(packet).unwrap();
        assert_eq!(recv_with_timeout(&mut host), packet);
        host.send(packet).unwrap();
        assert_eq!(recv_with_timeout(&mut client), packet);
    }
    let packet = vec![0x5a; MAX_TCP_PACKET_LEN];
    client.send(&packet).unwrap();
    assert_eq!(recv_with_timeout(&mut host), packet);
}

#[test]
fn disconnect_joins_reader_with_idle_partial_length_or_partial_payload_peer() {
    for partial in [&[][..], &[5], &[5, 0, 1, 2]] {
        let (mut link, mut peer) = pair();
        peer.write_all(partial).unwrap();
        wait_partial(&link, partial.len());
        let started = Instant::now();
        link.disconnect();
        assert!(started.elapsed() < Duration::from_secs(2));
        assert!(link.reader.is_none());
        assert_eq!(link.state(), LinkConnectionState::Disconnected);
        assert_eq!(link.try_receive(), Err(LinkTransportError::Disconnected));
        assert_eq!(link.send(&[1]), Err(LinkTransportError::Disconnected));
        link.disconnect();
        assert_closed(&mut peer);
    }
}

#[test]
fn drop_joins_reader_with_idle_and_partial_frames() {
    for partial in [&[][..], &[5], &[5, 0, 1, 2]] {
        let (link, mut peer) = pair();
        let connected = Arc::clone(&link.connected);
        peer.write_all(partial).unwrap();
        wait_partial(&link, partial.len());
        let started = Instant::now();
        drop(link);
        assert!(started.elapsed() < Duration::from_secs(2));
        assert!(!connected.load(Ordering::Relaxed));
        assert_closed(&mut peer);
    }
}

#[test]
fn partial_prefix_and_payload_survive_multiple_readiness_intervals() {
    let (mut link, mut peer) = pair();
    peer.write_all(&[5]).unwrap();
    wait_partial(&link, 1);
    assert_eq!(link.try_receive().unwrap(), None);
    peer.write_all(&[0, 0x11, 0x22]).unwrap();
    wait_partial(&link, 4);
    assert_eq!(link.try_receive().unwrap(), None);
    peer.write_all(&[0x33, 0x44, 0x55]).unwrap();
    assert_eq!(recv_with_timeout(&mut link), [0x11, 0x22, 0x33, 0x44, 0x55]);
    peer.write_all(&[0, 0, 1, 0, 0xaa]).unwrap();
    assert!(recv_with_timeout(&mut link).is_empty());
    assert_eq!(recv_with_timeout(&mut link), [0xaa]);
}

#[test]
fn actual_eof_mid_prefix_or_payload_never_publishes_partial_packet() {
    for partial in [&[5][..], &[5, 0, 1, 2]] {
        let (mut link, mut peer) = pair();
        peer.write_all(partial).unwrap();
        wait_partial(&link, partial.len());
        peer.shutdown(Shutdown::Write).unwrap();
        let deadline = Instant::now() + Duration::from_secs(2);
        while link.state() == LinkConnectionState::Connected {
            assert!(Instant::now() < deadline, "partial frame EOF deadline");
            thread::yield_now();
        }
        assert_eq!(link.try_receive(), Err(LinkTransportError::Disconnected));
        link.disconnect();
        assert!(link.reader.is_none());
    }
}

#[test]
fn reader_cancellation_exits_after_scheduled_wait_without_socket_shutdown() {
    for partial in [&[][..], &[5], &[5, 0, 1, 2]] {
        let (mut link, mut peer) = pair();
        peer.write_all(partial).unwrap();
        wait_partial(&link, partial.len());
        link.connected.store(false, Ordering::Release);
        let deadline = Instant::now() + Duration::from_secs(2);
        while !link.reader.as_ref().unwrap().is_finished() {
            assert!(Instant::now() < deadline, "reader cancellation deadline");
            thread::yield_now();
        }
        assert_eq!(link.try_receive(), Err(LinkTransportError::Disconnected));
        link.disconnect();
        assert!(link.reader.is_none());
        assert_closed(&mut peer);
    }
}
