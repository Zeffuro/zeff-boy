use super::*;
use std::net::Shutdown;
use std::sync::atomic::Ordering;

#[test]
fn deferred_close_rejects_inflight_gameplay_without_cancelling_delivery() {
    let (host, mut client) = pair();
    let socket = host.stream.try_clone().unwrap();
    let cancelled = Arc::new(AtomicBool::new(false));
    let mut host = host;
    host.cancellation = Some(cancelled.clone());
    let (mut sender, mut receiver) = host.split_deferred().unwrap();
    client.send(&input(Player::Two)).unwrap();
    sender.send(&Message::Close { frame: 12 }).unwrap();
    assert!(receiver.receive().is_err());
    assert!(!cancelled.load(Ordering::Acquire));
    assert_eq!(client.receive().unwrap(), Message::Close { frame: 12 });
    assert!(sender.send(&input(Player::One)).is_err());
    assert!(receiver.receive().is_err());
    socket.shutdown(Shutdown::Both).unwrap();
}

#[test]
fn deferred_receive_publishes_close_and_owner_shutdown_interrupts_io() {
    let (host, mut client) = pair();
    let socket = host.stream.try_clone().unwrap();
    let (mut sender, mut receiver) = host.split_deferred().unwrap();
    client.send(&Message::Close { frame: 24 }).unwrap();
    assert_eq!(receiver.receive().unwrap(), Message::Close { frame: 24 });
    sender.send(&Message::Close { frame: 24 }).unwrap();
    assert_eq!(client.receive().unwrap(), Message::Close { frame: 24 });
    assert!(sender.send(&input(Player::One)).is_err());
    assert!(receiver.receive().is_err());
    socket.shutdown(Shutdown::Both).unwrap();

    let (host, _client) = pair();
    let socket = host.stream.try_clone().unwrap();
    let (sender, mut receiver) = host.split_deferred().unwrap();
    let reader = thread::spawn(move || receiver.receive());
    drop(sender);
    let started = Instant::now();
    socket.shutdown(Shutdown::Both).unwrap();
    assert!(reader.join().unwrap().is_err());
    assert!(started.elapsed() < Duration::from_secs(1));
}

#[test]
fn deferred_split_preserves_mac_role_sequence_and_length_fences() {
    for field in 0..4 {
        let (mut host, client) = pair();
        let socket = client.stream.try_clone().unwrap();
        let (mut sender, mut receiver) = client.split_deferred().unwrap();
        let mut packet = host.encode(&Message::Progress {
            frame: 9,
            confirmed: 8,
        });
        match field {
            0 => packet[38] = 2,
            1 => packet[46] = 1,
            2 => packet[HEADER_LEN] ^= 1,
            3 => {
                packet.remove(HEADER_LEN);
                resign(&mut packet);
            }
            _ => unreachable!(),
        }
        write_packet(&mut host, &packet);
        assert!(receiver.receive().is_err(), "field {field}");
        assert!(receiver.receive().is_err());
        assert!(sender.send(&input(Player::Two)).is_err());
        socket.shutdown(Shutdown::Both).unwrap();
    }
}

#[test]
fn owner_cancellation_interrupts_deferred_receiver_and_invalid_progress_is_terminal() {
    let (mut host, _client) = pair();
    let socket = host.stream.try_clone().unwrap();
    let cancelled = Arc::new(AtomicBool::new(false));
    host.cancellation = Some(cancelled.clone());
    let (_sender, mut receiver) = host.split_deferred().unwrap();
    let (started, waiting) = std::sync::mpsc::channel();
    let reader = thread::spawn(move || {
        started.send(()).unwrap();
        receiver.receive()
    });
    waiting.recv_timeout(Duration::from_secs(1)).unwrap();
    let start = Instant::now();
    cancelled.store(true, Ordering::Release);
    assert!(
        reader
            .join()
            .unwrap()
            .unwrap_err()
            .to_string()
            .contains("cancelled")
    );
    assert!(start.elapsed() < Duration::from_secs(1));
    socket.shutdown(Shutdown::Both).unwrap();

    let (mut host, client) = pair();
    let socket = client.stream.try_clone().unwrap();
    let (mut sender, mut receiver) = client.split_deferred().unwrap();
    let packet = host.encode(&Message::Progress {
        frame: 9,
        confirmed: 10,
    });
    write_packet(&mut host, &packet);
    assert!(
        receiver
            .receive()
            .unwrap_err()
            .to_string()
            .contains("confirmed progress")
    );
    assert!(sender.send(&input(Player::Two)).is_err());
    assert!(receiver.receive().is_err());
    socket.shutdown(Shutdown::Both).unwrap();
}
