use super::*;
use crate::netplay::md_fixture::{self as fixture, Baseline};
use zeff_md_core::{ControllerKind, Region, TimingModel};

pub(super) fn loaded() -> (Option<crate::test_support::TestDirectory>, EmuBackend) {
    let config = fixture::config(
        Region::NtscUs,
        TimingModel::Standard,
        ControllerKind::ThreeButton,
    );
    #[cfg(not(target_arch = "wasm32"))]
    {
        let (directory, backend) = fixture::loaded(config);
        (Some(directory), backend)
    }
    #[cfg(target_arch = "wasm32")]
    {
        (None, fixture::loaded(config))
    }
}

pub(super) fn transports() -> (crate::netplay::Transport, crate::netplay::Transport) {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let a = std::net::TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let (b, _) = listener.accept().unwrap();
        (a.into(), b.into())
    }
    #[cfg(target_arch = "wasm32")]
    {
        let (a, b, _) = crate::netplay::network::paired::pair();
        (
            crate::netplay::Transport::Paired(a),
            crate::netplay::Transport::Paired(b),
        )
    }
}

pub(super) fn start(stream: crate::netplay::Transport, player: Player) -> Start {
    Start {
        stream,
        player,
        build: crate::netplay::connect::executable_build().unwrap(),
        secret: [5; 32],
        scope: zeff_netplay::endpoint::ConnectionScope::Loopback,
        allow_different_versions: false,
        verify_every_frame: true,
        input_delay: zeff_netplay::rollback::InputDelay::new(0).unwrap(),
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn md_every_post_acquisition_start_failure_restores_original_pointer_runtime_and_publication() {
    use md_start_failure::{Failure, Stage};
    for player in [Player::One, Player::Two] {
        for stage in [
            Stage::FirstCapture,
            Stage::SecondCapture,
            Stage::FrameZero,
            Stage::Network,
            Stage::AfterNetwork,
        ] {
            let (_directory, mut backend) = loaded();
            let baseline = Baseline::capture(&backend);
            let (stream, _peer) = transports();
            md_start_failure::next(stage);
            let error = match Session::start(&mut backend, start(stream, player)) {
                Ok(_) => panic!("injected start failure was skipped"),
                Err(error) => error,
            };
            assert_eq!(
                error.downcast_ref::<Failure>().map(|failure| failure.0),
                Some(stage)
            );
            baseline.verify(&backend);
        }
    }
    for player in [Player::One, Player::Two] {
        let (_directory, mut backend) = loaded();
        let baseline = Baseline::capture(&backend);
        let EmuBackend::Md(md) = &mut backend else {
            unreachable!()
        };
        md.arm_start_failure_for_test(Stage::AbortVerification)
            .unwrap();
        let (stream, _peer) = transports();
        let error = match Session::start(&mut backend, start(stream, player)) {
            Ok(_) => panic!("abort verification failure was skipped"),
            Err(error) => error,
        };
        assert_eq!(
            error.downcast_ref::<Failure>().map(|failure| failure.0),
            Some(Stage::AbortVerification)
        );
        baseline.verify_restoring(&mut backend);
    }
}

pub(super) fn ready(a: &mut Session, b: &mut Session, one: &mut EmuBackend, two: &mut EmuBackend) {
    let deadline = Instant::now() + Duration::from_secs(3);
    while !a.ready || !b.ready {
        a.poll(one).unwrap();
        b.poll(two).unwrap();
        assert!(Instant::now() < deadline);
        #[cfg(not(target_arch = "wasm32"))]
        std::thread::yield_now();
    }
    a.responses.clear();
    b.responses.clear();
}

fn frontier(session: &Session) -> (u64, u64, u64, Option<u64>, Vec<u64>, Vec<u64>) {
    (
        session.timeline.frame(),
        session.timeline.confirmed_frame(),
        session.timeline.prediction_depth(),
        session.sampled,
        session.snapshots.keys().copied().collect(),
        session.outputs.keys().copied().collect(),
    )
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn md_direct_and_authenticated_remote_words_fail_before_timeline_or_core_mutation() {
    for local in [Player::One, Player::Two] {
        let remote = if local == Player::One {
            Player::Two
        } else {
            Player::One
        };
        for invalid in [0x04, 0x10, 0x20, 0x80, 0x1000, 0x8000] {
            let (_a_dir, mut one) = loaded();
            let (_b_dir, mut two) = loaded();
            let baseline_one = Baseline::capture(&one);
            let baseline_two = Baseline::capture(&two);
            let (a_stream, b_stream) = transports();
            let mut a = Session::start(&mut one, start(a_stream, local)).unwrap();
            let mut b = Session::start(&mut two, start(b_stream, remote)).unwrap();
            ready(&mut a, &mut b, &mut one, &mut two);
            let before = frontier(&a);
            let core = fixture::md(&one).core().clone();
            assert!(a.step(&mut one, invalid).is_err());
            assert_eq!(frontier(&a), before);
            assert!(fixture::md(&one).core() == &core);
            let wrong_role = a
                .handle_event(Event::Message(Message::Input {
                    player: local,
                    frame: 0,
                    buttons: 0x0101,
                }))
                .unwrap_err();
            assert!(wrong_role.to_string().contains("local port"));
            assert_eq!(frontier(&a), before);
            b.network
                .send(Message::Input {
                    player: remote,
                    frame: 0,
                    buttons: invalid,
                })
                .unwrap();
            let deadline = Instant::now() + Duration::from_secs(3);
            let error = loop {
                if let Err(error) = a.poll(&mut one) {
                    break error;
                }
                assert!(Instant::now() < deadline);
                #[cfg(not(target_arch = "wasm32"))]
                std::thread::yield_now();
            };
            assert!(error.to_string().contains("unsupported input bits"));
            assert_eq!(frontier(&a), before);
            assert!(fixture::md(&one).core() == &core);
            a.restore(&mut one).unwrap();
            b.restore(&mut two).unwrap();
            baseline_one.verify(&one);
            baseline_two.verify(&two);
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn md_failed_original_verification_and_worker_verification_keep_persistence_fenced() {
    for corrupt_worker in [false, true] {
        let (_directory, mut backend) = loaded();
        let original = fixture::md(&backend).core().clone();
        let pointer = fixture::md(&backend).core() as *const _ as usize;
        let baseline = Baseline::capture(&backend);
        let (_peer_directory, mut peer_backend) = loaded();
        let peer_baseline = Baseline::capture(&peer_backend);
        let (stream, peer_stream) = transports();
        let mut session = Session::start(&mut backend, start(stream, Player::One)).unwrap();
        let mut peer = Session::start(&mut peer_backend, start(peer_stream, Player::Two)).unwrap();
        ready(&mut session, &mut peer, &mut backend, &mut peer_backend);
        session.step(&mut backend, 0x0101).unwrap();
        let deadline = Instant::now() + Duration::from_secs(90);
        loop {
            let completed = matches!(
                session.poll(&mut backend).unwrap(),
                Some(Response::Presented {
                    frame: 1,
                    step_complete: true,
                    ..
                })
            );
            peer.poll(&mut peer_backend).unwrap();
            if completed {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "MD verification field completion deadline"
            );
        }
        assert_eq!(backend.frame_count(), 1);
        assert!(fixture::md(&backend).core() != &original);
        let EmuBackend::Md(md) = &mut backend else {
            unreachable!()
        };
        assert!(matches!(
            md.arm_start_failure_for_test(md_start_failure::Stage::AbortVerification),
            Err(crate::emu_backend::md::MdOwnershipError::RollbackOwned)
        ));
        if !corrupt_worker {
            session.corrupt_restore_checkpoint_for_test();
        }
        let result = session.restore_with_worker(
            &mut backend,
            if corrupt_worker {
                Err(anyhow::anyhow!("worker schedule changed"))
            } else {
                Ok(())
            },
        );
        assert!(result.is_err());
        assert!(fixture::md(&backend).rollback_owned());
        assert!(!fixture::md(&backend).host_persistence_enabled());
        assert!(backend.flush_battery_sram().is_err());
        assert_eq!(fixture::md(&backend).core() as *const _ as usize, pointer);
        assert!(fixture::md(&backend).core() == &original);
        baseline.verify_restoring(&mut backend);
        peer.restore(&mut peer_backend).unwrap();
        peer_baseline.verify(&peer_backend);
        let before = fixture::md(&backend).core().clone();
        backend.set_input(0xff, 0x0f);
        backend.set_input_p2(0xff, 0x0f);
        backend.set_sample_rate(22_050);
        backend.step_frame();
        assert!(fixture::md(&backend).core() == &before);
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn md_real_close_and_production_stall_limit_restore_the_original_owner() {
    let (_a_dir, mut one) = loaded();
    let (_b_dir, mut two) = loaded();
    let baseline_one = Baseline::capture(&one);
    let baseline_two = Baseline::capture(&two);
    let (a_stream, b_stream) = transports();
    let mut a = Session::start(&mut one, start(a_stream, Player::One)).unwrap();
    let mut b = Session::start(&mut two, start(b_stream, Player::Two)).unwrap();
    ready(&mut a, &mut b, &mut one, &mut two);
    let frontier_one = frontier(&a);
    let frontier_two = frontier(&b);
    let stalled_since = Instant::now();
    a.stalled_since = Some(stalled_since);
    while stalled_since.elapsed() < STALL_LIMIT {
        for session in [&mut a, &mut b] {
            session.send_progress(false).unwrap();
            while let Some(event) = session.network.poll().unwrap() {
                session.handle_event(event).unwrap();
            }
        }
        #[cfg(not(target_arch = "wasm32"))]
        std::thread::yield_now();
        #[cfg(target_arch = "wasm32")]
        std::hint::spin_loop();
    }
    assert_eq!(a.stalled_since, Some(stalled_since));
    assert_eq!(frontier(&a), frontier_one);
    assert_eq!(frontier(&b), frontier_two);
    assert!(
        a.poll(&mut one)
            .err()
            .expect("prediction must hit the production stall limit")
            .to_string()
            .contains("prediction stalled")
    );
    a.restore(&mut one).unwrap();
    baseline_one.verify(&one);
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        if let Err(error) = b.poll(&mut two) {
            assert!(
                error.to_string().contains("close") || error.to_string().contains("Close"),
                "{error:#}"
            );
            break;
        }
        assert!(Instant::now() < deadline);
        #[cfg(not(target_arch = "wasm32"))]
        std::thread::yield_now();
    }
    b.restore(&mut two).unwrap();
    baseline_two.verify(&two);
}
