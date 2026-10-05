use super::*;
use crate::netplay::session::Session;

#[derive(Default)]
struct Observation {
    confirmed: u64,
    paused: (bool, bool),
    frames: Vec<(Message, [u16; 2], Vec<f32>)>,
}

fn cycle(
    sessions: &mut [Session; 2],
    backends: [&mut EmuBackend; 2],
    observed: &mut [Observation; 2],
    buttons: [u16; 2],
) {
    for index in 0..2 {
        sessions[index]
            .step(backends[index], buttons[index])
            .unwrap();
    }
    let mut complete = [false; 2];
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        for index in 0..2 {
            while let Some(response) = sessions[index].poll(backends[index]).unwrap() {
                match response {
                    Response::Presented {
                        step_complete: true,
                        ..
                    } => complete[index] = true,
                    Response::Presented { .. } | Response::NetworkStats(_) => {}
                    Response::Paused { local, peer, .. } => observed[index].paused = (local, peer),
                    Response::Frame {
                        checkpoint,
                        ports,
                        audio,
                    } => {
                        let Message::Checkpoint { frame, .. } = checkpoint else {
                            unreachable!()
                        };
                        assert_eq!(frame, observed[index].confirmed + 1);
                        observed[index].confirmed = frame;
                        observed[index].frames.push((checkpoint, ports, audio));
                    }
                    _ => panic!("unexpected session output"),
                }
            }
        }
        if complete.iter().all(|complete| *complete)
            && observed
                .iter()
                .zip(backends.iter())
                .all(|(observation, backend)| observation.confirmed == backend.frame_count())
        {
            return;
        }
        assert!(Instant::now() < deadline, "pause cycle timed out");
        std::thread::sleep(Duration::from_millis(1));
    }
}

#[test]
fn pause_keeps_native_bytes_and_delayed_inputs_exact_until_both_release() {
    pause_exact(zeff_nes_core::hardware::cartridge::TimingMode::Ntsc);
}
#[test]
fn pal_pause_keeps_native_bytes_and_delayed_inputs_exact_until_both_release() {
    pause_exact(zeff_nes_core::hardware::cartridge::TimingMode::Pal);
}
#[test]
fn dendy_pause_keeps_native_bytes_and_delayed_inputs_exact_until_both_release() {
    pause_exact(zeff_nes_core::hardware::cartridge::TimingMode::Dendy);
}

fn pause_exact(timing: zeff_nes_core::hardware::cartridge::TimingMode) {
    let directory = crate::test_support::test_directory("netplay-pause-native").unwrap();
    let media = media::Media::fixture_timing(timing);
    let (_, mut one) = media.load(directory.path(), "one").unwrap();
    let (_, mut two) = media.load(directory.path(), "two").unwrap();
    let (_, mut reference) = media.load(directory.path(), "reference").unwrap();
    let initial = [
        one.encode_state_bytes().unwrap(),
        two.encode_state_bytes().unwrap(),
    ];
    let config = identity::identity(&reference, [9; 32]).unwrap().config;
    let (stream_one, stream_two) = sockets().unwrap();
    let mut sessions = [
        Session::start(
            &mut one,
            Start {
                allow_different_versions: false,
                verify_every_frame: true,
                input_delay: zeff_netplay::rollback::InputDelay::default(),
                scope: zeff_netplay::endpoint::ConnectionScope::Loopback,
                stream: stream_one.into(),
                player: Player::One,
                build: [9; 32],
                secret: [5; 32],
            },
        )
        .unwrap(),
        Session::start(
            &mut two,
            Start {
                allow_different_versions: false,
                verify_every_frame: true,
                input_delay: zeff_netplay::rollback::InputDelay::default(),
                scope: zeff_netplay::endpoint::ConnectionScope::Loopback,
                stream: stream_two.into(),
                player: Player::Two,
                build: [9; 32],
                secret: [5; 32],
            },
        )
        .unwrap(),
    ];
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut ready = [false; 2];
    while !ready.iter().all(|v| *v) {
        for (index, backend) in [&mut one, &mut two].into_iter().enumerate() {
            if let Some(response) = sessions[index].poll(backend).unwrap() {
                assert!(matches!(response, Response::Ready));
                ready[index] = true;
            }
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
    let mut observed = [Observation::default(), Observation::default()];
    for frame in 0..20 {
        if frame == 8 {
            sessions[0].request_pause(true).unwrap();
        }
        cycle(
            &mut sessions,
            [&mut one, &mut two],
            &mut observed,
            [sample(frame, Player::One), sample(frame, Player::Two)],
        );
        let ports = if frame < 2 {
            [0, 0]
        } else {
            [
                sample(frame - 2, Player::One),
                sample(frame - 2, Player::Two),
            ]
        };
        let (expected, audio) = reference_frame(&mut reference, ports, config).unwrap();
        for observation in &observed {
            let (actual, used, pcm) = observation.frames.last().unwrap();
            assert_eq!(actual, &expected);
            assert_eq!(*used, ports);
            assert!(
                pcm.iter()
                    .map(|v| v.to_bits())
                    .eq(audio.iter().map(|v| v.to_bits()))
            );
        }
    }
    assert_eq!(observed[0].paused, (true, false));
    assert_eq!(observed[1].paused, (false, true));
    let paused = [
        one.encode_state_bytes().unwrap(),
        two.encode_state_bytes().unwrap(),
    ];
    sessions[1].request_pause(true).unwrap();
    cycle(&mut sessions, [&mut one, &mut two], &mut observed, [255; 2]);
    sessions[0].request_pause(false).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while observed[0].paused != (false, true) || observed[1].paused != (true, false) {
        for (index, backend) in [&mut one, &mut two].into_iter().enumerate() {
            while let Some(response) = sessions[index].poll(backend).unwrap() {
                if let Response::Paused { local, peer, .. } = response {
                    observed[index].paused = (local, peer);
                }
            }
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }

    for _ in 0..3 {
        cycle(&mut sessions, [&mut one, &mut two], &mut observed, [255; 2]);
        assert_eq!(one.encode_state_bytes().unwrap(), paused[0]);
        assert_eq!(two.encode_state_bytes().unwrap(), paused[1]);
        assert_eq!(observed[0].paused, (false, true));
        assert_eq!(observed[1].paused, (true, false));
    }
    sessions[1].request_pause(false).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while observed
        .iter()
        .any(|observation| observation.paused != (false, false))
    {
        for (index, backend) in [&mut one, &mut two].into_iter().enumerate() {
            while let Some(response) = sessions[index].poll(backend).unwrap() {
                if let Response::Paused { local, peer, .. } = response {
                    observed[index].paused = (local, peer);
                }
            }
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
    cycle(
        &mut sessions,
        [&mut one, &mut two],
        &mut observed,
        [sample(20, Player::One), sample(20, Player::Two)],
    );
    assert_eq!(one.frame_count(), 21);
    assert_eq!(two.frame_count(), 21);
    let (expected, audio) = reference_frame(
        &mut reference,
        [sample(18, Player::One), sample(18, Player::Two)],
        config,
    )
    .unwrap();
    for observation in &observed {
        let (actual, _, pcm) = observation.frames.last().unwrap();
        assert_eq!(actual, &expected);
        assert!(
            pcm.iter()
                .map(|v| v.to_bits())
                .eq(audio.iter().map(|v| v.to_bits()))
        );
    }
    for ((session, backend), initial) in sessions.into_iter().zip([&mut one, &mut two]).zip(initial)
    {
        session.restore(backend).unwrap();
        assert_eq!(backend.encode_state_bytes().unwrap(), initial);
        assert!(backend.nes().unwrap().host_persistence_enabled());
    }
}
