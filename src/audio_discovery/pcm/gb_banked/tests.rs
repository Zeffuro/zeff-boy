use super::*;

fn fixture() -> PreparedGbBanked {
    use zeff_audio_discovery::gb_music::native;
    native::prepare_rom(
        &native::fixture_rom(),
        &native::fixture_song(),
        &AtomicBool::new(false),
    )
    .unwrap()
}

fn options() -> RenderOptions {
    RenderOptions {
        max_seconds: 1,
        sample_rate: 44_100,
        ..Default::default()
    }
}

fn render(session: &mut GbBankedSession, block: usize) -> Result<Vec<i16>> {
    let mut result = Vec::new();
    let mut chunk = vec![0; block];
    loop {
        let count = session.read(&mut chunk, &AtomicBool::new(false))?;
        if count == 0 {
            return Ok(result);
        }
        result.extend_from_slice(&chunk[..count]);
    }
}

#[test]
fn cgb_native_handoff_chunking_reset_and_requested_duration_are_stable() -> Result<()> {
    let mut session =
        GbBankedSession::new(fixture(), options(), Vec::new(), &AtomicBool::new(false))?;
    assert!(!session.has_source_duration_limit());
    assert_eq!(session.duration_frames(), 44_100);
    assert_eq!(session.read(&mut [], &AtomicBool::new(false))?, 0);
    assert!(session.read(&mut [0; 64], &AtomicBool::new(true)).is_err());
    assert_eq!(session.position_frames(), 0);
    let expected = render(&mut session, 2048)?;
    assert_eq!(expected.len(), 88_200);
    assert!(expected.iter().any(|&sample| sample != 0));
    assert_eq!(session.emulator.hardware_mode(), HardwareMode::CGBNormal);
    assert_eq!(session.emulator.cpu_peek8(0xc103), 0x11);
    assert_eq!(session.emulator.cpu_peek8(0xc101), 1);
    assert!(session.emulator.cpu_peek8(0xc100) > 0);
    session.reset()?;
    assert_eq!(render(&mut session, 258)?, expected);
    session.set_track_mask(0)?;
    session.reset()?;
    assert!(render(&mut session, 512)?.iter().all(|&sample| sample == 0));
    assert!(session.set_track_mask(2).is_err());
    Ok(())
}

#[test]
fn native_handoff_needs_both_marker_and_wait_pc() -> Result<()> {
    let mut prepared = fixture();
    prepared.wait_start = 0xf0;
    prepared.wait_end = 0xf4;
    let ack = prepared.ack_address;
    let mut session =
        GbBankedSession::new(prepared, options(), Vec::new(), &AtomicBool::new(false))?;
    assert!(session.read(&mut [0; 64], &AtomicBool::new(false)).is_err());
    assert_eq!(session.emulator.cpu_peek8(ack), 0);
    assert_eq!(session.position_frames(), 0);
    Ok(())
}

#[test]
fn native_cgb_cartridge_and_handoff_contracts_are_required() {
    for (offset, value) in [(0x143, 0), (0x147, 0x13), (0x148, 5), (0x149, 0)] {
        let mut prepared = fixture();
        prepared.bytes[offset] = value;
        assert!(
            GbBankedSession::new(prepared, options(), Vec::new(), &AtomicBool::new(false)).is_err()
        );
    }
    let mut prepared = fixture();
    prepared.ack_address = prepared.ready_address;
    assert!(
        GbBankedSession::new(prepared, options(), Vec::new(), &AtomicBool::new(false)).is_err()
    );
}
