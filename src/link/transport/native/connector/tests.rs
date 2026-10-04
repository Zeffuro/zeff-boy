use super::*;
use crate::link::transport::LinkTransport;
use std::io::{Read, Write};

fn wait_finished(connector: &TcpLinkConnector) {
    let deadline = Instant::now() + Duration::from_secs(2);
    while !connector.worker.as_ref().unwrap().is_finished() {
        assert!(Instant::now() < deadline, "TCP connector deadline");
        thread::yield_now();
    }
}

fn host(budget: Duration) -> (TcpLinkConnector, SocketAddr) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    (
        TcpLinkConnector::host_listener(listener, budget).unwrap(),
        addr,
    )
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

#[test]
fn numeric_addresses_and_localhost_do_not_require_dns() {
    assert_eq!(numeric_address("127.0.0.1:8765").unwrap().port(), 8765);
    assert_eq!(
        numeric_address("localhost:8765").unwrap(),
        numeric_address("127.0.0.1:8765").unwrap()
    );
    assert_eq!(
        numeric_address("[::1]:8765").unwrap().ip(),
        std::net::Ipv6Addr::LOCALHOST
    );
    for addr in ["example.com:8765", "localhost:invalid", "127.0.0.1", ""] {
        assert_eq!(
            numeric_address(addr).unwrap_err().kind(),
            io::ErrorKind::InvalidInput
        );
        assert!(TcpLinkConnector::join(addr).is_err());
        assert!(TcpLinkConnector::host(addr).is_err());
    }
}

#[test]
fn cancel_pending_host_joins_worker_and_immediately_releases_port() {
    let (mut connector, addr) = host(CONNECT_BUDGET);
    assert!(connector.poll().unwrap().is_none());
    let started = Instant::now();
    connector.cancel();
    assert!(started.elapsed() < Duration::from_secs(2));
    assert!(connector.worker.is_none());
    assert!(connector.poll().unwrap().is_none());
    connector.cancel();
    let rebound = TcpListener::bind(addr).unwrap();
    drop(rebound);
}

#[test]
fn drop_pending_host_releases_listener() {
    let (connector, addr) = host(CONNECT_BUDGET);
    let started = Instant::now();
    drop(connector);
    assert!(started.elapsed() < Duration::from_secs(2));
    let rebound = TcpListener::bind(addr).unwrap();
    drop(rebound);
}

#[test]
fn cancel_completed_accept_drops_queued_transport_and_joins_reader() {
    let (mut connector, addr) = host(CONNECT_BUDGET);
    let mut peer = TcpStream::connect(addr).unwrap();
    peer.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
    peer.write_all(&[8, 0, 1]).unwrap();
    wait_finished(&connector);
    let started = Instant::now();
    connector.cancel();
    assert!(started.elapsed() < Duration::from_secs(2));
    assert!(connector.worker.is_none());
    assert!(connector.poll().unwrap().is_none());
    assert_closed(&mut peer);
}

#[test]
fn refused_join_is_cancellable_and_leaves_no_owned_worker() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener);
    let (attempt_tx, attempt_rx) = std::sync::mpsc::channel();
    let mut connector = TcpLinkConnector::spawn(move |cancelled| {
        wait_for_stream(cancelled, CONNECT_BUDGET, |remaining| {
            let result = TcpStream::connect_timeout(&addr, remaining.min(ATTEMPT_LIMIT));
            attempt_tx
                .send(result.as_ref().err().map(io::Error::kind))
                .unwrap();
            result
        })
    })
    .unwrap();
    assert!(matches!(
        attempt_rx.recv_timeout(Duration::from_secs(2)).unwrap(),
        Some(io::ErrorKind::ConnectionRefused | io::ErrorKind::TimedOut)
    ));
    let started = Instant::now();
    connector.cancel();
    assert!(started.elapsed() < Duration::from_secs(2));
    assert!(connector.worker.is_none());
    assert!(connector.poll().unwrap().is_none());
}

#[test]
fn repeated_refused_attempts_end_at_the_connection_deadline() {
    let (attempt_tx, attempt_rx) = std::sync::mpsc::channel();
    let mut connector = TcpLinkConnector::spawn(move |cancelled| {
        wait_for_stream(cancelled, Duration::from_millis(20), |remaining| {
            assert!(remaining <= Duration::from_millis(20));
            attempt_tx.send(()).unwrap();
            Err(io::Error::from(io::ErrorKind::ConnectionRefused))
        })
    })
    .unwrap();
    attempt_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    wait_finished(&connector);
    assert!(matches!(connector.poll(), Err(error) if error.kind() == io::ErrorKind::TimedOut));
    assert!(connector.worker.is_none());
}

#[test]
fn host_deadline_error_joins_worker_and_releases_port() {
    let (mut connector, addr) = host(Duration::from_millis(20));
    wait_finished(&connector);
    let error = match connector.poll() {
        Err(error) => error,
        _ => panic!("expected TCP host timeout"),
    };
    assert_eq!(error.kind(), io::ErrorKind::TimedOut);
    assert!(connector.worker.is_none());
    let rebound = TcpListener::bind(addr).unwrap();
    drop(rebound);
}

#[test]
fn host_and_join_publish_one_transport_each_and_keep_packet_framing() {
    let (mut host, addr) = host(CONNECT_BUDGET);
    let mut client = TcpLinkConnector::join(&addr.to_string()).unwrap();
    wait_finished(&host);
    wait_finished(&client);
    let mut host_transport = host.poll().unwrap().unwrap();
    let mut client_transport = client.poll().unwrap().unwrap();
    assert!(host.worker.is_none());
    assert!(client.worker.is_none());
    assert!(host.poll().unwrap().is_none());
    assert!(client.poll().unwrap().is_none());
    client_transport.send(&[1, 2, 3]).unwrap();
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        if let Some(packet) = host_transport.try_receive().unwrap() {
            assert_eq!(packet, [1, 2, 3]);
            break;
        }
        assert!(Instant::now() < deadline);
        thread::yield_now();
    }
}
