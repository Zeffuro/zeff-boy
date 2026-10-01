use super::*;

fn fixture(looping: bool, sweep: bool) -> PreparedWsTose {
    let bytes = zeff_audio_discovery::ws_tose::synthetic_legacy_rom();
    let cancel = AtomicBool::new(false);
    let scan = zeff_audio_discovery::scan(
        zeff_emu_common::system::System::Ws,
        &bytes,
        Default::default(),
        &cancel,
    );
    let mut prepared =
        zeff_audio_discovery::ws_tose::prepare_rom(&bytes, &scan.ws_tose_songs[0], &cancel)
            .unwrap();
    assert!(prepared.timing.is_none());
    let bootstrap = bytes.len() - 0x2000 + 128;
    let idle = prepared.bytes[bootstrap..bootstrap + 256]
        .windows(3)
        .position(|bytes| bytes == [0xf4, 0xeb, 0xfd])
        .unwrap();
    prepared.timing = Some(WsToseTiming::FixedV14 {
        idle_address: 0x3c00 + idle as u32 + 1,
    });
    let mut selector = Vec::new();
    for slot in 0..8_u16 {
        selector.extend([0xc7, 0x06]);
        selector.extend((0xe1d + slot * 0x2a).to_le_bytes());
        selector.extend(if slot == 0 { [1, 0] } else { [255, 255] });
    }
    selector.push(0xcb);
    prepared.bytes[0x44480..0x44480 + selector.len()].copy_from_slice(&selector);
    let mut tick = vec![0xfe, 0x06, 0, 0x0e];
    if looping {
        tick.extend([0x80, 0x26, 0, 0x0e, 3, 0xfe, 0x06, 4, 0x0e]);
    } else {
        tick.extend([
            0x80, 0x3e, 0, 0x0e, 3, 0x72, 11, 0xc7, 0x06, 0x1d, 0x0e, 255, 255, 0xb0, 0x40, 0xe6,
            0x90, 0xcb,
        ]);
    }
    tick.extend([
        0xb0,
        u8::from(sweep),
        0xe6,
        0x8c,
        0xb8,
        0,
        4,
        0xe7,
        0x80,
        0xb0,
        0xff,
        0xe6,
        0x88,
        0xb0,
        0x41,
        0xe6,
        0x90,
        0xcb,
    ]);
    prepared.bytes[0x44500..0x44500 + tick.len()].copy_from_slice(&tick);
    prepared
}

fn options() -> RenderOptions {
    RenderOptions {
        max_seconds: 1,
        sample_rate: 48_000,
        ..Default::default()
    }
}

fn render(session: &mut WsSession, block: usize) -> Vec<i16> {
    let mut result = Vec::new();
    let mut output = vec![0; block];
    loop {
        let count = session.read(&mut output, &AtomicBool::new(false)).unwrap();
        if count == 0 {
            return result;
        }
        result.extend_from_slice(&output[..count]);
    }
}

#[test]
fn end_and_control_loop_measure_actual_pcm_frames_and_replay_exactly() {
    for looping in [false, true] {
        let prepared = fixture(looping, false);
        let mut baseline = fixture(looping, false);
        baseline.timing = None;
        let mut session =
            WsSession::new(prepared, options(), vec![], &AtomicBool::new(false)).unwrap();
        let reason = if looping { "driver_loop" } else { "driver_end" };
        assert_eq!(session.runtime_validation().unwrap()["stop_reason"], reason);
        assert!(session.has_source_duration_limit());
        assert_eq!(session.position_frames(), 0);
        assert!(session.duration_frames() < 48_000);
        let mut original =
            WsSession::new(baseline, options(), vec![], &AtomicBool::new(false)).unwrap();
        let expected = render(&mut original, 512);
        let actual = render(&mut session, 2048);
        assert_eq!(actual.len(), session.duration_frames() * 2);
        assert_eq!(actual, expected[..actual.len()]);
        assert!(actual.iter().any(|&value| value != 0));
        session.reset().unwrap();
        assert_eq!(render(&mut session, 258), actual);
        session.set_track_mask(0).unwrap();
        session.reset().unwrap();
        assert!(render(&mut session, 128).iter().all(|&value| value == 0));
    }
}

#[test]
fn analysis_keeps_duration_cap_without_a_safe_control_boundary() {
    for variant in 0..3 {
        let mut prepared = fixture(true, variant == 0);
        if variant == 1 {
            prepared.timing = Some(WsToseTiming::FixedV14 {
                idle_address: 0x3cff,
            });
        }
        if variant == 2 {
            prepared.bytes[0x44500..0x44550].fill(0x90);
            prepared.bytes[0x44550] = 0xcb;
            prepared.timing = None;
        }
        let session = WsSession::new(prepared, options(), vec![], &AtomicBool::new(false)).unwrap();
        assert_eq!(session.duration_frames(), 48_000);
        assert!(!session.has_source_duration_limit());
    }
}

#[test]
fn paragraph_profiles_use_their_own_slots_and_do_not_infer_loops() {
    for (looping, slots, reason) in [
        (false, 0xe1d, "driver_end"),
        (false, 0x1000, "duration_limit"),
        (true, 0xe1d, "duration_limit"),
    ] {
        let mut prepared = fixture(looping, false);
        let Some(WsToseTiming::FixedV14 { idle_address }) = prepared.timing else {
            panic!("unexpected fixture timing");
        };
        prepared.timing = Some(WsToseTiming::FixedParagraph {
            idle_address,
            slots,
            profile: "ws-tose-fixed-paragraph-synthetic",
        });
        let mut session =
            WsSession::new(prepared, options(), vec![], &AtomicBool::new(false)).unwrap();
        assert_eq!(session.runtime_validation().unwrap()["stop_reason"], reason);
        assert_eq!(session.has_source_duration_limit(), reason == "driver_end");
        let actual = render(&mut session, 258);
        let mut baseline = fixture(looping, false);
        baseline.timing = None;
        let mut baseline =
            WsSession::new(baseline, options(), vec![], &AtomicBool::new(false)).unwrap();
        assert_eq!(actual, render(&mut baseline, 512)[..actual.len()]);
    }
}

#[test]
fn measured_duration_applies_fade_without_extending_or_reanalysing() {
    assert_eq!(fade(1000, 0, 10, 48_000, 15), 1000);
    assert_eq!(fade(1000, 9, 10, 48_000, 15), 0);
    let mut options = options();
    options.fade_seconds = 1;
    let mut session = WsSession::new(
        fixture(true, false),
        options,
        vec![],
        &AtomicBool::new(false),
    )
    .unwrap();
    let mut dry = WsSession::new(
        fixture(true, false),
        RenderOptions {
            fade_seconds: 0,
            ..options
        },
        vec![],
        &AtomicBool::new(false),
    )
    .unwrap();
    let expected: Vec<_> = render(&mut dry, 512)
        .into_iter()
        .enumerate()
        .map(|(index, value)| {
            fade(
                value,
                index / 2,
                dry.duration_frames(),
                options.sample_rate,
                1,
            )
        })
        .collect();
    assert_eq!(render(&mut session, 256), expected);
    let duration = session.duration_frames();
    session.reset().unwrap();
    assert_eq!(session.duration_frames(), duration);
    assert!(session.read(&mut [0; 32], &AtomicBool::new(true)).is_err());
    assert!(
        WsSession::new(
            fixture(true, false),
            options,
            vec![],
            &AtomicBool::new(true)
        )
        .is_err()
    );
}
