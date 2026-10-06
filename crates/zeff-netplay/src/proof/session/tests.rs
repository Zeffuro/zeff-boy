use super::*;
use std::net::TcpListener;

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
    assert!(matches!(
        host.transport_error.as_deref(),
        Some(
            "ConnectionAborted"
                | "ConnectionReset"
                | "BrokenPipe"
                | "NotConnected"
                | "UnexpectedEof"
                | "WriteZero"
        )
    ));
}
