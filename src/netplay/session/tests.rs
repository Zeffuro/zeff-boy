use super::*;
use zeff_nes_core::hardware::cartridge::TimingMode;

fn checkpoint(frame: u64) -> Message {
    Message::Checkpoint {
        frame,
        logical: [1; 32],
        video: [2; 32],
        audio: [3; 32],
        persistent: [4; 32],
    }
}

#[test]
fn pause_votes_join_the_known_barrier_and_release_their_original_target() {
    let mut pause = PauseControl::default();
    assert_eq!(
        pause.request(20, true).unwrap(),
        Some(Message::PauseChange {
            request: 1,
            frame: 32,
            paused: true
        })
    );
    assert_eq!(pause.status(31, 31), None);
    assert_eq!(pause.status(32, 31), None);
    assert_eq!(pause.status(32, 32), Some((true, false)));
    pause.observe(32, 1, 32, true).unwrap();
    assert_eq!(pause.status(32, 32), Some((true, true)));
    assert_eq!(
        pause.request(32, false).unwrap(),
        Some(Message::PauseChange {
            request: 2,
            frame: 32,
            paused: false
        })
    );
    assert_eq!(pause.status(32, 32), Some((false, true)));
    pause.acknowledge(32, 2).unwrap();
    assert_eq!(
        pause.request(32, true).unwrap(),
        Some(Message::PauseChange {
            request: 3,
            frame: 32,
            paused: true
        })
    );
    pause.request(32, false).unwrap();
    assert!(pause.observe(32, 2, 33, false).is_err());
    pause.observe(32, 2, 32, false).unwrap();
    assert_eq!(pause.barrier(), Some(32));
    assert_eq!(pause.status(32, 32), None);
    pause.acknowledge(32, 4).unwrap();
    assert_eq!(pause.status(32, 32), Some((false, false)));
    assert_eq!(pause.status(32, 32), None);
}

#[test]
fn crossed_request_and_release_cannot_clear_the_earlier_barrier() {
    for current in [110, 112] {
        let mut early = PauseControl::default();
        let mut late = PauseControl::default();
        early.request(100, true).unwrap();
        late.request(110, true).unwrap();
        early.request(100, false).unwrap();
        assert_eq!(early.barrier(), Some(112));
        late.observe(110, 1, 112, true).unwrap();
        late.observe(current, 2, 112, false).unwrap();
        assert_eq!(late.barrier(), Some(112));
        assert_eq!(late.status(112, 112), Some((true, false)));
        early.observe(current, 1, 122, true).unwrap();
        early.acknowledge(current, 1).unwrap();
        early.acknowledge(current, 2).unwrap();
        assert_eq!(early.barrier(), Some(112));
        assert_eq!(early.status(112, 112), Some((false, true)));
        late.request(112, false).unwrap();
        early.observe(112, 2, 122, false).unwrap();
        assert_eq!(early.barrier(), None);
        assert_eq!(early.status(112, 112), Some((false, false)));
        assert_eq!(late.barrier(), Some(112));
        assert_eq!(late.status(112, 112), None);
        late.acknowledge(112, 2).unwrap();
        assert_eq!(late.barrier(), None);
        assert_eq!(late.status(112, 112), Some((false, false)));
    }
}

#[test]
fn stale_acknowledgments_cannot_release_a_newer_vote_episode() {
    let mut pause = PauseControl::default();
    pause.request(10, true).unwrap();
    pause.request(10, false).unwrap();
    pause.acknowledge(10, 2).unwrap();
    pause.request(30, true).unwrap();
    pause.request(30, false).unwrap();
    assert_eq!(pause.barrier(), Some(42));
    for request in [1, 2, 3, 2] {
        pause.acknowledge(30, request).unwrap();
        assert_eq!(pause.barrier(), Some(42));
    }
    assert!(pause.acknowledge(30, 0).is_err());
    assert!(pause.acknowledge(30, 5).is_err());
    pause.acknowledge(30, 4).unwrap();
    assert_eq!(pause.barrier(), None);
    assert_eq!(
        pause.request(30, true).unwrap(),
        Some(Message::PauseChange {
            request: 5,
            frame: 42,
            paused: true
        })
    );
}

#[test]
fn pause_future_bounds_duplicates_order_and_early_cancellation() {
    let mut pause = PauseControl::default();
    assert!(pause.observe(10, 0, 20, true).is_err());
    assert!(pause.observe(10, 2, 20, true).is_err());
    assert!(pause.observe(10, 1, 9, true).is_err());
    assert!(pause.observe(10, 1, 35, true).is_err());
    assert_eq!(
        pause.observe(10, 1, 34, true).unwrap(),
        Message::PauseAck { request: 1 }
    );
    pause.observe(11, 1, 34, true).unwrap();
    assert!(pause.observe(11, 1, 33, true).is_err());
    assert!(pause.observe(11, 2, 34, true).is_err());
    pause.observe(12, 2, 34, false).unwrap();
    assert!(pause.observe(12, 1, 34, true).is_err());
    assert_eq!(pause.status(12, 12), None);
    pause.request(12, true).unwrap();
    pause.request(13, false).unwrap();
    assert_eq!(pause.status(13, 13), None);
    assert_eq!(pause.barrier(), Some(24));
    pause.acknowledge(13, 2).unwrap();
    assert_eq!(pause.barrier(), None);
}

#[test]
fn re_pause_waits_for_release_ack_and_uses_a_fresh_future_target() {
    let mut local = PauseControl::default();
    let mut peer = PauseControl::default();
    local.request(100, true).unwrap();
    peer.observe(100, 1, 112, true).unwrap();
    local.acknowledge(100, 1).unwrap();
    assert_eq!(local.status(112, 112), Some((true, false)));
    assert_eq!(peer.status(112, 112), Some((false, true)));
    local.request(112, false).unwrap();
    peer.observe(112, 2, 112, false).unwrap();
    assert_eq!(peer.barrier(), None);
    assert_eq!(local.request(112, true).unwrap(), None);
    assert_eq!(local.barrier(), Some(112));
    assert_eq!(local.acknowledge(112, 1).unwrap(), None);
    let change = local.acknowledge(112, 2).unwrap();
    assert_eq!(
        change,
        Some(Message::PauseChange {
            request: 3,
            frame: 124,
            paused: true
        })
    );
    assert_eq!(local.barrier(), Some(124));
    assert_eq!(local.status(112, 112), Some((false, false)));
    assert_eq!(local.status(113, 113), None);
    assert_eq!(local.status(124, 123), None);
    assert_eq!(local.status(124, 124), Some((true, false)));
    peer.observe(113, 3, 124, true).unwrap();
    assert_eq!(peer.barrier(), Some(124));
}

#[test]
fn later_false_cancels_a_deferred_re_pause_intent() {
    let mut pause = PauseControl::default();
    pause.request(100, true).unwrap();
    pause.request(112, false).unwrap();
    assert_eq!(pause.request(112, true).unwrap(), None);
    assert_eq!(pause.request(112, false).unwrap(), None);
    assert_eq!(pause.acknowledge(112, 2).unwrap(), None);
    assert_eq!(pause.barrier(), None);
    assert_eq!(
        pause.request(113, true).unwrap(),
        Some(Message::PauseChange {
            request: 3,
            frame: 125,
            paused: true
        })
    );
}

#[test]
fn checkpoints_match_in_either_arrival_order_and_reject_each_changed_domain() {
    for remote_first in [false, true] {
        let mut hashes = Hashes::default();
        if remote_first {
            hashes
                .remote(
                    checkpoint(60),
                    60,
                    zeff_netplay::rollback::InputDelay::default().lookahead(),
                )
                .unwrap();
        }
        hashes.local(checkpoint(60)).unwrap();
        if !remote_first {
            hashes
                .remote(
                    checkpoint(60),
                    60,
                    zeff_netplay::rollback::InputDelay::default().lookahead(),
                )
                .unwrap();
        }
        hashes
            .remote(
                checkpoint(60),
                60,
                zeff_netplay::rollback::InputDelay::default().lookahead(),
            )
            .unwrap();
        for domain in 0..4 {
            let mut hashes = Hashes::default();
            hashes.local(checkpoint(60)).unwrap();
            let mut changed = checkpoint(60);
            let Message::Checkpoint {
                logical,
                video,
                audio,
                persistent,
                ..
            } = &mut changed
            else {
                unreachable!()
            };
            [logical, video, audio, persistent][domain][0] ^= 1;
            assert!(
                hashes
                    .remote(
                        changed,
                        60,
                        zeff_netplay::rollback::InputDelay::default().lookahead()
                    )
                    .is_err()
            );
        }
    }
}

#[test]
fn checkpoint_queues_and_future_frames_are_bounded_without_render_gating() {
    let mut hashes = Hashes::default();
    assert!(
        hashes
            .remote(
                checkpoint(60),
                46,
                zeff_netplay::rollback::InputDelay::default().lookahead()
            )
            .is_err()
    );
    hashes
        .remote(
            checkpoint(60),
            47,
            zeff_netplay::rollback::InputDelay::default().lookahead(),
        )
        .unwrap();
    assert!(
        hashes
            .remote(
                checkpoint(61),
                61,
                zeff_netplay::rollback::InputDelay::default().lookahead()
            )
            .is_err()
    );
    assert!(hashes.local(checkpoint(0)).is_err());
    for frame in (120..=480).step_by(60) {
        hashes
            .remote(
                checkpoint(frame),
                frame,
                zeff_netplay::rollback::InputDelay::default().lookahead(),
            )
            .unwrap();
    }
    assert!(
        hashes
            .remote(
                checkpoint(540),
                540,
                zeff_netplay::rollback::InputDelay::default().lookahead()
            )
            .is_err()
    );
    let mut hashes = Hashes::default();
    for frame in (60..=480).step_by(60) {
        hashes.local(checkpoint(frame)).unwrap();
    }
    assert!(hashes.local(checkpoint(540)).is_err());
}

#[test]
fn production_session_replaces_predicted_audio_before_confirmation_all_timings() {
    for timing in [TimingMode::Ntsc, TimingMode::Pal, TimingMode::Dendy] {
        for frames in
            zeff_netplay::rollback::InputDelay::MIN..=zeff_netplay::rollback::InputDelay::MAX
        {
            replay_case(
                timing,
                zeff_netplay::rollback::InputDelay::new(frames).unwrap(),
            );
        }
    }
}

#[test]
fn selected_delay_bounds_pause_and_checkpoint_lookahead() {
    for frames in zeff_netplay::rollback::InputDelay::MIN..=zeff_netplay::rollback::InputDelay::MAX
    {
        let delay = zeff_netplay::rollback::InputDelay::new(frames).unwrap();
        let mut host = PauseControl::with_delay(delay);
        let mut join = PauseControl::with_delay(delay);
        let Some(Message::PauseChange {
            request,
            frame,
            paused,
        }) = host.request(100, true).unwrap()
        else {
            panic!("pause request missing")
        };
        assert_eq!(frame, 100 + delay.pause_lead());
        let ack = join.observe(100, request, frame, paused).unwrap();
        assert_eq!(ack, Message::PauseAck { request });
        assert_eq!(join.barrier(), Some(frame));
        assert_eq!(join.status(frame - 1, frame - 1), None);
        assert_eq!(join.status(frame, frame - 1), None);
        assert_eq!(join.status(frame, frame), Some((false, true)));
        assert!(
            PauseControl::with_delay(delay)
                .observe(100, 1, 101 + 2 * delay.pause_lead(), true)
                .is_err()
        );
        let mut hashes = Hashes::default();
        assert!(
            hashes
                .remote(checkpoint(60), 59 - delay.lookahead(), delay.lookahead())
                .is_err()
        );
        hashes
            .remote(checkpoint(60), 60 - delay.lookahead(), delay.lookahead())
            .unwrap();
        hashes.local(checkpoint(60)).unwrap();
    }
}

fn replay_case(timing: TimingMode, delay: zeff_netplay::rollback::InputDelay) {
    let count = delay.frames() + PREDICTION_WINDOW;
    use crate::emu_backend::{ActiveSystem, BackendLoadConfig, load_backend_from_rom_source};
    use std::net::{TcpListener, TcpStream};
    let directory = crate::test_support::test_directory("netplay-session-rollback").unwrap();
    let path = directory.path().join("game.nes");
    std::fs::write(&path, super::super::proof::fixture_rom(timing)).unwrap();
    let loaded = || {
        load_backend_from_rom_source(
            ActiveSystem::Nes,
            &path,
            &path,
            None,
            BackendLoadConfig {
                nes_load_battery_sram: false,
                ..BackendLoadConfig::default()
            },
        )
        .unwrap()
        .backend
    };
    let mut backend = loaded();
    let mut reference = loaded();
    let original = backend.encode_state_bytes().unwrap();
    let admission = identity::identity_with_delay(&backend, [7; 32], delay).unwrap();
    let config = admission.config;
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let local = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
    let (remote, _) = listener.accept().unwrap();
    let (control, commands) = crossbeam_channel::bounded::<bool>(1);
    let peer = std::thread::spawn(move || {
        let mut connection =
            zeff_netplay::wire::admit(remote, Player::Two, &admission, &[5; 32]).unwrap();
        loop {
            match commands.recv_timeout(Duration::from_millis(100)) {
                Ok(true) => {
                    for frame in delay.frames()..count {
                        connection
                            .send(&Message::Input {
                                player: Player::Two,
                                frame,
                                buttons: remote_buttons(frame),
                            })
                            .unwrap();
                    }
                }
                Ok(false) | Err(crossbeam_channel::RecvTimeoutError::Disconnected) => break,
                Err(crossbeam_channel::RecvTimeoutError::Timeout) => {
                    if connection
                        .send(&Message::Progress {
                            frame: 0,
                            confirmed: 0,
                        })
                        .is_err()
                    {
                        break;
                    }
                }
            }
        }
    });
    let mut session = Session::start(
        &mut backend,
        Start {
            stream: local.into(),
            player: Player::One,
            build: [7; 32],
            secret: [5; 32],
            scope: zeff_netplay::endpoint::ConnectionScope::Loopback,
            allow_different_versions: false,
            verify_every_frame: true,
            input_delay: delay,
        },
    )
    .unwrap();
    let deadline = Instant::now() + Duration::from_secs(1);
    while !session.ready {
        session.poll(&mut backend).unwrap();
        assert!(Instant::now() < deadline);
        std::thread::yield_now();
    }
    let mut published = Vec::new();
    for frame in 0..count {
        session.step(&mut backend, local_buttons(frame)).unwrap();
        collect_frames(&mut session, &mut published);
    }
    assert_eq!(session.timeline.frame(), count);
    assert_eq!(published.len(), delay.frames() as usize);
    session.step(&mut backend, 0xff).unwrap();
    assert_eq!(session.timeline.frame(), count);
    assert!(session.responses.iter().any(|response| matches!(
        response,
        Response::Presented {
            changed: false,
            step_complete: true,
            ..
        }
    )));
    collect_frames(&mut session, &mut published);
    control.send(true).unwrap();
    let deadline = Instant::now() + Duration::from_secs(1);
    while session.timeline.confirmed_frame() != count {
        session.batch(&mut backend, None).unwrap();
        assert!(Instant::now() < deadline);
        std::thread::yield_now();
    }
    collect_frames(&mut session, &mut published);
    assert_eq!(published.len(), count as usize);
    assert_eq!(session.snapshots.len(), 1);
    assert!(session.outputs.is_empty());
    assert!(
        session.retained_bytes()
            >= session
                .snapshots
                .values()
                .map(Snapshot::retained_bytes)
                .sum()
    );
    for (index, (checkpoint, ports, audio)) in published.into_iter().enumerate() {
        let frame = index as u64;
        let expected_ports = if frame < delay.frames() {
            [0; 2]
        } else {
            [local_buttons(frame - delay.frames()), remote_buttons(frame)]
        };
        assert_eq!(ports, expected_ports);
        let nes = nes_mut(&mut reference).unwrap();
        nes.emu.set_input_p1_raw(ports[0]);
        nes.emu.set_input_p2_raw(ports[1]);
        reference.step_frame();
        let mut expected_audio = Vec::new();
        reference.drain_audio_samples_into(&mut expected_audio);
        assert_eq!(
            audio.iter().map(|x| x.to_bits()).collect::<Vec<_>>(),
            expected_audio
                .iter()
                .map(|x| x.to_bits())
                .collect::<Vec<_>>()
        );
        assert_eq!(
            checkpoint,
            identity::checkpoint(&reference, frame + 1, &expected_audio, config).unwrap()
        );
    }
    assert_eq!(
        backend.encode_state_bytes().unwrap(),
        reference.encode_state_bytes().unwrap()
    );
    control.send(false).unwrap();
    peer.join().unwrap();
    session.restore(&mut backend).unwrap();
    assert_eq!(backend.encode_state_bytes().unwrap(), original);
    assert!(backend.nes().unwrap().host_persistence_enabled());
}

fn collect_frames(session: &mut Session, frames: &mut Vec<(Message, [u8; 2], Vec<f32>)>) {
    while let Some(response) = session.responses.pop_front() {
        if let Response::Frame {
            checkpoint,
            ports,
            audio,
        } = response
        {
            frames.push((checkpoint, ports, audio));
        }
    }
}

fn local_buttons(frame: u64) -> u8 {
    (frame * 17 + 3) as u8
}
fn remote_buttons(frame: u64) -> u8 {
    (frame * 29 + 11) as u8
}
