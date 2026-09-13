use super::*;
use crate::audio_discovery::tracker::xm;

fn xm_fixture() -> Result<Vec<u8>> {
    let mut cells = vec![xm::Cell::default(); 8];
    cells[0] = xm::Cell {
        note: 49,
        instrument: 1,
        ..Default::default()
    };
    let module = xm::Module {
        name: "Playback fixture".to_owned(),
        channels: 1,
        orders: vec![0],
        restart: 0,
        speed: 6,
        bpm: 125,
        linear_frequency: true,
        patterns: vec![xm::Pattern { rows: 8, cells }],
        instruments: vec![xm::Instrument {
            samples: vec![xm::Sample {
                name: "Pulse".to_owned(),
                pcm: (0..64)
                    .map(|i| if i < 32 { 16000 } else { -16000 })
                    .collect(),
                sixteen_bit: true,
                loop_range: Some((0, 64)),
                ping_pong: false,
                volume: 64,
                panning: 128,
                relative_note: 0,
                finetune: 0,
            }],
            ..Default::default()
        }],
    };
    xm::encode(&module, &AtomicBool::new(false))
}

fn options() -> RenderOptions {
    RenderOptions {
        max_seconds: 1,
        sample_rate: 44_100,
        ..Default::default()
    }
}

fn tracker(options: RenderOptions) -> Result<Box<dyn PcmSession>> {
    Ok(Box::new(tracker::TrackerSession::from_xm(
        &xm_fixture()?,
        options,
        Vec::new(),
        &AtomicBool::new(false),
    )?))
}

fn render(session: &mut dyn PcmSession, block: usize) -> Result<Vec<i16>> {
    let mut result = Vec::new();
    let mut chunk = vec![0; block];
    let cancel = AtomicBool::new(false);
    while session.position_frames() < session.duration_frames() {
        let count = session.read(&mut chunk, &cancel)?;
        ensure!(count > 0, "test session stopped early");
        result.extend_from_slice(&chunk[..count]);
    }
    assert_eq!(session.read(&mut chunk, &cancel)?, 0);
    Ok(result)
}

#[test]
fn tracker_stream_reset_seek_and_masks_preserve_sample_boundaries() -> Result<()> {
    let mut session = tracker(options())?;
    let expected = render(session.as_mut(), 2048)?;
    assert_eq!(expected.len(), 88200);
    assert!(expected.iter().any(|v| *v != 0));
    session.reset()?;
    assert_eq!(render(session.as_mut(), 514)?, expected);

    session.reset()?;
    let mut skipped = vec![0; 1234];
    assert_eq!(
        session.read(&mut skipped, &AtomicBool::new(false))?,
        skipped.len()
    );
    assert_eq!(render(session.as_mut(), 202)?, expected[1234..]);
    session.set_track_mask(0)?;
    session.reset()?;
    assert!(render(session.as_mut(), 512)?.iter().all(|v| *v == 0));
    assert!(session.set_track_mask(2).is_err());
    Ok(())
}

#[test]
fn wav_export_uses_the_preview_pcm_including_fade() -> Result<()> {
    let options = RenderOptions {
        fade_seconds: 1,
        ..options()
    };
    let expected = render(tracker(options)?.as_mut(), 258)?;
    assert_eq!(&expected[expected.len() - 2..], &[0, 0]);
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("song.wav");
    write_new(
        tracker(options)?,
        AudioFormat::Wav,
        options,
        serde_json::json!({"engine": "synthetic_xm"}),
        &path,
        &AtomicBool::new(false),
        &AtomicU32::new(0),
    )?;
    let mut wave = hound::WavReader::open(&path)?;
    assert_eq!(wave.spec().channels, 2);
    assert_eq!(wave.spec().sample_rate, 44100);
    assert_eq!(
        wave.samples::<i16>()
            .collect::<std::result::Result<Vec<_>, _>>()?,
        expected
    );
    let original = std::fs::read(&path)?;
    assert!(
        write_new(
            tracker(options)?,
            AudioFormat::Wav,
            options,
            serde_json::json!({}),
            &path,
            &AtomicBool::new(false),
            &AtomicU32::new(0)
        )
        .is_err()
    );
    assert_eq!(std::fs::read(path)?, original);
    Ok(())
}

#[test]
fn tracker_cancellation_invalid_options_and_truncation_fail_without_output() -> Result<()> {
    let cancel = AtomicBool::new(true);
    assert!(
        tracker::TrackerSession::from_xm(&xm_fixture()?, options(), Vec::new(), &cancel).is_err()
    );
    let mut session = tracker(options())?;
    assert!(session.read(&mut [0; 256], &cancel).is_err());
    assert_eq!(session.position_frames(), 0);
    assert!(session.read(&mut [0; 3], &AtomicBool::new(false)).is_err());
    assert!(
        tracker(RenderOptions {
            sample_rate: 0,
            ..options()
        })
        .is_err()
    );
    assert!(
        tracker(RenderOptions {
            loops: 2,
            ..options()
        })
        .is_err()
    );
    assert!(
        tracker(RenderOptions {
            max_seconds: 0,
            ..options()
        })
        .is_err()
    );
    for end in [0, 17, 59, 80, 337] {
        assert!(
            tracker::TrackerSession::from_xm(
                &xm_fixture()?[..end],
                options(),
                Vec::new(),
                &AtomicBool::new(false)
            )
            .is_err()
        );
    }
    Ok(())
}

fn gba_fixture() -> Vec<u8> {
    let mut rom = vec![0; 0x1000];
    rom[0xB2] = 0x96;
    rom[..4].copy_from_slice(&0xEA00_002Eu32.to_le_bytes());
    let words: [u32; 16] = [
        0xE59F_1024,
        0xE3A0_2080,
        0xE5C1_2004,
        0xE59F_201C,
        0xE581_2000,
        0xE59F_1018,
        0xE59F_2018,
        0xE581_2000,
        0xE59F_2014,
        0xE581_2004,
        0xEAFF_FFFE,
        0x0400_0080,
        0x0002_1177,
        0x0400_0060,
        0xF080_0000,
        0x0000_8700,
    ];
    for (slot, value) in rom[0xC0..].as_chunks_mut::<4>().0.iter_mut().zip(words) {
        slot.copy_from_slice(&value.to_le_bytes());
    }
    rom
}

#[test]
fn isolated_gba_stream_is_repeatable_and_has_bounded_duration() -> Result<()> {
    let mut session = gba::GbaSession::new(
        gba_fixture(),
        options(),
        Vec::new(),
        &AtomicBool::new(false),
    )?;
    let expected = render(&mut session, 2048)?;
    assert_eq!(expected.len(), 88200);
    assert!(expected.iter().any(|v| *v != 0));
    session.reset()?;
    assert_eq!(render(&mut session, 258)?, expected);
    session.set_track_mask(0)?;
    session.reset()?;
    assert!(render(&mut session, 2048)?.iter().all(|v| *v == 0));
    Ok(())
}

#[test]
fn native_initialization_handoff_preserves_audio_on_reset_and_export() -> Result<()> {
    let mut rom = gba_fixture();
    rom[..4].copy_from_slice(&0xea00_007eu32.to_le_bytes());
    let words: [u32; 11] = [
        0xe59f_001c,
        0xe3a0_1000,
        0xe580_1004,
        0xe59f_1014,
        0xe580_1000,
        0xe590_1004,
        0xe351_0001,
        0x1aff_fffc,
        0xeaff_ffa6,
        0x0300_7fe0,
        0x4155_4449,
    ];
    for (slot, value) in rom[0x200..].as_chunks_mut::<4>().0.iter_mut().zip(words) {
        slot.copy_from_slice(&value.to_le_bytes());
    }
    let wait_loop = crate::audio_discovery::RomSpan {
        effective_offset: 0x214,
        canonical_cpu_address: 0x0800_0214,
        byte_len: 12,
    };
    let make = || {
        gba::GbaSession::new_ready(
            rom.clone(),
            wait_loop,
            options(),
            Vec::new(),
            &AtomicBool::new(false),
        )
    };
    let mut session = make()?;
    assert_eq!(session.read(&mut [], &AtomicBool::new(false))?, 0);
    assert!(session.read(&mut [0; 128], &AtomicBool::new(true)).is_err());
    assert_eq!(session.position_frames(), 0);
    let expected = render(&mut session, 2048)?;
    assert!(expected.iter().any(|sample| *sample != 0));
    session.reset()?;
    assert_eq!(render(&mut session, 258)?, expected);
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("native.wav");
    write_new(
        Box::new(make()?),
        AudioFormat::Wav,
        options(),
        serde_json::json!({}),
        &path,
        &AtomicBool::new(false),
        &AtomicU32::new(0),
    )?;
    let mut wave = hound::WavReader::open(path)?;
    assert_eq!(
        wave.samples::<i16>()
            .collect::<std::result::Result<Vec<_>, _>>()?,
        expected
    );
    Ok(())
}
