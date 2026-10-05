use super::*;
use std::net::TcpListener;
use std::time::Instant;

pub(super) fn identity() -> Identity {
    Identity {
        build: [1; 32],
        build_info: Default::default(),
        source: [2; 32],
        effective: [2; 32],
        media_len: 123,
        config: [3; 32],
        initial: [4; 32],
        persistent: [5; 32],
        state_format: 11,
    }
}

fn streams() -> (TcpStream, TcpStream) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let client = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
    let (host, _) = listener.accept().unwrap();
    (host, client)
}

pub(super) fn event(network: &Network) -> Event {
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        if let Some(event) = network.poll().unwrap() {
            return event;
        }
        assert!(Instant::now() < deadline, "network event deadline");
        thread::sleep(Duration::from_millis(1));
    }
}

pub(super) fn input(player: Player, frame: u64) -> Message {
    Message::Input {
        player,
        frame,
        buttons: 0xa5,
    }
}

#[test]
fn admission_and_ordered_input_checkpoint_exchange() {
    let (host, client) = streams();
    let one = Network::spawn(
        host,
        Player::One,
        identity(),
        [9; 32],
        ConnectionScope::Loopback,
    )
    .unwrap();
    let two = Network::spawn(
        client,
        Player::Two,
        identity(),
        [9; 32],
        ConnectionScope::Loopback,
    )
    .unwrap();
    assert!(matches!(event(&one), Event::Ready));
    assert!(matches!(event(&two), Event::Ready));
    for frame in 2..5 {
        one.send(input(Player::One, frame)).unwrap();
        two.send(input(Player::Two, frame)).unwrap();
        assert!(
            matches!(event(&one), Event::Message(message) if message == input(Player::Two, frame))
        );
        assert!(
            matches!(event(&two), Event::Message(message) if message == input(Player::One, frame))
        );
        let checkpoint = Message::Checkpoint {
            frame,
            logical: [1; 32],
            video: [2; 32],
            audio: [3; 32],
            persistent: [4; 32],
        };
        one.send(checkpoint.clone()).unwrap();
        two.send(checkpoint.clone()).unwrap();
        assert!(matches!(event(&one), Event::Message(message) if message == checkpoint));
        assert!(matches!(event(&two), Event::Message(message) if message == checkpoint));
    }
    let deadline = std::time::Instant::now() + Duration::from_secs(1);
    while one.stats().sent < 6 {
        assert!(std::time::Instant::now() < deadline);
        thread::yield_now();
    }
    let stats = one.stats();
    assert!(!stats.datagrams);
    assert_eq!((stats.sent, stats.received), (6, 6));
    assert_eq!(stats.ack_wait, None);
}

#[test]
fn admission_failure_is_reported_and_disconnect_remains_observable() {
    let (host, client) = streams();
    let one = Network::spawn(
        host,
        Player::One,
        identity(),
        [9; 32],
        ConnectionScope::Loopback,
    )
    .unwrap();
    let two = Network::spawn(
        client,
        Player::Two,
        identity(),
        [8; 32],
        ConnectionScope::Loopback,
    )
    .unwrap();
    for network in [&one, &two] {
        assert!(matches!(event(network), Event::Failed(cause) if cause.contains("authentication")));
        let deadline = Instant::now() + Duration::from_secs(1);
        while network.poll().is_ok() {
            assert!(Instant::now() < deadline);
            thread::yield_now();
        }
    }
}

#[test]
fn cancellation_interrupts_pending_handshake_and_read() {
    let (host, _silent_peer) = streams();
    let mut pending = Network::spawn(
        host,
        Player::One,
        identity(),
        [9; 32],
        ConnectionScope::Loopback,
    )
    .unwrap();
    let start = Instant::now();
    pending.cancel();
    assert!(start.elapsed() < Duration::from_secs(1));
    assert!(pending.send(input(Player::One, 2)).is_err());

    let (host, client) = streams();
    let mut one = Network::spawn(
        host,
        Player::One,
        identity(),
        [9; 32],
        ConnectionScope::Loopback,
    )
    .unwrap();
    let mut peer = wire::admit(client, Player::Two, &identity(), &[9; 32]).unwrap();
    assert!(matches!(event(&one), Event::Ready));
    one.send(input(Player::One, 2)).unwrap();
    assert_eq!(peer.receive().unwrap(), input(Player::One, 2));
    let start = Instant::now();
    one.cancel();
    assert!(start.elapsed() < Duration::from_secs(1));
}

#[test]
fn outbound_pipeline_and_unsolicited_inbound_need_no_matching_reply() {
    let (host, client) = streams();
    let network = Network::spawn(
        host,
        Player::One,
        identity(),
        [9; 32],
        ConnectionScope::Loopback,
    )
    .unwrap();
    let mut peer = wire::admit(client, Player::Two, &identity(), &[9; 32]).unwrap();
    assert!(matches!(event(&network), Event::Ready));
    for frame in 2..10 {
        network.send(input(Player::One, frame)).unwrap();
    }
    for frame in 2..10 {
        assert_eq!(peer.receive().unwrap(), input(Player::One, frame));
    }
    peer.send(&Message::Progress {
        frame: 8,
        confirmed: 6,
    })
    .unwrap();
    assert!(matches!(
        event(&network),
        Event::Message(Message::Progress {
            frame: 8,
            confirmed: 6
        })
    ));
    peer.send(&input(Player::Two, 19)).unwrap();
    assert!(
        matches!(event(&network), Event::Message(message) if message == input(Player::Two, 19))
    );
}

#[test]
fn unsolicited_close_stops_both_workers_without_waiting_for_acknowledgement() {
    let (host, client) = streams();
    let mut network = Network::spawn(
        host,
        Player::One,
        identity(),
        [9; 32],
        ConnectionScope::Loopback,
    )
    .unwrap();
    let mut peer = wire::admit(client, Player::Two, &identity(), &[9; 32]).unwrap();
    assert!(matches!(event(&network), Event::Ready));
    peer.send(&Message::Close { frame: 9 }).unwrap();
    assert!(matches!(
        event(&network),
        Event::Message(Message::Close { frame: 9 })
    ));
    let start = Instant::now();
    network.cancel();
    assert!(start.elapsed() < Duration::from_secs(1));
    assert!(network.failure.lock().unwrap().is_none());
    assert!(network.poll().is_err());
}

#[test]
fn concurrent_close_cancel_and_socket_shutdown_join_without_spinning() {
    for _ in 0..8 {
        let (host, client) = streams();
        let mut one = Network::spawn(
            host,
            Player::One,
            identity(),
            [9; 32],
            ConnectionScope::Loopback,
        )
        .unwrap();
        let mut two = Network::spawn(
            client,
            Player::Two,
            identity(),
            [9; 32],
            ConnectionScope::Loopback,
        )
        .unwrap();
        assert!(matches!(event(&one), Event::Ready));
        assert!(matches!(event(&two), Event::Ready));
        one.send(Message::Close { frame: 10 }).unwrap();
        let start = Instant::now();
        let peer = thread::spawn(move || two.cancel());
        one.cancel();
        peer.join().unwrap();
        assert!(start.elapsed() < Duration::from_secs(1));
    }
}

#[test]
fn silent_peer_expires_receive_deadline_without_network_generated_heartbeat() {
    let (host, client) = streams();
    let network = Network::spawn(
        host,
        Player::One,
        identity(),
        [9; 32],
        ConnectionScope::Loopback,
    )
    .unwrap();
    let _peer = wire::admit(client, Player::Two, &identity(), &[9; 32]).unwrap();
    assert!(matches!(event(&network), Event::Ready));
    assert!(matches!(event(&network), Event::Failed(_)));
}

#[test]
fn outbound_overflow_cancels_without_blocking() {
    let (host, _silent_peer) = streams();
    let mut network = Network::spawn(
        host,
        Player::One,
        identity(),
        [9; 32],
        ConnectionScope::Loopback,
    )
    .unwrap();
    for frame in 0..OUTBOUND_CAPACITY {
        network.send(input(Player::One, frame as u64)).unwrap();
    }
    let start = Instant::now();
    assert!(
        network
            .send(input(Player::One, 4))
            .unwrap_err()
            .to_string()
            .contains("overflow")
    );
    network.cancel();
    assert!(start.elapsed() < Duration::from_secs(1));
}

#[test]
fn inbound_overflow_preserves_cause_when_failed_event_cannot_fit() {
    let (host, client) = streams();
    let mut network = Network::spawn(
        host,
        Player::One,
        identity(),
        [9; 32],
        ConnectionScope::Loopback,
    )
    .unwrap();
    let mut peer = wire::admit(client, Player::Two, &identity(), &[9; 32]).unwrap();
    assert!(matches!(event(&network), Event::Ready));
    network.send(input(Player::One, 2)).unwrap();
    assert_eq!(peer.receive().unwrap(), input(Player::One, 2));
    for frame in 10..(10 + INBOUND_CAPACITY as u64 + 1) {
        if peer.send(&input(Player::Two, frame)).is_err() {
            break;
        }
    }
    let deadline = Instant::now() + Duration::from_secs(3);
    while !network.worker.as_ref().unwrap().is_finished() {
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(1));
    }
    for _ in 0..INBOUND_CAPACITY {
        assert!(matches!(network.poll().unwrap(), Some(Event::Message(_))));
    }
    assert!(
        network
            .poll()
            .unwrap_err()
            .to_string()
            .contains("inbound queue overflow")
    );
    network.cancel();
}

fn admitted_pair() -> (wire::Connection, wire::Connection) {
    let (host, client) = streams();
    let admission =
        thread::spawn(move || wire::admit(host, Player::One, &identity(), &[9; 32]).unwrap());
    let peer = wire::admit(client, Player::Two, &identity(), &[9; 32]).unwrap();
    (admission.join().unwrap(), peer)
}

#[test]
fn reader_protocol_failure_survives_send_before_completion_publication() {
    let (connection, mut peer) = admitted_pair();
    let (mut sender, mut receiver) = connection.split().unwrap();
    let (finished, completion) = crossbeam_channel::bounded(1);
    peer.send_invalid_length_for_test().unwrap();
    let reader_result = receiver
        .receive()
        .map(|_| ())
        .context("receiving netplay message");
    assert!(format!("{:#}", reader_result.as_ref().unwrap_err()).contains("invalid packet length"));
    assert!(matches!(
        completion.try_recv(),
        Err(crossbeam_channel::TryRecvError::Empty)
    ));
    let result = sender
        .send(&input(Player::One, 2))
        .context("sending netplay message");
    assert!(
        result
            .as_ref()
            .unwrap_err()
            .is::<wire::ConnectionTerminated>()
    );
    finished.send(reader_result).unwrap();
    let error = finish_connection(result, completion.try_recv().ok(), false).unwrap_err();
    assert_eq!(
        format!("{error:#}"),
        "receiving netplay message: invalid packet length"
    );
}

#[test]
fn genuine_send_failure_survives_reader_shutdown_completion() {
    let (connection, _peer) = admitted_pair();
    let (mut sender, mut receiver) = connection.split().unwrap();
    let result = sender
        .send(&input(Player::Two, 2))
        .context("sending netplay message");
    assert!(
        !result
            .as_ref()
            .unwrap_err()
            .is::<wire::ConnectionTerminated>()
    );
    let reader_result = receiver
        .receive()
        .map(|_| ())
        .context("receiving netplay message");
    assert!(
        reader_result
            .as_ref()
            .unwrap_err()
            .is::<wire::ConnectionTerminated>()
    );
    let error = finish_connection(result, Some(reader_result), false).unwrap_err();
    assert_eq!(
        format!("{error:#}"),
        "sending netplay message: input player does not own this port"
    );
}

#[test]
fn intentional_stop_ignores_reader_shutdown_completion() {
    let (connection, _peer) = admitted_pair();
    let (sender, mut receiver) = connection.split().unwrap();
    drop(sender);
    let reader_result = receiver.receive().map(|_| ());
    assert!(reader_result.is_err());
    finish_connection(Ok(()), Some(reader_result), true).unwrap();
}
