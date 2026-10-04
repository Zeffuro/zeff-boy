use super::*;
use std::net::{Shutdown, TcpListener};
use std::thread;

fn streams() -> (TcpStream, TcpStream) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let client = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
    let (host, _) = listener.accept().unwrap();
    (host, client)
}

#[test]
fn idle_probe_preserves_pending_bytes_and_observes_fin_after_drain() {
    let (mut host, mut peer) = streams();
    let configured = Duration::from_secs(1);
    host.set_read_timeout(Some(configured)).unwrap();
    let mut driver = Driver::new(&host).unwrap();
    assert!(!driver.peer_disconnected(&host).unwrap());
    assert_eq!(host.read_timeout().unwrap(), Some(configured));
    peer.write_all(b"kept").unwrap();
    peer.shutdown(Shutdown::Both).unwrap();
    for _ in 0..3 {
        assert!(!driver.peer_disconnected(&host).unwrap());
    }
    let mut bytes = [0; 4];
    driver
        .read(&mut host, &mut bytes, Instant::now() + configured, None)
        .unwrap();
    assert_eq!(&bytes, b"kept");
    let deadline = Instant::now() + configured;
    while !driver.peer_disconnected(&host).unwrap() {
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(1));
    }
}

#[test]
fn partial_read_survives_readiness_timeouts_and_changes_direction() {
    let (mut host, mut peer) = streams();
    let configured = Duration::from_millis(20);
    host.set_read_timeout(Some(configured)).unwrap();
    let mut driver = Driver::new(&host).unwrap();
    peer.write_all(b"first").unwrap();
    let writer = thread::spawn(move || {
        thread::sleep(Duration::from_millis(180));
        peer.write_all(b"second").unwrap();
        let mut acknowledgment = [0; 2];
        peer.read_exact(&mut acknowledgment).unwrap();
        assert_eq!(&acknowledgment, b"ok");
    });
    let deadline = Instant::now() + Duration::from_secs(2);
    let mut bytes = [0; 11];
    let cancellation = AtomicBool::new(false);
    driver
        .read(&mut host, &mut bytes, deadline, Some(&cancellation))
        .unwrap();
    assert_eq!(&bytes, b"firstsecond");
    driver
        .write(&mut host, b"ok", deadline, Some(&cancellation))
        .unwrap();
    writer.join().unwrap();
    assert_eq!(host.read_timeout().unwrap(), Some(configured));
}

#[test]
fn large_write_preserves_all_bytes_after_a_slow_reader() {
    let (mut host, mut peer) = streams();
    host.set_write_timeout(Some(Duration::from_millis(20)))
        .unwrap();
    let mut driver = Driver::new(&host).unwrap();
    let bytes: Vec<_> = (0..4 * 1024 * 1024).map(|i| (i % 251) as u8).collect();
    let reader = thread::spawn(move || {
        thread::sleep(Duration::from_millis(180));
        let mut received = vec![0; 4 * 1024 * 1024];
        peer.read_exact(&mut received).unwrap();
        assert!(
            received
                .iter()
                .enumerate()
                .all(|(i, byte)| *byte == (i % 251) as u8)
        );
        peer.write_all(b"ok").unwrap();
    });
    let deadline = Instant::now() + Duration::from_secs(3);
    let cancellation = AtomicBool::new(false);
    driver
        .write(&mut host, &bytes, deadline, Some(&cancellation))
        .unwrap();
    let mut acknowledgment = [0; 2];
    driver
        .read(
            &mut host,
            &mut acknowledgment,
            deadline,
            Some(&cancellation),
        )
        .unwrap();
    assert_eq!(&acknowledgment, b"ok");
    reader.join().unwrap();
}

#[cfg(windows)]
#[test]
fn repeated_idle_probes_and_direction_changes_preserve_the_stream() {
    let (mut host, mut peer) = streams();
    host.set_nodelay(true).unwrap();
    peer.set_nodelay(true).unwrap();
    let mut driver = Driver::new(&host).unwrap();
    let echo = thread::spawn(move || {
        for _ in 0..128 {
            let mut byte = [0];
            peer.read_exact(&mut byte).unwrap();
            peer.write_all(&byte).unwrap();
        }
    });
    let deadline = Instant::now() + Duration::from_secs(5);
    for value in 0..128 {
        for _ in 0..8 {
            assert!(!driver.peer_disconnected(&host).unwrap());
        }
        driver.write(&mut host, &[value], deadline, None).unwrap();
        let mut response = [0];
        driver
            .read(&mut host, &mut response, deadline, None)
            .unwrap();
        assert_eq!(response, [value]);
    }
    echo.join().unwrap();
}

#[cfg(windows)]
#[test]
fn configured_read_timeout_does_not_damage_the_nonblocking_socket() {
    let (mut host, mut peer) = streams();
    let configured = Duration::from_millis(20);
    host.set_read_timeout(Some(configured)).unwrap();
    let mut driver = Driver::new(&host).unwrap();
    let start = Instant::now();
    let error = driver
        .read(&mut host, &mut [0], start + Duration::from_secs(1), None)
        .unwrap_err();
    assert_eq!(
        error.downcast_ref::<std::io::Error>().unwrap().kind(),
        ErrorKind::TimedOut
    );
    assert!(start.elapsed() < Duration::from_millis(500));
    peer.write_all(b"x").unwrap();
    let mut byte = [0];
    driver
        .read(
            &mut host,
            &mut byte,
            Instant::now() + Duration::from_secs(1),
            Some(&AtomicBool::new(false)),
        )
        .unwrap();
    assert_eq!(&byte, b"x");
    assert_eq!(host.read_timeout().unwrap(), Some(configured));
}
