use super::*;
use crate::audio_discovery::{formats::AudioFormat, pcm::write_new};
use std::sync::atomic::AtomicU32;

fn fixture() -> PreparedNesNative {
    let bytes = zeff_audio_discovery::nes_native::fixture_rom();
    let report = zeff_audio_discovery::scan(
        zeff_emu_common::system::System::Nes,
        &bytes,
        Default::default(),
        &AtomicBool::new(false),
    );
    zeff_audio_discovery::nes_native::prepare_rom(
        &bytes,
        &report.nes_native_songs[0],
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
fn make() -> Result<NesSession> {
    NesSession::new(fixture(), options(), Vec::new(), &AtomicBool::new(false))
}
fn render(session: &mut NesSession, block: usize) -> Result<Vec<i16>> {
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
fn native_handoff_reset_and_recording_match() -> Result<()> {
    let mut session = make()?;
    assert_eq!(session.read(&mut [], &AtomicBool::new(false))?, 0);
    assert!(session.read(&mut [0; 64], &AtomicBool::new(true)).is_err());
    assert_eq!(session.position_frames(), 0);
    let expected = render(&mut session, 2048)?;
    assert_eq!(expected.len(), 88_200);
    assert!(expected.iter().any(|sample| *sample != 0));
    assert!(
        expected
            .as_chunks::<2>()
            .0
            .iter()
            .all(|frame| frame[0] == frame[1])
    );
    session.reset()?;
    assert_eq!(render(&mut session, 258)?, expected);
    session.set_track_mask(0)?;
    session.reset()?;
    assert!(render(&mut session, 512)?.iter().all(|sample| *sample == 0));
    assert!(session.set_track_mask(2).is_err());
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("nes.wav");
    write_new(
        Box::new(make()?),
        AudioFormat::Wav,
        options(),
        serde_json::json!({}),
        &path,
        &AtomicBool::new(false),
        &AtomicU32::new(0),
    )?;
    assert_eq!(
        hound::WavReader::open(path)?
            .samples::<i16>()
            .collect::<std::result::Result<Vec<_>, _>>()?,
        expected
    );
    Ok(())
}
#[test]
fn ready_marker_alone_does_not_release_the_song() -> Result<()> {
    let mut prepared = fixture();
    prepared.wait_start = 0x8200;
    prepared.wait_end = 0x8204;
    let ack = prepared.ack_address;
    let mut session = NesSession::new(prepared, options(), Vec::new(), &AtomicBool::new(false))?;
    assert!(session.read(&mut [0; 64], &AtomicBool::new(false)).is_err());
    assert_eq!(session.emulator.cpu_peek8(ack), 0);
    assert_eq!(session.position_frames(), 0);
    Ok(())
}

#[test]
fn mmc1_original_startup_selectors_reset_and_recording_match() -> Result<()> {
    let bytes = zeff_audio_discovery::nes_native::fixture_rom_nintendo();
    let cancel = AtomicBool::new(false);
    let report = zeff_audio_discovery::scan(
        zeff_emu_common::system::System::Nes,
        &bytes,
        Default::default(),
        &cancel,
    );
    assert_eq!(report.nes_native_songs.len(), 2);
    let mut previous = None;
    for song in &report.nes_native_songs {
        let make = || {
            NesSession::new(
                zeff_audio_discovery::nes_native::prepare_rom(&bytes, song, &cancel)?,
                options(),
                Vec::new(),
                &cancel,
            )
        };
        let mut session = make()?;
        assert_eq!(session.emulator.cartridge_header().mapper_id, 1);
        assert!(session.read(&mut [0; 64], &AtomicBool::new(true)).is_err());
        assert_eq!(session.position_frames(), 0);
        let expected = render(&mut session, 258)?;
        assert_eq!(expected.len(), 88_200);
        assert!(expected.iter().any(|&sample| sample != 0));
        assert_eq!(session.emulator.cpu_peek8(0xeb), 0x55);
        assert_eq!(session.emulator.cpu_peek8(0x06fd), song.raw_index);
        if let Some(previous) = previous.replace(expected.clone()) {
            assert_ne!(previous, expected);
        }
        session.reset()?;
        assert_eq!(render(&mut session, 4096)?, expected);
        session.set_track_mask(0)?;
        session.reset()?;
        assert!(render(&mut session, 512)?.iter().all(|&sample| sample == 0));
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("mmc1.wav");
        write_new(
            Box::new(make()?),
            AudioFormat::Wav,
            options(),
            serde_json::json!({}),
            &path,
            &cancel,
            &AtomicU32::new(0),
        )?;
        assert_eq!(
            hound::WavReader::open(path)?
                .samples::<i16>()
                .collect::<std::result::Result<Vec<_>, _>>()?,
            expected
        );
    }
    Ok(())
}
