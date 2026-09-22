use super::*;
use zeff_emu_common::audio_trace::AudioTraceWrite;

fn capture(
    path: &Path,
    system: &str,
    schedule: impl FnOnce(u64) -> Vec<(u64, AudioTraceWrite)>,
) -> Result<CaptureArtifact> {
    let (mut trace, context) = crate::audio_discovery::trace_capture::tests::sega_trace(system);
    let template = trace.events[0];
    let clock = u64::from(trace.cycle_hz);
    trace.events = schedule(clock)
        .into_iter()
        .map(
            |(cycle, write)| zeff_emu_common::audio_trace::AudioTraceEvent {
                cycle,
                write,
                ..template
            },
        )
        .collect();
    trace.end_cycle = clock * 3;
    crate::audio_discovery::trace_capture::write_new(
        path,
        &trace,
        context,
        &AtomicBool::new(false),
    )?;
    CaptureArtifact::load(path)
}

fn write(value: u8) -> AudioTraceWrite {
    AudioTraceWrite::Sn76489 { port: 0x7f, value }
}

fn verify_slices(artifact: &CaptureArtifact, directory: &Path, result: &Value) -> Result<()> {
    let cancel = AtomicBool::new(false);
    let mut session = artifact.session(
        RenderOptions {
            max_seconds: 4,
            sample_rate: 48000,
            ..Default::default()
        },
        &cancel,
    )?;
    let mut source = Vec::new();
    let mut buffer = [0; 514];
    loop {
        let count = session.read(&mut buffer, &cancel)?;
        if count == 0 {
            break;
        }
        source.extend_from_slice(&buffer[..count]);
    }
    for row in result["excerpts"].as_array().unwrap() {
        let mut wav = hound::WavReader::open(directory.join(row["file"].as_str().unwrap()))?;
        let samples = wav
            .samples::<i16>()
            .collect::<std::result::Result<Vec<_>, _>>()?;
        let start = row["start_frame"].as_u64().unwrap() as usize;
        let end = row["end_frame"].as_u64().unwrap() as usize;
        assert_eq!(samples, source[start * 2..end * 2]);
        assert_eq!(row["wav_pcm_verified"], true);
    }
    Ok(())
}

#[test]
fn phase_cancellation_does_not_split_an_unmuted_capture() -> Result<()> {
    let temp = tempfile::tempdir()?;
    let artifact = capture(&temp.path().join("capture.zip"), "sms", |clock| {
        let start = (clock / 64 + 1) * 64 + 32;
        vec![
            (0, write(0x82)),
            (0, write(0x00)),
            (0, write(0x90)),
            (start, write(0xa2)),
            (start, write(0x00)),
            (start, write(0xb0)),
            (clock * 2, write(0xbf)),
        ]
    })?;
    let directory = StableDirectory::open_or_create(&temp.path().join("out"), "excerpt test")?;
    let result = extract(&artifact, &directory, 48000, None, &AtomicBool::new(false))?;
    assert_eq!(
        result["source_playback"]["pcm"]["activity_intervals"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(result["excerpt_count"], 1);
    let support = &result["segmentation"]["native_mute_support"];
    assert_eq!(support["merged_gap_count"], 1);
    assert_eq!(support["gap_decisions"][0]["longest_muted_frames"], 0);
    assert_eq!(support["gap_decisions"][0]["retained_split"], false);
    verify_slices(&artifact, directory.path(), &result)
}

#[test]
fn stereo_disconnect_supports_a_quiet_gap() -> Result<()> {
    let temp = tempfile::tempdir()?;
    let artifact = capture(&temp.path().join("capture.zip"), "gg", |clock| {
        vec![
            (0, write(0x90)),
            (clock, AudioTraceWrite::GameGearStereo { port: 6, value: 0 }),
            (
                clock * 2,
                AudioTraceWrite::GameGearStereo {
                    port: 6,
                    value: 0x11,
                },
            ),
        ]
    })?;
    let directory = StableDirectory::open_or_create(&temp.path().join("out"), "excerpt test")?;
    let result = extract(&artifact, &directory, 48000, None, &AtomicBool::new(false))?;
    assert_eq!(result["excerpt_count"], 2);
    assert_eq!(
        result["segmentation"]["native_mute_support"]["merged_gap_count"],
        0
    );
    assert_eq!(
        result["segmentation"]["native_mute_support"]["gap_decisions"][0]["longest_muted_frames"],
        48000
    );
    verify_slices(&artifact, directory.path(), &result)
}

#[test]
fn inaudible_unmute_interrupts_native_mute_support() -> Result<()> {
    let temp = tempfile::tempdir()?;
    let artifact = capture(&temp.path().join("capture.zip"), "sms", |clock| {
        vec![
            (0, write(0x90)),
            (clock, write(0x9f)),
            (clock * 3 / 2, write(0x90)),
            (clock * 3 / 2, write(0x9f)),
            (clock * 2, write(0x90)),
        ]
    })?;
    let directory = StableDirectory::open_or_create(&temp.path().join("out"), "excerpt test")?;
    let result = extract(&artifact, &directory, 48000, None, &AtomicBool::new(false))?;
    assert_eq!(
        result["source_playback"]["pcm"]["activity_intervals"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(result["excerpt_count"], 1);
    let longest =
        result["segmentation"]["native_mute_support"]["gap_decisions"][0]["longest_muted_frames"]
            .as_u64()
            .unwrap();
    assert!((23999..=24001).contains(&longest));
    verify_slices(&artifact, directory.path(), &result)
}
