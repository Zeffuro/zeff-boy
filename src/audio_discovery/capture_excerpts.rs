use std::collections::BTreeMap;
use std::fs::File;
use std::io::{BufWriter, Read, Seek, SeekFrom, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use super::audio_file::{self, AudioInfo};
use super::capture_artifact::CaptureArtifact;
use super::formats::AudioFormat;
use super::render::RenderOptions;
use super::validation::{self, PlaybackEvidence};
use crate::platform::StableDirectory;

const MAX_SECONDS: usize = 120;

pub(crate) fn extract(
    artifact: &CaptureArtifact,
    directory: &StableDirectory,
    sample_rate: u32,
    reference: Option<&Path>,
    cancel: &AtomicBool,
) -> Result<Value> {
    let options = RenderOptions {
        sample_rate,
        max_seconds: MAX_SECONDS as u16 + 1,
        fade_seconds: 0,
        ..Default::default()
    };
    let max_frames = sample_rate as usize * MAX_SECONDS;
    ensure!(
        artifact.session(options, cancel)?.duration_frames() <= max_frames,
        "excerpt acquisition requires the complete capture within 120 seconds"
    );
    let evidence = validation::validate(|| artifact.session(options, cancel), max_frames, cancel)?;
    admit(&evidence)?;
    let writers = artifact.writer_evidence(options, cancel)?;
    let reference = reference
        .map(|path| validation::reference::compare_f32_file(path, &evidence.pcm, cancel))
        .transpose()?;
    ensure!(
        reference
            .as_ref()
            .is_none_or(|evidence| evidence.projected_pcm_matches),
        "native reference does not match the entire captured interval"
    );

    let mut spool = tempfile::tempfile()?;
    let replay = {
        let mut writer = BufWriter::new(&mut spool);
        let mut encoded = [0; 4096];
        let replay = validation::render_into(
            &mut *artifact.session(options, cancel)?,
            max_frames,
            1024,
            cancel,
            |samples| {
                for (index, sample) in samples.iter().enumerate() {
                    encoded[index * 2..index * 2 + 2].copy_from_slice(&sample.to_le_bytes());
                }
                writer.write_all(&encoded[..samples.len() * 2])?;
                Ok(())
            },
        )?;
        writer.flush()?;
        replay
    };
    ensure!(
        replay == evidence.pcm,
        "excerpt replay differs from validated PCM"
    );
    let selection = artifact.excerpt_intervals(options, &evidence, cancel)?;
    let mut segmentation = segmentation(&evidence);
    let intervals = if let Some((intervals, support)) = &selection {
        segmentation["method"] = support["method"].clone();
        segmentation["native_mute_support"] = support.clone();
        intervals
    } else {
        &evidence.pcm.activity_intervals
    };
    let mut excerpts = Vec::new();
    for (index, interval) in intervals.iter().enumerate() {
        check_cancel(cancel)?;
        directory.revalidate()?;
        let previous_end = index
            .checked_sub(1)
            .map_or(0, |index| intervals[index].end_frame);
        let next_start = intervals
            .get(index + 1)
            .map_or(evidence.pcm.frames, |next| next.start_frame);
        let mut row = json!({
            "index": index,
            "file": format!("excerpt-{index:03}.wav"),
            "start_frame": interval.start_frame,
            "end_frame": interval.end_frame,
            "frames": interval.end_frame - interval.start_frame,
            "preceding_quiet_frames": interval.start_frame - previous_end,
            "following_quiet_frames": next_start - interval.end_frame,
            "starts_at_capture_edge": interval.start_frame == 0,
            "ends_at_capture_edge": interval.end_frame == evidence.pcm.frames,
        });
        ensure!(
            interval.end_frame <= evidence.pcm.frames,
            "excerpt exceeds captured PCM"
        );
        let (mut pcm, hash) = copy_interval(&mut spool, interval, cancel)?;
        row["pcm_sha256"] = json!(hash);
        let metadata = json!({
            "schema": "zeff-audio-capture-excerpt/1",
            "archive_sha256": artifact.archive_sha256,
            "trace_sha256": artifact.trace_sha256,
            "capture_manifest": artifact.manifest,
            "source_pcm": {"sha256": evidence.pcm.pcm_sha256, "frames": evidence.pcm.frames,
                "sample_rate": sample_rate},
            "selection": row,
            "qualification": "replay_verified_excerpt",
            "segmentation": segmentation,
        });
        let info = AudioInfo {
            frames: (interval.end_frame - interval.start_frame) as u64,
            channels: 2,
            sample_rate,
            loop_range: None,
            pitch: None,
        };
        let metadata = serde_json::to_vec(&metadata)?;
        let path = directory.path().join(format!("excerpt-{index:03}.wav"));
        crate::platform::write_new_file_atomically_streamed(
            &path,
            |file| {
                audio_file::encode_to(AudioFormat::Wav, &mut pcm, info, &metadata, cancel, file)?;
                verify_wav(file, info, &hash, cancel)
            },
            || {
                check_cancel(cancel)?;
                directory.revalidate()
            },
        )?;
        row["wav_pcm_verified"] = json!(true);
        excerpts.push(row);
    }
    directory.revalidate()?;
    check_cancel(cancel)?;
    Ok(json!({
        "status": "complete",
        "qualification": "replay_verified_excerpts",
        "archive_sha256": artifact.archive_sha256,
        "trace_sha256": artifact.trace_sha256,
        "capture_manifest": artifact.manifest,
        "segmentation": segmentation,
        "writer_evidence": writers,
        "source_playback": evidence,
        "native_reference": reference,
        "excerpt_count": excerpts.len(),
        "identical_excerpt_intervals": duplicates(&excerpts, sample_rate),
        "excerpts": excerpts,
    }))
}

fn admit(evidence: &PlaybackEvidence) -> Result<()> {
    ensure!(
        evidence.deterministic(),
        "capture replay is nondeterministic"
    );
    ensure!(
        !evidence.validation_duration_capped,
        "excerpt acquisition requires the complete capture within 120 seconds"
    );
    ensure!(
        !evidence.pcm.activity_intervals_truncated,
        "activity evidence exceeds the excerpt limit"
    );
    Ok(())
}

fn segmentation(evidence: &PlaybackEvidence) -> Value {
    json!({
        "method": "amplitude_activity_gap",
        "silence_threshold_i16": evidence.silence_threshold_i16,
        "activity_gap_frames": evidence.activity_gap_frames,
        "sample_rate": evidence.pcm.sample_rate,
        "frame_ranges": "half_open_stereo_pcm",
        "padding_frames": 0,
        "maximum_capture_seconds": MAX_SECONDS,
        "limitations": "Activity excerpts can contain music, effects, or mixtures. Quiet gaps, capture edges and identical PCM do not identify songs, natural ends, loops, or causal input changes.",
    })
}

fn copy_interval(
    source: &mut File,
    interval: &validation::Interval,
    cancel: &AtomicBool,
) -> Result<(File, String)> {
    ensure!(
        interval.start_frame < interval.end_frame,
        "empty excerpt interval"
    );
    let start = u64::try_from(interval.start_frame)?
        .checked_mul(4)
        .context("excerpt offset overflows")?;
    let end = u64::try_from(interval.end_frame)?
        .checked_mul(4)
        .context("excerpt end overflows")?;
    ensure!(
        end <= source.metadata()?.len(),
        "excerpt exceeds PCM storage"
    );
    source.seek(SeekFrom::Start(start))?;
    let mut output = tempfile::tempfile()?;
    let mut remaining = end - start;
    let mut buffer = [0; 8192];
    let mut hash = Sha256::new();
    while remaining > 0 {
        check_cancel(cancel)?;
        let count = remaining.min(buffer.len() as u64) as usize;
        source.read_exact(&mut buffer[..count])?;
        output.write_all(&buffer[..count])?;
        hash.update(&buffer[..count]);
        remaining -= count as u64;
    }
    Ok((output, const_hex::encode(hash.finalize())))
}

fn verify_wav(file: &mut File, info: AudioInfo, hash: &str, cancel: &AtomicBool) -> Result<()> {
    file.rewind()?;
    let mut reader = hound::WavReader::new(file)?;
    let spec = reader.spec();
    ensure!(
        spec.channels == 2
            && spec.sample_rate == info.sample_rate
            && spec.bits_per_sample == 16
            && spec.sample_format == hound::SampleFormat::Int
            && u64::from(reader.duration()) == info.frames,
        "encoded excerpt WAV settings differ from the selected interval"
    );
    let mut actual = Sha256::new();
    let mut count = 0_u64;
    for sample in reader.samples::<i16>() {
        if count.is_multiple_of(2048) {
            check_cancel(cancel)?;
        }
        actual.update(sample?.to_le_bytes());
        count += 1;
    }
    ensure!(
        count == info.frames * 2 && const_hex::encode(actual.finalize()) == hash,
        "encoded excerpt WAV differs from the selected PCM"
    );
    Ok(())
}

fn duplicates(excerpts: &[Value], sample_rate: u32) -> Vec<Value> {
    let mut groups = BTreeMap::<(u64, String), Vec<usize>>::new();
    for (index, excerpt) in excerpts.iter().enumerate() {
        let frames = excerpt["frames"].as_u64().expect("excerpt frames");
        let hash = excerpt["pcm_sha256"].as_str().expect("excerpt hash");
        groups
            .entry((frames, hash.to_owned()))
            .or_default()
            .push(index);
    }
    groups
        .into_iter()
        .filter(|(_, indices)| indices.len() > 1)
        .map(|((frames, hash), indices)| {
            json!({"sample_rate": sample_rate, "frames": frames,
            "pcm_sha256": hash, "excerpt_indices": indices})
        })
        .collect()
}

fn check_cancel(cancel: &AtomicBool) -> Result<()> {
    ensure!(
        !cancel.load(Ordering::Relaxed),
        "excerpt extraction cancelled"
    );
    Ok(())
}

#[cfg(test)]
mod sn_tests;
#[cfg(test)]
mod tests;
