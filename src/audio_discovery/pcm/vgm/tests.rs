use std::{io::Write, sync::Arc};

use super::*;
use crate::audio_discovery::{
    ScanLimits,
    catalog::{SongId, SongRef},
    export::SongExportRequest,
    formats::{AudioFormat, SongFormat},
    media::{ScanInput, SourceIdentity, StandaloneFormat},
    pcm::song::PcmSong,
    preview::PreviewRequest,
};

const EVENTS: &[(u64, u8, u8)] = &[
    (0, 0x50, 0x85),
    (0, 0x50, 0x02),
    (0, 0x50, 0x90),
    (1, 0x50, 0xe4),
    (1, 0x50, 0xf5),
    (100, 0x50, 0xa1),
    (100, 0x50, 0x01),
    (101, 0x50, 0xb2),
    (399, 0x50, 0x98),
    (1000, 0x50, 0x90),
    (1200, 0x50, 0xe7),
    (1600, 0x50, 0xff),
];

fn fixture(ti: bool, stereo: bool, gzip: bool) -> Vec<u8> {
    let mut bytes = vec![0; 0x100];
    bytes[..4].copy_from_slice(b"Vgm ");
    bytes[8..12].copy_from_slice(&0x171u32.to_le_bytes());
    bytes[0x0c..0x10].copy_from_slice(&3_579_545u32.to_le_bytes());
    bytes[0x18..0x1c].copy_from_slice(&2205u32.to_le_bytes());
    bytes[0x28] = if ti { 3 } else { 9 };
    bytes[0x2a] = if ti { 15 } else { 16 };
    bytes[0x2b] = if ti {
        5
    } else if stereo {
        0
    } else {
        4
    };
    bytes[0x34..0x38].copy_from_slice(&0xccu32.to_le_bytes());
    if stereo {
        bytes.extend_from_slice(&[0x4f, 0xf0]);
    }
    let mut tick = 0;
    for &(at, op, value) in EVENTS {
        if at > tick {
            bytes.push(0x61);
            bytes.extend_from_slice(&((at - tick) as u16).to_le_bytes());
        }
        bytes.extend_from_slice(&[op, value]);
        tick = at;
    }
    bytes.push(0x61);
    bytes.extend_from_slice(&((2205 - tick) as u16).to_le_bytes());
    bytes.push(0x66);
    let eof = (bytes.len() - 4) as u32;
    bytes[4..8].copy_from_slice(&eof.to_le_bytes());
    if gzip {
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(&bytes).unwrap();
        encoder.finish().unwrap()
    } else {
        bytes
    }
}

fn log(bytes: &[u8]) -> VgmLog {
    zeff_audio_discovery::vgm::inspect(bytes, ScanLimits::default(), &AtomicBool::new(false))
        .unwrap()
        .unwrap()
}

fn read_all(session: &mut dyn PcmSession, size: usize) -> Vec<i16> {
    let mut output = Vec::new();
    let mut buffer = vec![0; size];
    loop {
        let count = session.read(&mut buffer, &AtomicBool::new(false)).unwrap();
        if count == 0 {
            break;
        }
        output.extend_from_slice(&buffer[..count]);
    }
    output
}

// A one-clock reference exercises sample boundaries independently of the session scheduler.
fn reference(ti: bool, stereo: bool, rate: u32) -> Vec<i16> {
    let mut sega =
        zeff_sega8_core::hardware::psg::Psg::new_with_sample_rate_and_clock_hz(rate, 3_579_545);
    let mut coleco = zeff_coleco_core::psg::Psg::new_with_sample_rate(rate);
    if stereo {
        sega.write_stereo_control(0xf0);
    }
    let total_frames = 2205 * u64::from(rate) / 44_100;
    let cycles = (total_frames * 3_579_545).div_ceil(u64::from(rate));
    let mut index = 0;
    let mut output = Vec::new();
    for cycle in 0..cycles {
        while let Some(&(tick, _, value)) = EVENTS.get(index) {
            if (tick * 3_579_545).div_ceil(44_100) != cycle {
                break;
            }
            if ti {
                coleco.write(value);
            } else {
                sega.write_data(value);
            }
            index += 1;
        }
        if ti {
            coleco.step_cycles(1);
            coleco.drain_audio_samples_into(&mut output);
        } else {
            sega.step_cycles(1);
            sega.drain_audio_samples_into(&mut output);
        }
    }
    output
        .into_iter()
        .map(|v| (v.clamp(-1.0, 1.0) * f32::from(i16::MAX)) as i16)
        .collect()
}

#[test]
fn clock_reference_matches_all_rates_chunk_sizes_and_reset() -> Result<()> {
    for (ti, stereo) in [(false, false), (false, true), (true, false)] {
        let bytes = fixture(ti, stereo, false);
        for rate in [44_100, 48_000, 63_072, 96_000] {
            let options = RenderOptions {
                sample_rate: rate,
                max_seconds: 1,
                ..RenderOptions::default()
            };
            let mut session =
                VgmSession::new(&bytes, &log(&bytes), options, &AtomicBool::new(false))?;
            let expected = reference(ti, stereo, rate);
            assert!(expected.iter().any(|value| *value != 0));
            for size in [2, 74, 2048, 8192] {
                session.reset()?;
                assert_eq!(
                    read_all(&mut session, size),
                    expected,
                    "ti={ti} stereo={stereo} rate={rate} size={size}"
                );
                assert_eq!(session.position_frames(), session.duration_frames());
            }
            if stereo {
                assert!(expected.as_chunks::<2>().0.iter().all(|pair| pair[1] == 0));
            }
        }
    }
    Ok(())
}

#[test]
fn wait_one_emits_the_prior_state_before_the_next_write() -> Result<()> {
    for rate in [44_100, 48_000, 63_072, 96_000] {
        let mut bytes = fixture(false, false, false);
        bytes.truncate(0x100);
        bytes[0x18..0x1c].copy_from_slice(&2u32.to_le_bytes());
        bytes.extend_from_slice(&[0x50, 0x90, 0x70, 0x50, 0x9f, 0x70, 0x66]);
        let eof = (bytes.len() - 4) as u32;
        bytes[4..8].copy_from_slice(&eof.to_le_bytes());
        let options = RenderOptions {
            sample_rate: rate,
            ..RenderOptions::default()
        };
        let mut session = VgmSession::new(&bytes, &log(&bytes), options, &AtomicBool::new(false))?;
        let pcm = read_all(&mut session, 2);
        let prior_frames = rate as usize / 44_100;
        assert!(pcm[..prior_frames * 2].iter().all(|sample| *sample > 0));
        assert!(pcm[prior_frames * 2..].iter().all(|sample| *sample == 0));
    }
    Ok(())
}

#[test]
fn duration_fade_mute_cancellation_and_source_identity_are_enforced() -> Result<()> {
    let bytes = fixture(false, false, false);
    let inventory = log(&bytes);
    let options = RenderOptions {
        max_seconds: 1,
        ..RenderOptions::default()
    };
    let mut session = VgmSession::new(&bytes, &inventory, options, &AtomicBool::new(false))?;
    let original = read_all(&mut session, 128);
    assert_eq!(original.len(), 4800);
    assert!(session.has_source_duration_limit());
    session.reset()?;
    assert!(session.read(&mut [0; 2], &AtomicBool::new(true)).is_err());
    assert_eq!(session.position_frames(), 0);
    assert!(session.read(&mut [0; 3], &AtomicBool::new(false)).is_err());
    assert!(session.set_track_mask(2).is_err());
    session.set_track_mask(0)?;
    assert!(read_all(&mut session, 512).iter().all(|value| *value == 0));
    session.reset()?;
    session.set_track_mask(1)?;
    assert_eq!(read_all(&mut session, 512), original);
    let mut faded = VgmSession::new(
        &bytes,
        &inventory,
        RenderOptions {
            fade_seconds: 1,
            ..options
        },
        &AtomicBool::new(false),
    )?;
    let faded = read_all(&mut faded, 1024);
    assert_eq!(&faded[faded.len() - 2..], &[0, 0]);
    assert_ne!(faded, original);
    let mut stale = inventory.clone();
    stale.samples += 1;
    assert!(VgmSession::new(&bytes, &stale, options, &AtomicBool::new(false)).is_err());
    assert!(
        VgmSession::new(
            &bytes,
            &inventory,
            RenderOptions {
                loops: 2,
                ..options
            },
            &AtomicBool::new(false)
        )
        .is_err()
    );
    Ok(())
}

#[test]
fn preview_audio_export_and_preservation_share_verified_sources() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let mut expected = None;
    for gzip in [false, true] {
        let bytes = fixture(false, true, gzip);
        let source = SourceIdentity {
            kind: "synthetic-vgm",
            sha256: zeff_firmware::sha256_hex(&bytes),
            len: bytes.len(),
            container: None,
            selected_member: None,
        };
        let input = Arc::new(ScanInput::standalone(
            bytes,
            source,
            StandaloneFormat::Vgm,
            None,
        ));
        let manifest = input.analyze(ScanLimits::default(), &AtomicBool::new(false));
        let song = manifest.scan.song(SongId::Vgm(0)).unwrap();
        assert!(song.supports(SongFormat::Audio(AudioFormat::Wav)));
        assert!(PcmSong::can_play(song));
        assert!(!PcmSong::is_native(song));
        assert!(
            crate::audio_discovery::vgm_export::VgmExportRequest::prepare(
                &input,
                &manifest,
                &manifest.scan.vgm_logs[0],
                SongFormat::Audio(AudioFormat::Wav)
            )
            .is_err()
        );
        assert!(PreviewRequest::can_preview(&manifest, SongId::Vgm(0)));
        PreviewRequest::prepare_song(&input, &manifest, SongId::Vgm(0), RenderOptions::default())?;
        let mut session = PcmSong::from_ref(song).unwrap().session(
            &input.bytes,
            RenderOptions::default(),
            &AtomicBool::new(false),
        )?;
        let pcm = read_all(session.as_mut(), 190);
        if let Some(expected) = &expected {
            assert_eq!(&pcm, expected);
        } else {
            expected = Some(pcm.clone());
        }
        let path = directory.path().join(format!("audio-{gzip}.wav"));
        let request = || {
            SongExportRequest::prepare(
                &input,
                &manifest,
                SongId::Vgm(0),
                SongFormat::Audio(AudioFormat::Wav),
                RenderOptions::default(),
            )
        };
        request()?.write_new(
            &path,
            &AtomicBool::new(false),
            &std::sync::atomic::AtomicU32::new(0),
        )?;
        let mut wav = hound::WavReader::open(&path)?;
        assert_eq!(wav.spec().sample_rate, 48_000);
        assert_eq!(
            wav.samples::<i16>()
                .collect::<std::result::Result<Vec<_>, _>>()?,
            pcm
        );
        assert!(
            request()?
                .write_new(
                    &path,
                    &AtomicBool::new(false),
                    &std::sync::atomic::AtomicU32::new(0)
                )
                .is_err()
        );
        let preserved = directory.path().join(format!("preserved-{gzip}.vgm"));
        SongExportRequest::prepare(
            &input,
            &manifest,
            SongId::Vgm(0),
            SongFormat::Vgm,
            RenderOptions::default(),
        )?
        .write_new(
            &preserved,
            &AtomicBool::new(false),
            &std::sync::atomic::AtomicU32::new(0),
        )?;
        assert_eq!(std::fs::read(preserved)?, fixture(false, true, false));
        let mut invalid = manifest.clone();
        invalid.scan.vgm_logs[0].sn_playback = None;
        assert!(!PcmSong::can_play(SongRef::Vgm(&invalid.scan.vgm_logs[0])));
        assert!(!PreviewRequest::can_preview(&invalid, SongId::Vgm(0)));
    }
    Ok(())
}
