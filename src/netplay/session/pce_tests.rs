use super::*;
use crate::emu_backend::{ActiveSystem, BackendLoadConfig, load_backend_from_rom_source};
use crate::test_support::{TestDirectory, test_directory};
use std::net::{TcpListener, TcpStream};

fn loaded(supergrafx: bool) -> (TestDirectory, EmuBackend) {
    let directory = test_directory("netplay-pce").unwrap();
    let path = directory.path().join("game.pce");
    std::fs::write(&path, crate::emu_backend::pce::netplay_fixture_hucard()).unwrap();
    let mut backend = load_backend_from_rom_source(
        ActiveSystem::Pce,
        &path,
        &path,
        None,
        BackendLoadConfig {
            sample_rate: Some(48_000),
            pce_netplay: true,
            pce_load_battery_bram: false,
            pce_cartridge_hardware: Some(if supergrafx {
                zeff_pce_core::hardware::PceCartridgeHardware::SuperGrafx
            } else {
                zeff_pce_core::hardware::PceCartridgeHardware::Base
            }),
            ..Default::default()
        },
    )
    .unwrap()
    .backend;
    let EmuBackend::Pce(pce) = &mut backend else {
        unreachable!()
    };
    pce.set_host_persistence_enabled(true);
    (directory, backend)
}

fn start(stream: TcpStream, player: Player, delay: u64) -> Start {
    Start {
        stream: stream.into(),
        player,
        build: [7; 32],
        secret: [5; 32],
        scope: zeff_netplay::endpoint::ConnectionScope::Loopback,
        allow_different_versions: false,
        verify_every_frame: true,
        input_delay: zeff_netplay::rollback::InputDelay::new(delay).unwrap(),
    }
}

fn frames(session: &mut Session, output: &mut Vec<(Message, [u8; 2], Vec<u32>)>) {
    while let Some(response) = session.responses.pop_front() {
        if let Response::Frame {
            checkpoint,
            ports,
            audio,
        } = response
        {
            output.push((
                checkpoint,
                ports,
                audio.iter().map(|s| s.to_bits()).collect(),
            ));
        }
    }
}

#[test]
fn pce_peers_correct_late_inputs_and_restore_exact_baselines() {
    for supergrafx in [false, true] {
        for delay in [0, 2, 3] {
            let (_a_dir, mut a) = loaded(supergrafx);
            let (_b_dir, mut b) = loaded(supergrafx);
            let (_ref_dir, mut reference) = loaded(supergrafx);
            let initial = a.encode_state_bytes().unwrap();
            let runtime = a.pce().unwrap().netplay_runtime_state_bytes();
            let persistent = identity::persistent_hash(&a).unwrap();
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let a_stream = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
            let (b_stream, _) = listener.accept().unwrap();
            let mut one = Session::start(&mut a, start(a_stream, Player::One, delay)).unwrap();
            let mut two = Session::start(&mut b, start(b_stream, Player::Two, delay)).unwrap();
            let config = one.config;
            assert!(!a.pce().unwrap().host_persistence_enabled());
            assert!(a.flush_battery_sram().unwrap().is_none());
            let deadline = Instant::now() + Duration::from_secs(3);
            while !one.ready || !two.ready {
                one.poll(&mut a).unwrap();
                two.poll(&mut b).unwrap();
                assert!(Instant::now() < deadline);
                std::thread::yield_now();
            }
            let local = [[1, 8, 4, 0x40, 2, 0], [0x80, 0, 1, 2, 4, 8]];
            let mut outputs = [Vec::new(), Vec::new()];
            for input in local[0] {
                one.step(&mut a, input).unwrap();
                frames(&mut one, &mut outputs[0]);
            }
            assert!(one.timeline.prediction_depth() >= 3);
            for input in local[1] {
                two.step(&mut b, input).unwrap();
                frames(&mut two, &mut outputs[1]);
            }
            let deadline = Instant::now() + Duration::from_secs(3);
            while one.committed < 6 || two.committed < 6 {
                one.batch(&mut a, None).unwrap();
                two.batch(&mut b, None).unwrap();
                frames(&mut one, &mut outputs[0]);
                frames(&mut two, &mut outputs[1]);
                assert!(Instant::now() < deadline);
                std::thread::yield_now();
            }
            assert_eq!(outputs[0], outputs[1]);
            assert_eq!(outputs[0].len(), 6);
            let lease = Lease::begin(&mut reference).unwrap();
            for (frame, (checkpoint, ports, audio)) in outputs[0].iter().enumerate() {
                let expected_ports = if frame < delay as usize {
                    [0, 0]
                } else {
                    local.map(|values| values[frame - delay as usize])
                };
                assert_eq!(*ports, expected_ports);
                let expected = lease.advance(&mut reference, expected_ports).unwrap();
                assert_eq!(
                    *audio,
                    expected.iter().map(|s| s.to_bits()).collect::<Vec<_>>()
                );
                assert_eq!(
                    *checkpoint,
                    identity::checkpoint(&reference, frame as u64 + 1, &expected, config).unwrap()
                );
            }
            assert!(
                reference
                    .framebuffer()
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .any(|p| p[..3].iter().any(|b| *b != 0))
            );
            assert!(
                outputs[0]
                    .iter()
                    .any(|(_, _, pcm)| pcm.iter().any(|bits| f32::from_bits(*bits) != 0.0))
            );
            one.restore(&mut a).unwrap();
            two.restore(&mut b).unwrap();
            for backend in [&a, &b] {
                assert_eq!(backend.encode_state_bytes().unwrap(), initial);
                assert_eq!(
                    backend.pce().unwrap().netplay_runtime_state_bytes(),
                    runtime
                );
                assert_eq!(identity::persistent_hash(backend).unwrap(), persistent);
                assert!(backend.pce().unwrap().host_persistence_enabled());
            }
        }
    }
}

#[test]
fn pce_admission_and_failed_restore_keep_persistence_fenced() {
    for (poison, publication) in [(false, true), (false, false), (true, true)] {
        let (_dir, mut backend) = loaded(false);
        let EmuBackend::Pce(pce) = &mut backend else {
            unreachable!()
        };
        pce.set_host_persistence_enabled(publication);
        let initial = backend.encode_state_bytes().unwrap();
        backend.set_input(1, 0);
        assert!(identity::identity(&backend, [7; 32]).is_err());
        backend.set_input(0, 0);
        assert!(identity::identity(&backend, [7; 32]).is_ok());
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let stream = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let (_peer, _) = listener.accept().unwrap();
        let mut session = Session::start(&mut backend, start(stream, Player::One, 0)).unwrap();
        session
            .execute(
                &mut backend,
                zeff_netplay::rollback::FrameInput {
                    frame: 0,
                    ports: [1, 2],
                },
            )
            .unwrap();
        backend.debug_suspend();
        assert!(
            session
                .execute(
                    &mut backend,
                    zeff_netplay::rollback::FrameInput {
                        frame: 1,
                        ports: [0, 0]
                    }
                )
                .is_err()
        );
        if poison {
            session.corrupt_restore_checkpoint_for_test();
            assert!(session.restore(&mut backend).is_err());
            assert!(!backend.pce().unwrap().host_persistence_enabled());
            assert!(backend.flush_battery_sram().unwrap().is_none());
        } else {
            session.restore(&mut backend).unwrap();
            assert_eq!(backend.encode_state_bytes().unwrap(), initial);
            assert_eq!(
                backend.pce().unwrap().host_persistence_enabled(),
                publication
            );
            assert!(!backend.is_suspended());
        }
    }
}
