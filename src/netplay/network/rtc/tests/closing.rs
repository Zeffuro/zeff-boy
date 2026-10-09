use super::*;

#[test]
fn local_stop_notifies_both_roles_and_simultaneous_stop_is_bounded() {
    for first in [Player::One, Player::Two] {
        let (a, b) = peers(64);
        let closed = [a.closed.clone(), b.closed.clone()];
        let a = network(a, Player::One);
        let b = network(b, Player::Two);
        assert!(matches!(event(&a), Event::Ready));
        assert!(matches!(event(&b), Event::Ready));
        let (local, mut remote) = if first == Player::One { (a, b) } else { (b, a) };
        let start = Instant::now();
        local.close(37);
        assert!(start.elapsed() < Duration::from_secs(1));
        assert!(matches!(
            event(&remote),
            Event::Message(Message::Close { frame: 37 })
        ));
        assert!(remote.failure.lock().unwrap().is_none());
        remote.cancel();
        assert!(closed.iter().all(|closed| closed.load(Ordering::Acquire)));
    }
    let (a, b) = peers(64);
    let closed = [a.closed.clone(), b.closed.clone()];
    let a = network(a, Player::One);
    let b = network(b, Player::Two);
    assert!(matches!(event(&a), Event::Ready));
    assert!(matches!(event(&b), Event::Ready));
    let start = Instant::now();
    let a = thread::spawn(move || a.close(7));
    b.close(9);
    a.join().unwrap();
    assert!(start.elapsed() < Duration::from_secs(1));
    assert!(closed.iter().all(|closed| closed.load(Ordering::Acquire)));
}

#[test]
fn local_close_holds_transport_for_inflight_gameplay_and_missing_echo() {
    for late_gameplay in [false, true] {
        let (a, mut peer) = peers(64);
        let closed = a.closed.clone();
        let a = network(a, Player::One);
        let runtime = runtime();
        let mut codec = runtime.block_on(admit_manual(&mut peer));
        assert!(matches!(event(&a), Event::Ready));
        let queued = codec
            .encode(&Message::Progress {
                frame: 0,
                confirmed: 0,
            })
            .unwrap();
        let manual = thread::spawn(move || {
            runtime.block_on(async {
                let packet = peer.receive(Duration::from_secs(1)).await.unwrap();
                assert_eq!(
                    codec.decode(&packet.bytes).unwrap(),
                    Message::Close { frame: 41 }
                );
                if late_gameplay {
                    peer.send_control(&queued).await.unwrap();
                    let echo = codec.encode(&Message::Close { frame: 41 }).unwrap();
                    peer.send_control(&echo).await.unwrap();
                }
                while !closed.load(Ordering::Acquire) {
                    tokio::time::sleep(Duration::from_millis(1)).await;
                }
            });
        });
        let start = Instant::now();
        a.close(41);
        assert!(start.elapsed() >= Duration::from_millis(200));
        assert!(start.elapsed() < Duration::from_secs(1));
        manual.join().unwrap();
    }
}

#[test]
fn authenticated_remote_close_survives_failed_echo() {
    let (a, mut peer) = peers(64);
    let mut a = network(a, Player::One);
    let runtime = runtime();
    let mut codec = runtime.block_on(admit_manual(&mut peer));
    assert!(matches!(event(&a), Event::Ready));
    peer.incoming.close();
    let packet = codec.encode(&Message::Close { frame: 53 }).unwrap();
    runtime.block_on(peer.send_control(&packet)).unwrap();
    assert!(matches!(
        event(&a),
        Event::Message(Message::Close { frame: 53 })
    ));
    a.cancel();
    assert!(a.failure.lock().unwrap().is_none());
}
