use std::io::Cursor;
use std::sync::Arc;
use std::time::{Duration, Instant};

use super::*;
use crate::audio_discovery::natsume::preview::tests::session_at;
use crate::audio_discovery::render::SAMPLE_RATES;

fn options(rate: u32, seconds: u16, fade: u8) -> Result<Options> {
    Options::from_render(RenderOptions {
        sample_rate: rate,
        max_seconds: seconds,
        fade_seconds: fade,
        ..Default::default()
    })
}

fn reference(rate: u32, seconds: u32) -> Result<Vec<i16>> {
    let cancel = AtomicBool::new(false);
    let mut session = session_at(rate, seconds, &cancel)?;
    let mut output = Vec::new();
    let mut chunk = 0;
    while session.position_frames() < session.duration_frames() {
        let mut block = vec![0; [2, 514, 18, 4096][chunk % 4]];
        let count = session.read(&mut block, &cancel)?;
        output.extend_from_slice(&block[..count]);
        chunk += 1;
    }
    Ok(output)
}

fn decoded(path: &Path, format: AudioFormat, rate: u32, frames: u64) -> Result<Vec<i16>> {
    let bytes = std::fs::read(path)?;
    if format == AudioFormat::Wav {
        let mut reader = hound::WavReader::new(Cursor::new(bytes))?;
        assert_eq!(reader.spec().channels, 2);
        assert_eq!(reader.spec().sample_rate, rate);
        assert_eq!(u64::from(reader.duration()), frames);
        Ok(reader
            .samples::<i16>()
            .collect::<std::result::Result<_, _>>()?)
    } else {
        let mut reader = claxon::FlacReader::new(Cursor::new(bytes))?;
        assert_eq!(reader.streaminfo().channels, 2);
        assert_eq!(reader.streaminfo().sample_rate, rate);
        assert_eq!(reader.streaminfo().samples, Some(frames));
        Ok(reader
            .samples()
            .map(|value| value.map(|value| value as i16))
            .collect::<std::result::Result<_, _>>()?)
    }
}

#[test]
fn lossless_native_exports_match_independent_chunked_render_at_every_rate() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let cancel = AtomicBool::new(false);
    for rate in SAMPLE_RATES {
        let expected = reference(rate, 1)?;
        assert!(expected.iter().any(|sample| *sample != 0));
        for format in [AudioFormat::Wav, AudioFormat::Flac] {
            let path = directory
                .path()
                .join(format!("{rate}.{}", format.extension()));
            let progress = AtomicU32::new(0);
            write_renderer(
                session_at(rate, 1, &cancel)?,
                Recording {
                    format,
                    options: options(rate, 1, 0)?,
                    metadata: json!({"fixture": "synthetic ARM sound driver"}),
                },
                &path,
                &cancel,
                &progress,
            )?;
            assert_eq!(progress.load(Ordering::Relaxed), 100);
            assert_eq!(decoded(&path, format, rate, u64::from(rate))?, expected);
        }
    }
    Ok(())
}

#[test]
fn fade_is_inside_the_requested_duration_and_reaches_silence() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let cancel = AtomicBool::new(false);
    let rate = 44_100;
    for seconds in [1, 2] {
        let expected = reference(rate, u32::from(seconds))?;
        let path = directory.path().join(format!("fade-{seconds}.wav"));
        write_renderer(
            session_at(rate, u32::from(seconds), &cancel)?,
            Recording {
                format: AudioFormat::Wav,
                options: options(rate, seconds, 1)?,
                metadata: json!({}),
            },
            &path,
            &cancel,
            &AtomicU32::new(0),
        )?;
        let actual = decoded(
            &path,
            AudioFormat::Wav,
            rate,
            u64::from(rate) * u64::from(seconds),
        )?;
        let fade_start = (usize::from(seconds) - 1) * rate as usize;
        assert_eq!(actual[..fade_start * 2], expected[..fade_start * 2]);
        for (offset, pair) in actual[fade_start * 2..]
            .as_chunks::<2>()
            .0
            .iter()
            .enumerate()
        {
            let numerator = i64::from(rate) - 1 - offset as i64;
            for channel in 0..2 {
                assert_eq!(
                    i64::from(pair[channel]),
                    i64::from(expected[(fade_start + offset) * 2 + channel]) * numerator
                        / (i64::from(rate) - 1)
                );
            }
        }
        assert_eq!(&actual[actual.len() - 2..], &[0, 0]);
    }
    Ok(())
}

#[test]
fn native_recording_settings_reject_unsupported_controls_and_oversize_before_render() {
    assert!(options(96_000, 5589, 0).is_ok());
    assert!(options(96_000, 5590, 0).is_err());
    assert!(options(63_072, 7200, 15).is_ok());
    for (rate, seconds, fade) in [
        (8000, 1, 0),
        (48000, 0, 0),
        (48000, 7201, 0),
        (48000, 1, 2),
        (48000, 30, 16),
    ] {
        assert!(options(rate, seconds, fade).is_err());
    }
    for unsupported in [
        RenderOptions {
            loops: 2,
            ..Default::default()
        },
        RenderOptions {
            playback_gain: PlaybackGain::Mp2kAmplitude,
            ..Default::default()
        },
        RenderOptions {
            skip_channel10: false,
            ..Default::default()
        },
        RenderOptions {
            bank_select: BankSelect::Mma,
            ..Default::default()
        },
    ] {
        assert!(Options::from_render(unsupported).is_err());
    }
}

#[test]
fn native_export_cancellation_and_existing_output_never_publish_partial_files() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let cancel = Arc::new(AtomicBool::new(false));
    let progress = Arc::new(AtomicU32::new(0));
    let path = directory.path().join("cancelled.wav");
    let mut renderer = session_at(48_000, 120, &cancel)?;
    renderer.set_track_mask(0)?;
    let child_cancel = Arc::clone(&cancel);
    let child_progress = Arc::clone(&progress);
    let child_path = path.clone();
    let worker = std::thread::spawn(move || {
        write_renderer(
            renderer,
            Recording {
                format: AudioFormat::Wav,
                options: options(48_000, 120, 0).unwrap(),
                metadata: json!({}),
            },
            &child_path,
            &child_cancel,
            &child_progress,
        )
    });
    let deadline = Instant::now() + Duration::from_secs(5);
    while progress.load(Ordering::Relaxed) == 0 {
        assert!(Instant::now() < deadline);
        std::thread::yield_now();
    }
    cancel.store(true, Ordering::Relaxed);
    assert!(worker.join().unwrap().is_err());
    assert!(!path.exists());
    assert_eq!(std::fs::read_dir(directory.path())?.count(), 0);

    let existing = directory.path().join("existing.wav");
    std::fs::write(&existing, b"keep this export")?;
    cancel.store(false, Ordering::Relaxed);
    assert!(
        write_renderer(
            session_at(48_000, 1, &cancel)?,
            Recording {
                format: AudioFormat::Wav,
                options: options(48_000, 1, 0)?,
                metadata: json!({}),
            },
            &existing,
            &cancel,
            &AtomicU32::new(0)
        )
        .is_err()
    );
    assert_eq!(std::fs::read(&existing)?, b"keep this export");
    assert_eq!(std::fs::read_dir(directory.path())?.count(), 1);
    Ok(())
}

#[cfg(feature = "audio-recording")]
#[test]
fn native_vorbis_exports_decode_with_the_requested_stereo_duration() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("driver.ogg");
    let cancel = AtomicBool::new(false);
    write_renderer(
        session_at(48_000, 1, &cancel)?,
        Recording {
            format: AudioFormat::Ogg,
            options: options(48_000, 1, 0)?,
            metadata: json!({}),
        },
        &path,
        &cancel,
        &AtomicU32::new(0),
    )?;
    let mut reader = lewton::inside_ogg::OggStreamReader::new(std::fs::File::open(path)?)?;
    assert_eq!(reader.ident_hdr.audio_sample_rate, 48_000);
    assert_eq!(reader.ident_hdr.audio_channels, 2);
    let mut pcm = Vec::new();
    while let Some(block) = reader.read_dec_packet_itl()? {
        pcm.extend(block);
    }
    assert_eq!(pcm.len(), 96_000);
    assert!(pcm.iter().any(|sample| *sample != 0));
    Ok(())
}
