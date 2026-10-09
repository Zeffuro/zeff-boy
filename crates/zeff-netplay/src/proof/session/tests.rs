use super::*;
use std::net::TcpListener;

#[test]
fn disconnect_does_not_relabel_unexpected_input_as_injection() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let peer = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
    let host = listener.accept().unwrap().0;
    let identity = Machine::new().unwrap().identity([42; 32]).unwrap();
    let host_identity = identity.clone();
    let worker = thread::spawn(move || {
        let mut connection = admit_scoped(
            host,
            Player::One,
            &host_identity,
            &[9; 32],
            ConnectionScope::Loopback,
        )
        .unwrap();
        connection
            .send(&Message::Input {
                player: Player::One,
                frame: 6 + INPUT_DELAY,
                buttons: u16::from(sample(Player::One, 5)),
            })
            .unwrap();
        assert!(connection.receive().is_err());
    });
    let mut connection = admit_scoped(
        peer,
        Player::Two,
        &identity,
        &[9; 32],
        ConnectionScope::Loopback,
    )
    .unwrap();
    let error = inject_before(&mut connection, 5, Scenario::Disconnect).unwrap_err();
    drop(connection);
    worker.join().unwrap();
    assert_eq!(
        error.to_string(),
        "unexpected input before injected disconnect"
    );
}

#[test]
fn disconnect_releases_admitted_sockets_before_reporting() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let peer = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
    let host = listener.accept().unwrap().0;
    let worker = thread::spawn(move || {
        run_peer(
            host,
            Player::One,
            [42; 32],
            [9; 32],
            24,
            Scenario::Disconnect,
        )
        .unwrap()
    });
    let peer = run_peer(
        peer,
        Player::Two,
        [42; 32],
        [9; 32],
        24,
        Scenario::Disconnect,
    )
    .unwrap();
    let host = worker.join().unwrap();
    assert!(peer.admitted && host.admitted);
    assert_eq!(peer.outcome, "injected disconnect");
    assert_eq!(peer.transport_error, None);
    assert_eq!(peer.frames, 5);
    assert_eq!(host.frames, 5);
    assert_eq!(host.reference_checked_frames, 5);
    assert_eq!(host.checkpoint, peer.checkpoint);
    assert_eq!(host.persistence, "leased-discard");
    assert_eq!(peer.persistence, "leased-discard");
    assert!(
        matches!(
            host.transport_error.as_deref(),
            Some(
                "ConnectionAborted"
                    | "ConnectionReset"
                    | "BrokenPipe"
                    | "NotConnected"
                    | "UnexpectedEof"
                    | "WriteZero"
            )
        ),
        "host outcome={} transport={:?}",
        host.outcome,
        host.transport_error
    );
}
