use super::*;
use crate::emu_backend::{ActiveSystem, BackendLoadConfig, load_backend_from_rom_source};
use crate::emu_core_trait::EmulatorCore;
use crate::test_support::{TestDirectory, test_directory};
use std::net::{TcpListener, TcpStream};

pub(super) fn loaded(color: bool) -> (TestDirectory, EmuBackend) {
    let directory = test_directory("netplay-ws").unwrap();
    let path = directory
        .path()
        .join(if color { "game.wsc" } else { "game.ws" });
    std::fs::write(&path, crate::emu_backend::ws::netplay_fixture_rom(color)).unwrap();
    let backend = load_backend_from_rom_source(
        ActiveSystem::WonderSwan,
        &path,
        &path,
        None,
        BackendLoadConfig {
            sample_rate: Some(48_000),
            ws_load_battery_sram: false,
            ..Default::default()
        },
    )
    .unwrap()
    .backend;
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

type Output = (Message, [u16; 2], Vec<u32>);
fn frames(session: &mut Session, output: &mut Vec<Output>) {
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
fn linked_peers_correct_ws11_inputs_and_check_both_machines() {
    for color in [false, true] {
        for delay in [0, 2, 3] {
            let (_ad, mut a) = loaded(color);
            let (_bd, mut b) = loaded(color);
            let (_rd0, mut reference0) = loaded(color);
            let (_rd1, mut reference1) = loaded(color);
            let initial = a.encode_state_bytes().unwrap();
            let persistent = identity::persistent_hash(&a).unwrap();
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let a_stream = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
            let (b_stream, _) = listener.accept().unwrap();
            let mut one = Session::start(&mut a, start(a_stream, Player::One, delay)).unwrap();
            let mut two = Session::start(&mut b, start(b_stream, Player::Two, delay)).unwrap();
            let EmuBackend::Ws(ws) = &mut a else {
                unreachable!()
            };
            assert!(!ws.host_persistence_enabled());
            assert!(ws.flush_battery_sram().unwrap().is_none());
            let deadline = Instant::now() + Duration::from_secs(3);
            while !one.ready || !two.ready {
                one.poll(&mut a).unwrap();
                two.poll(&mut b).unwrap();
                assert!(Instant::now() < deadline);
                std::thread::yield_now();
            }
            let local = [
                [0x0715, 0x042a, 0x0100, 0x0200, 0x0400, 0],
                [0x02a9, 0x0105, 0x0401, 0x0202, 0x0110, 0x0040],
            ];
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
            let mut leases = [
                Lease::begin(&mut reference0, Player::One).unwrap(),
                Lease::begin(&mut reference1, Player::Two).unwrap(),
            ];
            for frame in 0..6 {
                let ports = if frame < delay as usize {
                    [0, 0]
                } else {
                    local.map(|values| values[frame - delay as usize])
                };
                assert_eq!(outputs[0][frame].0, outputs[1][frame].0);
                for (endpoint, backend) in
                    [&mut reference0, &mut reference1].into_iter().enumerate()
                {
                    let audio = leases[endpoint].advance(backend, ports).unwrap();
                    let snapshot = leases[endpoint].capture(backend).unwrap();
                    assert_eq!(outputs[endpoint][frame].1, ports);
                    assert_eq!(
                        outputs[endpoint][frame].2,
                        audio.iter().map(|s| s.to_bits()).collect::<Vec<_>>()
                    );
                    assert_eq!(
                        outputs[endpoint][frame].0,
                        identity::checkpoint_with_snapshot(
                            backend,
                            frame as u64 + 1,
                            &audio,
                            one.config,
                            None,
                            snapshot.ws()
                        )
                        .unwrap()
                    );
                }
            }
            let mut distinct_video = false;
            for frame in 6..66 {
                one.step(&mut a, local[0][frame % 6]).unwrap();
                two.step(&mut b, local[1][frame % 6]).unwrap();
                let deadline = Instant::now() + Duration::from_secs(3);
                while one.committed < frame as u64 + 1 || two.committed < frame as u64 + 1 {
                    one.batch(&mut a, None).unwrap();
                    two.batch(&mut b, None).unwrap();
                    frames(&mut one, &mut outputs[0]);
                    frames(&mut two, &mut outputs[1]);
                    assert!(Instant::now() < deadline);
                    std::thread::yield_now();
                }
                frames(&mut one, &mut outputs[0]);
                frames(&mut two, &mut outputs[1]);
                distinct_video |= a.framebuffer() != b.framebuffer();
                let ports = local.map(|values| values[(frame - delay as usize) % 6]);
                assert_eq!(outputs[0][frame].0, outputs[1][frame].0);
                for (endpoint, backend) in
                    [&mut reference0, &mut reference1].into_iter().enumerate()
                {
                    let audio = leases[endpoint].advance(backend, ports).unwrap();
                    let snapshot = leases[endpoint].capture(backend).unwrap();
                    assert_eq!(outputs[endpoint][frame].1, ports);
                    assert_eq!(
                        outputs[endpoint][frame].2,
                        audio.iter().map(|s| s.to_bits()).collect::<Vec<_>>()
                    );
                    assert_eq!(
                        outputs[endpoint][frame].0,
                        identity::checkpoint_with_snapshot(
                            backend,
                            frame as u64 + 1,
                            &audio,
                            one.config,
                            None,
                            snapshot.ws()
                        )
                        .unwrap()
                    );
                }
            }
            let deadline = Instant::now() + Duration::from_secs(3);
            while one.hashes.matched_frame() != Some(60) || two.hashes.matched_frame() != Some(60) {
                one.batch(&mut a, None).unwrap();
                two.batch(&mut b, None).unwrap();
                assert!(Instant::now() < deadline);
                std::thread::yield_now();
            }
            assert!(distinct_video);
            assert_eq!(
                a.encode_state_bytes().unwrap(),
                reference0.encode_state_bytes().unwrap()
            );
            assert_eq!(
                b.encode_state_bytes().unwrap(),
                reference1.encode_state_bytes().unwrap()
            );
            assert_ne!(
                a.encode_state_bytes().unwrap(),
                b.encode_state_bytes().unwrap()
            );
            assert!(outputs.iter().any(|frames| {
                frames
                    .iter()
                    .any(|(_, _, pcm)| pcm.iter().any(|bits| f32::from_bits(*bits) != 0.0))
            }));
            one.restore(&mut a).unwrap();
            two.restore(&mut b).unwrap();
            for backend in [&a, &b] {
                assert_eq!(backend.encode_state_bytes().unwrap(), initial);
                assert_eq!(identity::persistent_hash(backend).unwrap(), persistent);
                let EmuBackend::Ws(ws) = backend else {
                    unreachable!()
                };
                assert!(ws.host_persistence_enabled());
            }
        }
    }
}

#[test]
fn ws_admission_rejects_changes_after_loading() {
    for mutation in 0..4 {
        let (_directory, mut backend) = loaded(false);
        assert!(identity::identity(&backend, [7; 32]).is_ok());
        let EmuBackend::Ws(ws) = &mut backend else {
            unreachable!()
        };
        match mutation {
            0 => ws.emu.cpu_write8(0x200, 0xaa),
            1 => ws.emu.set_sample_rate(44_100),
            2 => ws.emu.io_write8(0xb3, 0x80),
            3 => ws.emu.io_write8(0x80, 0x40),
            _ => unreachable!(),
        }
        assert!(identity::identity(&backend, [7; 32]).is_err());
    }
}
