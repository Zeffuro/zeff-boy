use super::*;
use zeff_emu_common::audio_trace::AudioTraceWrite;

pub(crate) fn fixture(path: &Path, silent: bool, seconds: u64) -> Result<CaptureArtifact> {
    let (mut trace, context) = crate::audio_discovery::trace_capture::tests::sega_trace("sms");
    let mut event = trace.events[0];
    trace.events.clear();
    if !silent {
        for (second, value) in [(0, 0x80), (0, 0x10), (0, 0x90), (1, 0x9f), (3, 0x90)] {
            event.cycle = second * u64::from(trace.cycle_hz);
            event.write = AudioTraceWrite::Sn76489 { port: 0x7f, value };
            trace.events.push(event);
        }
    }
    trace.end_cycle = seconds * u64::from(trace.cycle_hz);
    crate::audio_discovery::trace_capture::write_new(
        path,
        &trace,
        context,
        &AtomicBool::new(false),
    )?;
    CaptureArtifact::load(path)
}

fn output(path: &Path) -> Result<StableDirectory> {
    StableDirectory::open_or_create(path, "excerpt test")
}

#[test]
fn excerpts_match_full_replay_slices_and_keep_capture_identity() -> Result<()> {
    let temp = tempfile::tempdir()?;
    let artifact = fixture(&temp.path().join("capture.zip"), false, 4)?;
    let directory = output(&temp.path().join("excerpts"))?;
    let cancel = AtomicBool::new(false);
    let result = extract(&artifact, &directory, 44100, None, &cancel)?;
    assert_eq!(result["status"], "complete");
    assert_eq!(result["excerpt_count"], 2);
    assert_eq!(result["archive_sha256"], artifact.archive_sha256);
    assert_eq!(result["trace_sha256"], artifact.trace_sha256);
    assert_eq!(result["capture_manifest"], artifact.manifest);
    assert_eq!(
        result["segmentation"]["frame_ranges"],
        "half_open_stereo_pcm"
    );
    assert_eq!(result["excerpts"][1]["ends_at_capture_edge"], true);
    let options = RenderOptions {
        sample_rate: 44100,
        max_seconds: 5,
        ..Default::default()
    };
    let mut session = artifact.session(options, &cancel)?;
    let mut whole = Vec::new();
    let mut samples = [0_i16; 514];
    loop {
        let count = session.read(&mut samples, &cancel)?;
        if count == 0 {
            break;
        }
        whole.extend_from_slice(&samples[..count]);
    }
    for row in result["excerpts"].as_array().unwrap() {
        let path = directory.path().join(row["file"].as_str().unwrap());
        let mut reader = hound::WavReader::open(&path)?;
        let actual: Vec<_> = reader
            .samples::<i16>()
            .collect::<std::result::Result<_, _>>()?;
        let start = row["start_frame"].as_u64().unwrap() as usize;
        let end = row["end_frame"].as_u64().unwrap() as usize;
        assert_eq!(actual, whole[start * 2..end * 2]);
        let bytes: Vec<_> = actual
            .iter()
            .flat_map(|sample| sample.to_le_bytes())
            .collect();
        assert_eq!(row["pcm_sha256"], zeff_firmware::sha256_hex(&bytes));
        assert_eq!(row["wav_pcm_verified"], true);
        let wav = std::fs::read(path)?;
        assert!(
            wav.windows(artifact.trace_sha256.len())
                .any(|part| part == artifact.trace_sha256.as_bytes())
        );
    }
    Ok(())
}

#[test]
fn silence_has_no_excerpts_and_long_capture_is_rejected() -> Result<()> {
    let temp = tempfile::tempdir()?;
    let artifact = fixture(&temp.path().join("silent.zip"), true, 1)?;
    let directory = output(&temp.path().join("silent"))?;
    let result = extract(&artifact, &directory, 48000, None, &AtomicBool::new(false))?;
    assert_eq!(result["excerpt_count"], 0);
    assert_eq!(result["source_playback"]["silent"], true);
    assert_eq!(std::fs::read_dir(directory.path())?.count(), 0);
    let artifact = fixture(&temp.path().join("long.zip"), true, 121)?;
    assert!(
        extract(&artifact, &directory, 48000, None, &AtomicBool::new(false))
            .unwrap_err()
            .to_string()
            .contains("complete capture within 120 seconds")
    );
    Ok(())
}

#[test]
fn mismatch_and_cancellation_publish_no_excerpts_and_existing_files_survive() -> Result<()> {
    let temp = tempfile::tempdir()?;
    let artifact = fixture(&temp.path().join("capture.zip"), false, 4)?;
    let directory = output(&temp.path().join("out"))?;
    let reference = temp.path().join("native.f32");
    std::fs::write(&reference, [0; 8])?;
    assert!(
        extract(
            &artifact,
            &directory,
            48000,
            Some(&reference),
            &AtomicBool::new(false)
        )
        .is_err()
    );
    assert!(extract(&artifact, &directory, 48000, None, &AtomicBool::new(true)).is_err());
    assert_eq!(std::fs::read_dir(directory.path())?.count(), 0);
    let sentinel = directory.path().join("excerpt-000.wav");
    std::fs::write(&sentinel, b"sentinel")?;
    assert!(extract(&artifact, &directory, 48000, None, &AtomicBool::new(false)).is_err());
    assert_eq!(std::fs::read(sentinel)?, b"sentinel");
    Ok(())
}

#[test]
fn wav_verification_rejects_wrong_pcm_rate_and_length() -> Result<()> {
    let mut source = tempfile::tempfile()?;
    source.write_all(&[1, 0, 255, 255])?;
    let info = AudioInfo {
        frames: 1,
        channels: 2,
        sample_rate: 48000,
        loop_range: None,
        pitch: None,
    };
    let cancel = AtomicBool::new(false);
    let mut wav = tempfile::tempfile()?;
    audio_file::encode_to(
        AudioFormat::Wav,
        &mut source,
        info,
        b"{}",
        &cancel,
        &mut wav,
    )?;
    let hash = zeff_firmware::sha256_hex(&[1, 0, 255, 255]);
    verify_wav(&mut wav, info, &hash, &cancel)?;
    assert!(verify_wav(&mut wav, info, &"0".repeat(64), &cancel).is_err());
    assert!(verify_wav(&mut wav, AudioInfo { frames: 2, ..info }, &hash, &cancel).is_err());
    assert!(
        verify_wav(
            &mut wav,
            AudioInfo {
                sample_rate: 44100,
                ..info
            },
            &hash,
            &cancel
        )
        .is_err()
    );
    Ok(())
}

#[test]
fn duplicate_evidence_requires_exact_length_and_pcm() {
    let rows = [
        json!({"frames": 20, "pcm_sha256": "a"}),
        json!({"frames": 20, "pcm_sha256": "a"}),
        json!({"frames": 21, "pcm_sha256": "a"}),
        json!({"frames": 20, "pcm_sha256": "b"}),
    ];
    assert_eq!(
        duplicates(&rows, 48000),
        vec![json!({"sample_rate": 48000, "frames": 20,
        "pcm_sha256": "a", "excerpt_indices": [0, 1]})]
    );
}
