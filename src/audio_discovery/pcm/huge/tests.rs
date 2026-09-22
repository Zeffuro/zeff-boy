use super::*;
use zeff_audio_discovery::{ScanLimits, catalog::SongId, scan};
use zeff_emu_common::system::System;
use zeff_gb_core::emulator::Emulator;

fn fixture() -> (Vec<u8>, HugeSong) {
    let bytes = zeff_audio_discovery::huge::fixture::rom();
    let report = scan(
        System::Gb,
        &bytes,
        ScanLimits::default(),
        &AtomicBool::new(false),
    );
    assert!(report.song(SongId::Huge(0)).is_some());
    (bytes, report.huge_songs[0].clone())
}

#[test]
fn verified_buffer_matches_unobserved_native_audio_at_requested_rates() -> Result<()> {
    let (bytes, song) = fixture();
    let cancel = AtomicBool::new(false);
    for sample_rate in [44_100, 48_000, 96_000] {
        let options = RenderOptions {
            sample_rate,
            max_seconds: 3600,
            ..Default::default()
        };
        let mut session = HugeSession::new(&bytes, &song, options, &cancel)?;
        let mut emulator = Emulator::new(&bytes, sample_rate)?;
        while emulator.cpu_cycles() < u64::from(song.validation_frames) * 70_224 {
            emulator.step_instruction();
        }
        let expected: Vec<i16> = emulator
            .drain_audio_samples()
            .into_iter()
            .map(|value| (value.clamp(-1.0, 1.0) * f32::from(i16::MAX)) as i16)
            .collect();
        assert_eq!(session.duration_frames(), expected.len() / 2);
        assert_eq!(session.sample_rate(), sample_rate);
        assert_eq!(session.proof["sample_rate"], sample_rate);
        assert_eq!(session.proof["original"]["recurrence"]["passed"], true);
        let mut actual = Vec::new();
        for chunk_size in [2, 514, 4096].into_iter().cycle() {
            let mut chunk = vec![123; chunk_size];
            let count = session.read(&mut chunk, &cancel)?;
            actual.extend_from_slice(&chunk[..count]);
            if count == 0 {
                break;
            }
        }
        assert_eq!(actual, expected);
        session.reset()?;
        let mut first = [0; 1600];
        assert_eq!(session.read(&mut first, &cancel)?, first.len());
        assert_eq!(first, expected[..first.len()]);
        session.set_track_mask(0)?;
        assert_eq!(session.read(&mut first, &cancel)?, first.len());
        assert!(first.iter().all(|value| *value == 0));
        assert!(session.set_track_mask(2).is_err());
        let position = session.position_frames();
        assert!(session.read(&mut first, &AtomicBool::new(true)).is_err());
        assert_eq!(session.position_frames(), position);
    }
    Ok(())
}

#[test]
fn caps_to_requested_maximum_and_fades_at_the_actual_endpoint() -> Result<()> {
    let (bytes, song) = fixture();
    let cancel = AtomicBool::new(false);
    let options = RenderOptions {
        max_seconds: 1,
        fade_seconds: 1,
        ..Default::default()
    };
    let mut session = HugeSession::new(&bytes, &song, options, &cancel)?;
    assert_eq!(session.duration_frames(), 48_000);
    let mut output = vec![1; 96_008];
    assert_eq!(session.read(&mut output, &cancel)?, 96_000);
    assert_eq!(&output[95_998..96_000], &[0, 0]);
    assert!(output[..95_998].iter().any(|sample| *sample != 0));
    assert_eq!(&output[96_000..], &[1; 8]);
    assert_eq!(session.read(&mut output, &cancel)?, 0);
    assert!(session.has_source_duration_limit());
    Ok(())
}

#[test]
fn source_selection_bootstrap_and_cancellation_fail_before_a_session_exists() {
    let (bytes, song) = fixture();
    let options = RenderOptions::default();
    let cancel = AtomicBool::new(false);
    assert!(HugeSession::new(&bytes, &song, options, &AtomicBool::new(true)).is_err());
    let mut changed = bytes.clone();
    changed[0x7000] ^= 1;
    assert!(HugeSession::new(&changed, &song, options, &cancel).is_err());
    let mut forged = song.clone();
    forged.bound.evidence.ram_address += 1;
    assert!(HugeSession::new(&bytes, &forged, options, &cancel).is_err());
    forged = song.clone();
    forged.validation_frames -= 1;
    assert!(HugeSession::new(&bytes, &forged, options, &cancel).is_err());
    changed = bytes;
    changed[0x150] = 0;
    forged = song;
    forged.source_sha256 = zeff_firmware::sha256_hex(&changed);
    assert!(HugeSession::new(&changed, &forged, options, &cancel).is_err());
}
