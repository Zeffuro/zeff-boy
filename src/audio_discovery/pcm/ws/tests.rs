use super::*;

fn fixture() -> PreparedWsTose {
    let bytes = zeff_audio_discovery::ws_tose::synthetic_rom();
    let cancel = AtomicBool::new(false);
    let report = zeff_audio_discovery::scan(
        zeff_emu_common::system::System::Ws,
        &bytes,
        Default::default(),
        &cancel,
    );
    assert_eq!(report.ws_tose_songs.len(), 1);
    zeff_audio_discovery::ws_tose::prepare_rom(&bytes, &report.ws_tose_songs[0], &cancel).unwrap()
}

fn options() -> RenderOptions {
    RenderOptions {
        sample_rate: 44_100,
        max_seconds: 1,
        ..Default::default()
    }
}

fn render(session: &mut WsSession, block: usize) -> Result<Vec<i16>> {
    let mut result = Vec::new();
    let mut buffer = vec![0; block];
    loop {
        let count = session.read(&mut buffer, &AtomicBool::new(false))?;
        if count == 0 {
            return Ok(result);
        }
        result.extend_from_slice(&buffer[..count]);
    }
}

#[test]
fn native_ws_handoff_audio_reset_chunking_and_mute_are_stable() -> Result<()> {
    let mut session = WsSession::new(fixture(), options(), Vec::new(), &AtomicBool::new(false))?;
    assert_eq!(session.read(&mut [], &AtomicBool::new(false))?, 0);
    assert!(session.read(&mut [0; 1], &AtomicBool::new(false)).is_err());
    assert!(session.read(&mut [0; 32], &AtomicBool::new(true)).is_err());
    assert_eq!(session.position_frames(), 0);
    let expected = render(&mut session, 2048)?;
    assert_eq!(expected.len(), 88_200);
    assert!(expected.iter().any(|&value| value != 0));
    assert_eq!(session.emulator.cpu_peek8(0x3e01), 1);
    session.reset()?;
    assert_eq!(render(&mut session, 258)?, expected);
    session.set_track_mask(0)?;
    session.reset()?;
    assert!(render(&mut session, 512)?.iter().all(|&value| value == 0));
    assert!(session.set_track_mask(2).is_err());
    Ok(())
}

#[test]
fn ws_handoff_requires_both_wait_pc_and_marker() -> Result<()> {
    let mut prepared = fixture();
    prepared.wait_start = 0xfe1f0;
    prepared.wait_end = 0xfe1f7;
    let mut session = WsSession::new(prepared, options(), Vec::new(), &AtomicBool::new(false))?;
    assert!(session.read(&mut [0; 32], &AtomicBool::new(false)).is_err());
    assert_eq!(session.emulator.cpu_peek8(0x3e01), 0);
    assert_eq!(session.position_frames(), 0);
    let mut prepared = fixture();
    prepared.ready_address = prepared.ack_address;
    assert!(WsSession::new(prepared, options(), Vec::new(), &AtomicBool::new(false)).is_err());
    let mut prepared = fixture();
    prepared.bootstrap = WsToseBootstrap::Ram;
    assert!(WsSession::new(prepared, options(), Vec::new(), &AtomicBool::new(false)).is_err());
    let mut prepared = fixture();
    prepared.bootstrap = WsToseBootstrap::Ram;
    prepared.wait_start = 0x3c12;
    prepared.wait_end = 0x3c19;
    let mut session = WsSession::new(prepared, options(), Vec::new(), &AtomicBool::new(false))?;
    assert!(session.read(&mut [0; 32], &AtomicBool::new(false)).is_err());
    assert_eq!(session.emulator.cpu_peek8(0x3e01), 0);
    assert_eq!(session.position_frames(), 0);
    let mut prepared = fixture();
    prepared.hardware = zeff_audio_discovery::ws_tose::WsToseHardware::Mono;
    assert!(WsSession::new(prepared, options(), Vec::new(), &AtomicBool::new(false)).is_err());
    Ok(())
}

#[test]
fn direct_ws_preserves_selector_banks_and_stops_at_driver_end() -> Result<()> {
    let bytes = zeff_audio_discovery::ws_tose::synthetic_direct_rom();
    let cancel = AtomicBool::new(false);
    let report = zeff_audio_discovery::scan(
        zeff_emu_common::system::System::Ws,
        &bytes,
        Default::default(),
        &cancel,
    );
    assert_eq!(report.ws_tose_songs.len(), 2);
    for (song, (first, count)) in report.ws_tose_songs.iter().zip([(0, 4), (4, 1)]) {
        let prepared = zeff_audio_discovery::ws_tose::prepare_rom(&bytes, song, &cancel)?;
        let options = RenderOptions {
            fade_seconds: 0,
            ..options()
        };
        let mut session = WsSession::new(prepared, options, Vec::new(), &cancel)?;
        assert!(session.has_source_duration_limit());
        assert_eq!(
            session.runtime_validation().unwrap()["stop_reason"],
            "driver_end"
        );
        assert!(session.duration_frames() < options.sample_rate as usize);
        let expected = render(&mut session, 2048)?;
        assert!(expected.iter().any(|&value| value != 0));
        assert_eq!(session.emulator.cpu_peek16(0x1df0), first);
        assert_eq!(session.emulator.cpu_peek16(0x1df2), count);
        assert_eq!(session.emulator.cpu_peek8(0x1df7), 0x21);
        assert_eq!(session.emulator.io_peek8(0xc3), 0xff);
        for slot in 0..8 {
            assert_eq!(session.emulator.cpu_peek16(0x1e21 + slot * 0x34), 0xffff);
        }
        session.reset()?;
        assert_eq!(render(&mut session, 258)?, expected);
        session.set_track_mask(0)?;
        session.reset()?;
        assert!(render(&mut session, 512)?.iter().all(|&value| value == 0));
        let mut prepared = zeff_audio_discovery::ws_tose::prepare_rom(&bytes, song, &cancel)?;
        prepared.timing = None;
        let mut untimed = WsSession::new(prepared, options, Vec::new(), &cancel)?;
        let full = render(&mut untimed, 2048)?;
        assert_eq!(full[..expected.len()], expected);
        assert_eq!(untimed.emulator.io_peek8(0x90) & 15, 0);
        assert_eq!(untimed.emulator.cpu_peek8(0x1df8), 0);
    }
    Ok(())
}

#[test]
fn volume_ws_runs_grouped_selectors_and_original_timer_mixer() -> Result<()> {
    let bytes = zeff_audio_discovery::ws_tose::synthetic_volume_rom();
    let cancel = AtomicBool::new(false);
    let report = zeff_audio_discovery::scan(
        zeff_emu_common::system::System::Ws,
        &bytes,
        Default::default(),
        &cancel,
    );
    assert_eq!(report.ws_tose_songs.len(), 4);
    for (song, (first, count)) in report
        .ws_tose_songs
        .iter()
        .zip([(0, 1), (1, 2), (3, 3), (6, 4)])
    {
        let prepared = zeff_audio_discovery::ws_tose::prepare_rom(&bytes, song, &cancel)?;
        let mut session = WsSession::new(prepared, options(), Vec::new(), &cancel)?;
        assert!(session.has_source_duration_limit());
        assert_eq!(
            session.runtime_validation().unwrap()["stop_reason"],
            "driver_end"
        );
        let expected = render(&mut session, 2048)?;
        assert!(expected.iter().any(|&value| value != 0));
        assert_eq!(session.emulator.cpu_peek16(0x1df0), first);
        assert_eq!(session.emulator.cpu_peek16(0x1df2), count);
        assert_eq!(session.emulator.cpu_peek8(0x4a), 63);
        assert_eq!(session.emulator.io_peek8(0xa2) & 3, 3);
        assert_eq!(session.emulator.io_peek8(0xa4), 160);
        assert_eq!(session.emulator.io_peek8(0xa5), 0);
        session.reset()?;
        assert_eq!(render(&mut session, 258)?, expected);
        session.set_track_mask(0)?;
        session.reset()?;
        assert!(render(&mut session, 512)?.iter().all(|&value| value == 0));
        let mut prepared = zeff_audio_discovery::ws_tose::prepare_rom(&bytes, song, &cancel)?;
        prepared.bytes[0x3f6000] = 0xc3;
        let mut silent = WsSession::new(prepared, options(), Vec::new(), &cancel)?;
        assert!(render(&mut silent, 2048)?.iter().all(|&value| value == 0));
    }
    Ok(())
}

#[test]
fn scaled_ws_runs_grouped_selectors_and_native_endings() -> Result<()> {
    for (bytes, bank, profile) in [
        (
            zeff_audio_discovery::ws_tose::synthetic_scaled_rom(),
            3,
            "ws-tose-scaled-v1",
        ),
        (
            zeff_audio_discovery::ws_tose::synthetic_relocated_scaled_rom(),
            0,
            "ws-tose-scaled-v2",
        ),
    ] {
        let cancel = AtomicBool::new(false);
        let report = zeff_audio_discovery::scan(
            zeff_emu_common::system::System::Ws,
            &bytes,
            Default::default(),
            &cancel,
        );
        assert_eq!(report.ws_tose_songs.len(), 4);
        for (song, (first, count)) in
            report
                .ws_tose_songs
                .iter()
                .zip([(0, 1), (1, 2), (3, 3), (6, 4)])
        {
            let prepared = zeff_audio_discovery::ws_tose::prepare_rom(&bytes, song, &cancel)?;
            let mut session = WsSession::new(prepared, options(), Vec::new(), &cancel)?;
            assert!(session.has_source_duration_limit());
            assert_eq!(
                session.runtime_validation().unwrap()["stop_reason"],
                "driver_end"
            );
            let expected = render(&mut session, 2048)?;
            assert!(expected.iter().any(|&value| value != 0));
            assert_eq!(session.emulator.cpu_peek16(0x1df0), first);
            assert_eq!(session.emulator.cpu_peek16(0x1df2), count);
            assert_eq!(session.emulator.io_peek8(0xb2), 0x40);
            assert_eq!(session.emulator.io_peek8(0xc0) & 15, bank);
            assert_eq!(session.runtime_validation().unwrap()["profile"], profile);
            session.reset()?;
            assert_eq!(render(&mut session, 258)?, expected);
            session.set_track_mask(0)?;
            session.reset()?;
            assert!(render(&mut session, 512)?.iter().all(|&value| value == 0));
        }
    }
    Ok(())
}
