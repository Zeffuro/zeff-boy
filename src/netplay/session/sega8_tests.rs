use super::*;
use crate::emu_backend::{ActiveSystem, BackendLoadConfig, load_backend_from_rom_source};
use crate::test_support::{TestDirectory, test_directory};
use std::net::{TcpListener, TcpStream};

fn loaded(system: ActiveSystem, pal: bool) -> (TestDirectory, EmuBackend) {
    let directory = test_directory("netplay-sega8").unwrap();
    let path = directory
        .path()
        .join(if system == ActiveSystem::MasterSystem {
            "game.sms"
        } else {
            "game.sg"
        });
    let mut rom = vec![0; 32768];
    let program = [
        0x3e, 0x84, 0xd3, 0x7f, 0x3e, 0x12, 0xd3, 0x7f, 0x3e, 0x90, 0xd3, 0x7f, 0x3e, 0x08, 0x32,
        0xfc, 0xff, 0xdb, 0xdc, 0x32, 0x00, 0xc0, 0xdb, 0xdd, 0x32, 0x01, 0xc0, 0x3e, 0x5a, 0x00,
        0x3c, 0x32, 0x00, 0x80, 0xc3, 0x11, 0x00,
    ];
    rom[..program.len()].copy_from_slice(&program);
    rom[0x66..0x6f].copy_from_slice(&[0x3a, 0x03, 0xc0, 0x3c, 0x32, 0x03, 0xc0, 0xed, 0x45]);
    rom[0x7ff0..0x7ff8].copy_from_slice(b"TMR SEGA");
    rom[0x7fff] = 0x4c;
    std::fs::write(&path, rom).unwrap();
    let backend = load_backend_from_rom_source(
        system,
        &path,
        &path,
        None,
        BackendLoadConfig {
            sample_rate: Some(48_000),
            sega8_load_battery_sram: false,
            sega8_video_standard: Some(if pal {
                zeff_sega8_core::hardware::timing::Sega8VideoStandard::Pal
            } else {
                zeff_sega8_core::hardware::timing::Sega8VideoStandard::Ntsc
            }),
            ..Default::default()
        },
    )
    .unwrap()
    .backend;
    (directory, backend)
}

fn collect(session: &mut Session, frames: &mut Vec<(Message, [u8; 2], Vec<u32>)>) {
    while let Some(response) = session.responses.pop_front() {
        if let Response::Frame {
            checkpoint,
            ports,
            audio,
        } = response
        {
            frames.push((
                checkpoint,
                ports,
                audio.iter().map(|sample| sample.to_bits()).collect(),
            ));
        }
    }
}

#[test]
fn sega_two_peers_correct_predictions_and_restore_exact_console_baselines() {
    for system in [ActiveSystem::MasterSystem, ActiveSystem::Sg1000] {
        for pal in [false, true] {
            let (_a_dir, mut a) = loaded(system, pal);
            let (_b_dir, mut b) = loaded(system, pal);
            let (_ref_dir, mut reference) = loaded(system, pal);
            let initial = a.encode_state_bytes().unwrap();
            let runtime = a.sega8().unwrap().emu.encode_rollback_runtime_state();
            let persistent = identity::persistent_hash(&a).unwrap();
            let config = identity::identity_with_delay(
                &a,
                [7; 32],
                zeff_netplay::rollback::InputDelay::new(0).unwrap(),
            )
            .unwrap()
            .config;
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let a_stream = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
            let (b_stream, _) = listener.accept().unwrap();
            let start = |stream: TcpStream, player| Start {
                stream: stream.into(),
                player,
                build: [7; 32],
                secret: [5; 32],
                scope: zeff_netplay::endpoint::ConnectionScope::Loopback,
                allow_different_versions: false,
                verify_every_frame: true,
                input_delay: zeff_netplay::rollback::InputDelay::new(0).unwrap(),
            };
            let mut a_session = Session::start(&mut a, start(a_stream, Player::One)).unwrap();
            let mut b_session = Session::start(&mut b, start(b_stream, Player::Two)).unwrap();
            assert!(!a.sega8().unwrap().host_persistence_enabled());
            assert!(a.flush_battery_sram().unwrap().is_none());
            let deadline = Instant::now() + Duration::from_secs(3);
            while !a_session.ready || !b_session.ready {
                a_session.poll(&mut a).unwrap();
                b_session.poll(&mut b).unwrap();
                assert!(Instant::now() < deadline);
                std::thread::yield_now();
            }
            let one = [1, 8, 8, 0, 0x40, 2];
            let two = [0x80, 0, 8, 8, 0, 1];
            let mut a_frames = Vec::new();
            let mut b_frames = Vec::new();
            for buttons in one {
                a_session.step(&mut a, buttons).unwrap();
                collect(&mut a_session, &mut a_frames);
            }
            assert_eq!(a_session.timeline.prediction_depth(), 6);
            for buttons in two {
                b_session.step(&mut b, buttons).unwrap();
                collect(&mut b_session, &mut b_frames);
            }
            let deadline = Instant::now() + Duration::from_secs(3);
            while a_session.committed < 6 || b_session.committed < 6 {
                a_session.batch(&mut a, None).unwrap();
                b_session.batch(&mut b, None).unwrap();
                collect(&mut a_session, &mut a_frames);
                collect(&mut b_session, &mut b_frames);
                assert!(Instant::now() < deadline);
                std::thread::yield_now();
            }
            assert_eq!(a_frames, b_frames);
            assert_eq!(a_frames.len(), 6);
            let lease = Lease::begin(&mut reference).unwrap();
            for (index, (checkpoint, ports, audio)) in a_frames.into_iter().enumerate() {
                assert_eq!(ports, [one[index], two[index]]);
                let expected = lease.advance(&mut reference, ports).unwrap();
                assert!(expected.iter().any(|sample| *sample != 0.0));
                assert_eq!(
                    audio,
                    expected
                        .iter()
                        .map(|sample| sample.to_bits())
                        .collect::<Vec<_>>()
                );
                assert_eq!(
                    checkpoint,
                    identity::checkpoint(&reference, index as u64 + 1, &expected, config).unwrap()
                );
            }
            assert_eq!(
                a.encode_state_bytes().unwrap(),
                reference.encode_state_bytes().unwrap()
            );
            if system == ActiveSystem::MasterSystem {
                assert_ne!(identity::persistent_hash(&a).unwrap(), persistent);
            }
            a_session.restore(&mut a).unwrap();
            b_session.restore(&mut b).unwrap();
            for backend in [&a, &b] {
                assert_eq!(backend.encode_state_bytes().unwrap(), initial);
                assert_eq!(
                    backend.sega8().unwrap().emu.encode_rollback_runtime_state(),
                    runtime
                );
                assert_eq!(identity::persistent_hash(backend).unwrap(), persistent);
                assert!(backend.sega8().unwrap().host_persistence_enabled());
            }
        }
    }
}

#[test]
fn sega_admission_rejects_unknown_loads_changed_runtime_and_foreign_topology() {
    for system in [ActiveSystem::MasterSystem, ActiveSystem::Sg1000] {
        for change in 0..7 {
            let (_directory, mut backend) = loaded(system, false);
            assert!(identity::identity(&backend, [7; 32]).is_ok());
            match change {
                0 => backend.set_input(1, 0),
                1 => backend.set_sample_rate(44_100),
                2 => {}
                3 => {
                    if let EmuBackend::Sega8(sega) = &mut backend {
                        sega.netplay_persistent_load_known = false;
                    }
                }
                4 => backend.debug_suspend(),
                5 => backend.set_apu_channel_mutes(&[true, false, false, false]),
                _ => {
                    if let EmuBackend::Sega8(sega) = &mut backend {
                        sega.emu.bus_mut().set_apu_sample_rate(44_100);
                    }
                }
            }
            if change == 2
                && let EmuBackend::Sega8(sega) = &mut backend
            {
                sega.emu.bus_mut().step_cycles(1);
            }
            assert!(identity::identity(&backend, [7; 32]).is_err());
            assert_eq!(backend.frame_count(), 0);
        }
    }
    let (_sms_dir, sms) = loaded(ActiveSystem::MasterSystem, false);
    let (_sg_dir, sg) = loaded(ActiveSystem::Sg1000, false);
    assert_ne!(
        identity::identity(&sms, [7; 32]).unwrap().config,
        identity::identity(&sg, [7; 32]).unwrap().config
    );
}

#[test]
fn sega_failed_execution_exit_restores_runtime_and_failed_restore_keeps_save_fence() {
    for poison in [false, true] {
        let (_directory, mut backend) = loaded(ActiveSystem::MasterSystem, false);
        let initial = backend.encode_state_bytes().unwrap();
        let runtime = backend.sega8().unwrap().emu.encode_rollback_runtime_state();
        let persistent = identity::persistent_hash(&backend).unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let stream = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let (_peer, _) = listener.accept().unwrap();
        let mut session = Session::start(
            &mut backend,
            Start {
                stream: stream.into(),
                player: Player::One,
                build: [7; 32],
                secret: [5; 32],
                scope: zeff_netplay::endpoint::ConnectionScope::Loopback,
                allow_different_versions: false,
                verify_every_frame: false,
                input_delay: zeff_netplay::rollback::InputDelay::new(0).unwrap(),
            },
        )
        .unwrap();
        session
            .execute(
                &mut backend,
                zeff_netplay::rollback::FrameInput {
                    frame: 0,
                    ports: [8, 0],
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
            assert!(!backend.sega8().unwrap().host_persistence_enabled());
            assert!(backend.flush_battery_sram().unwrap().is_none());
        } else {
            session.restore(&mut backend).unwrap();
            assert_eq!(backend.encode_state_bytes().unwrap(), initial);
            assert_eq!(
                backend.sega8().unwrap().emu.encode_rollback_runtime_state(),
                runtime
            );
            assert_eq!(identity::persistent_hash(&backend).unwrap(), persistent);
            assert!(backend.sega8().unwrap().host_persistence_enabled());
            assert!(!backend.is_suspended());
        }
    }
}
