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
