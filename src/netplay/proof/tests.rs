use super::*;
use zeff_netplay::wire;

#[test]
fn real_workers_match_delayed_reference_and_restore_without_publishing_session_sram() {
    let directory = crate::test_support::test_directory("netplay-worker-proof").unwrap();
    let report = run(directory.path(), 24, [9; 32]).unwrap();
    assert_eq!(report["frames"], 24);
    assert_eq!(report["save_protection"], true);
}

#[test]
fn worker_cancels_pending_admission_and_pending_input_promptly() {
    for admitted in [false, true] {
        let directory = crate::test_support::test_directory("netplay-worker-cancel").unwrap();
        let (_, backend) = loaded(directory.path(), "worker").unwrap();
        let initial = backend.encode_state_bytes().unwrap();
        let admission = identity::identity(&backend, [9; 32]).unwrap();
        let (local, remote) = sockets().unwrap();
        let mut worker = EmuThread::spawn(backend, false);
        worker.send(EmuCommand::StartNetplay(Box::new(Start {
            allow_different_versions: false,
            verify_every_frame: true,
            input_delay: zeff_netplay::rollback::InputDelay::default(),
            scope: zeff_netplay::endpoint::ConnectionScope::Loopback,
            stream: local,
            player: Player::One,
            build: [9; 32],
            secret: [5; 32],
        })));
        let _peer = if admitted {
            let peer = wire::admit(remote, Player::Two, &admission, &[5; 32]).unwrap();
            assert!(matches!(next(&worker).unwrap(), Response::Ready));
            worker.send(EmuCommand::StepNetplay(3));
            Some(peer)
        } else {
            None
        };
        let started = Instant::now();
        worker.send(EmuCommand::StopNetplay);
        stopped(&worker).unwrap();
        assert!(started.elapsed() < Duration::from_millis(750));
        assert_eq!(capture(&worker).unwrap(), initial);
        worker.shutdown();
    }
}

#[test]
fn idle_disconnect_restores_without_another_step() {
    let directory = crate::test_support::test_directory("netplay-worker-idle").unwrap();
    let (_, backend) = loaded(directory.path(), "worker").unwrap();
    let admission = identity::identity(&backend, [9; 32]).unwrap();
    let (local, remote) = sockets().unwrap();
    let mut worker = EmuThread::spawn(backend, false);
    worker.send(EmuCommand::StartNetplay(Box::new(Start {
        allow_different_versions: false,
        verify_every_frame: true,
        input_delay: zeff_netplay::rollback::InputDelay::default(),
        scope: zeff_netplay::endpoint::ConnectionScope::Loopback,
        stream: local,
        player: Player::One,
        build: [9; 32],
        secret: [5; 32],
    })));
    let peer = wire::admit(remote, Player::Two, &admission, &[5; 32]).unwrap();
    assert!(matches!(next(&worker).unwrap(), Response::Ready));
    drop(peer);
    let reason = stopped(&worker).unwrap();
    assert!(
        reason.contains("closed") || reason.contains("failed") || reason.contains("deadline"),
        "{reason}"
    );
    worker.shutdown();
}

#[test]
fn admission_mismatch_restores_before_any_frame_or_publication() {
    for wrong_secret in [true, false] {
        let directory = crate::test_support::test_directory("netplay-worker-admission").unwrap();
        let (path, backend) = loaded(directory.path(), "worker").unwrap();
        let initial = backend.encode_state_bytes().unwrap();
        let mut admission = identity::identity(&backend, [9; 32]).unwrap();
        if !wrong_secret {
            admission.source[0] ^= 1;
        }
        let (local, remote) = sockets().unwrap();
        let mut worker = EmuThread::spawn(backend, false);
        worker.send(EmuCommand::StartNetplay(Box::new(Start {
            allow_different_versions: false,
            verify_every_frame: true,
            input_delay: zeff_netplay::rollback::InputDelay::default(),
            scope: zeff_netplay::endpoint::ConnectionScope::Loopback,
            stream: local,
            player: Player::One,
            build: [9; 32],
            secret: [5; 32],
        })));
        assert!(
            wire::admit(
                remote,
                Player::Two,
                &admission,
                if wrong_secret { &[6; 32] } else { &[5; 32] }
            )
            .is_err()
        );
        let reason = stopped(&worker).unwrap();
        assert!(
            reason.contains(if wrong_secret {
                "authentication"
            } else {
                "identity"
            }),
            "{reason}"
        );
        assert_eq!(capture(&worker).unwrap(), initial);
        assert!(!path.with_extension("sav").exists());
        worker.shutdown();
    }
}

#[test]
fn shutdown_cancels_pending_admission_without_waiting_for_peer() {
    let directory = crate::test_support::test_directory("netplay-worker-shutdown").unwrap();
    let (path, backend) = loaded(directory.path(), "worker").unwrap();
    let persistent = identity::identity(&backend, [9; 32]).unwrap().persistent;
    let (local, _silent_peer) = sockets().unwrap();
    let mut worker = EmuThread::spawn(backend, false);
    worker.send(EmuCommand::StartNetplay(Box::new(Start {
        allow_different_versions: false,
        verify_every_frame: true,
        input_delay: zeff_netplay::rollback::InputDelay::default(),
        scope: zeff_netplay::endpoint::ConnectionScope::Loopback,
        stream: local,
        player: Player::One,
        build: [9; 32],
        secret: [5; 32],
    })));
    let started = Instant::now();
    worker.shutdown();
    assert!(started.elapsed() < Duration::from_millis(750));
    assert_eq!(
        <[u8; 32]>::from(Sha256::digest(
            std::fs::read(path.with_extension("sav")).unwrap()
        )),
        persistent
    );
}
