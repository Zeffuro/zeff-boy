use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU32},
};

use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};

use super::{AudioDiscoveryRequest, ScanLimits, SongId, audio_discovery_input::*};
use crate::audio_discovery::{
    capture_artifact::CaptureArtifact, formats::AudioFormat, pcm, preview::PreviewRequest,
    render::RenderOptions, validation,
};

struct Request {
    capture: bool,
    output: PathBuf,
    input: PathBuf,
    member: Option<String>,
    selection: Option<SongId>,
    limit: usize,
    options: RenderOptions,
    wav: Option<PathBuf>,
    reference: Option<PathBuf>,
}

pub(in crate::cli) fn run_if_requested(args: &[OsString]) -> Result<bool> {
    let Some(request) = parse(args)? else {
        return Ok(false);
    };
    run(&request)?;
    Ok(true)
}

fn parse(args: &[OsString]) -> Result<Option<Request>> {
    if !args
        .iter()
        .any(|arg| arg == "--audio-validate" || arg == "--audio-capture-check")
    {
        ensure!(
            !args.iter().any(|arg| arg == "--audio-validation-limit"
                || arg == "--audio-capture-wav"
                || arg == "--audio-capture-reference-f32"),
            "validation options require --audio-validate or --audio-capture-check"
        );
        return Ok(None);
    }
    ensure!(
        args.first()
            .is_some_and(|arg| arg == "--audio-validate" || arg == "--audio-capture-check"),
        "use --audio-validate REPORT INPUT or --audio-capture-check REPORT CAPTURE.zip"
    );
    let mut request = Request {
        capture: args[0] == "--audio-capture-check",
        output: required_path_value(args, 1, "validation requires a new report path")?.into(),
        input: required_path_value(args, 2, "validation requires an input path")?.into(),
        member: None,
        selection: None,
        limit: 32,
        options: RenderOptions {
            max_seconds: 10,
            ..RenderOptions::default()
        },
        wav: None,
        reference: None,
    };
    let mut seen = BTreeSet::new();
    let mut index = 3;
    while index < args.len() {
        let flag = args[index]
            .to_str()
            .context("validation option is not Unicode")?;
        ensure!(seen.insert(flag), "duplicate validation option: {flag}");
        match flag {
            "--archive-member" => {
                request.member = Some(
                    required_path_value(
                        args,
                        index + 1,
                        "--archive-member requires a member path",
                    )?
                    .to_str()
                    .context("member path is not Unicode")?
                    .to_owned(),
                )
            }
            "--audio-song-id" => {
                let value =
                    required_path_value(args, index + 1, "--audio-song-id requires engine:index")?
                        .to_str()
                        .context("song ID is not Unicode")?;
                let (engine, item) = value
                    .split_once(':')
                    .context("--audio-song-id requires engine:index")?;
                request.selection = Some(serde_json::from_value(json!({"engine": engine,
                    "index": item.parse::<usize>().context("invalid song index")?}))?);
            }
            "--audio-validation-limit" => {
                request.limit = required_audio_u32(args, index + 1, flag)? as usize
            }
            "--audio-max-seconds" => {
                request.options.max_seconds = required_audio_u32(args, index + 1, flag)?
                    .try_into()
                    .context("validation duration overflows")?
            }
            "--audio-sample-rate" => {
                request.options.sample_rate = required_audio_u32(args, index + 1, flag)?
            }
            "--audio-capture-wav" => {
                request.wav = Some(
                    required_path_value(
                        args,
                        index + 1,
                        "--audio-capture-wav requires a new WAV path",
                    )?
                    .into(),
                )
            }
            "--audio-capture-reference-f32" => {
                request.reference = Some(
                    required_path_value(
                        args,
                        index + 1,
                        "--audio-capture-reference-f32 requires a native audio dump path",
                    )?
                    .into(),
                );
            }
            _ => anyhow::bail!("unknown validation option: {flag}"),
        }
        index += 2;
    }
    ensure!(
        (1..=256).contains(&request.limit),
        "validation selection limit must be 1..=256"
    );
    ensure!(
        (1..=120).contains(&request.options.max_seconds),
        "validation duration must be 1..=120 seconds"
    );
    pcm::validate_options(request.options)?;
    ensure!(
        !request.capture
            || (request.member.is_none()
                && request.selection.is_none()
                && !seen.contains("--audio-validation-limit")),
        "catalog selection options do not apply to native captures"
    );
    ensure!(
        request.capture || request.wav.is_none(),
        "--audio-capture-wav requires --audio-capture-check"
    );
    ensure!(
        request.reference.is_none() || (request.capture && seen.contains("--audio-sample-rate")),
        "--audio-capture-reference-f32 requires --audio-capture-check and an explicit --audio-sample-rate"
    );
    ensure_distinct_output_path(&request.output, &request.input)?;
    ensure!(!request.output.exists(), "validation output already exists");
    if let Some(wav) = &request.wav {
        ensure_distinct_output_path(wav, &request.input)?;
        ensure_distinct_output_path(wav, &request.output)?;
        ensure!(!wav.exists(), "capture WAV output already exists");
    }
    if let Some(reference) = &request.reference {
        ensure_distinct_output_path(&request.output, reference)?;
        if let Some(wav) = &request.wav {
            ensure_distinct_output_path(wav, reference)?;
        }
    }
    Ok(Some(request))
}

fn run(request: &Request) -> Result<()> {
    let cancel = AtomicBool::new(false);
    let result = if request.capture {
        capture(request, &cancel)
    } else {
        catalog(request, &cancel)
    };
    let (outcome, passed) = match result {
        Ok(value) => value,
        Err(error) => (
            json!({"status": "input_error", "message": format!("{error:#}")}),
            false,
        ),
    };
    let report = json!({
        "schema": "zeff-audio-playback-validation/1",
        "application_version": env!("CARGO_PKG_VERSION"),
        "mode": if request.capture { "native_capture" } else { "catalog" },
        "input": request.input, "archive_member": request.member,
        "native_reference_f32": request.reference,
        "render_options": request.options, "selection_limit": request.limit,
        "outcome": outcome,
        "limitations": [
            "PCM hashes compare stereo signed 16-bit little-endian samples at the reported settings.",
            "Fresh and reset renders use different chunk sizes. Determinism and audible output do not establish source or hardware fidelity.",
            "Activity intervals use a fixed amplitude threshold and 750 ms gap; they are not song or loop boundaries.",
            "Identical rendered intervals do not establish that two selections are the same song. Silent output can be a valid captured interval or an untriggered selection.",
            "Native references use the explicitly requested sample rate and compare the entire f32 file after the same signed-16-bit projection; a match does not establish f32 or hardware equivalence.",
        ],
    });
    let bytes = serde_json::to_vec_pretty(&report)?;
    crate::platform::write_new_file_atomically_validated(&request.output, &bytes, |file| {
        use std::io::Seek;
        file.rewind()?;
        let _: Value = serde_json::from_reader(file)?;
        Ok(())
    })?;
    println!(
        "[audio-validation] passed={passed} wrote={}",
        request.output.display()
    );
    ensure!(
        passed,
        "audio validation recorded failures; see {}",
        request.output.display()
    );
    Ok(())
}

fn catalog(request: &Request, cancel: &AtomicBool) -> Result<(Value, bool)> {
    let input = Arc::new(load_input(&AudioDiscoveryRequest {
        output_path: request.output.clone(),
        input_path: request.input.clone(),
        archive_member: request.member.clone(),
        max_work: None,
        max_candidates: None,
        export: None,
        relations: None,
        driver_evidence: None,
    })?);
    let mut manifest = input.analyze(ScanLimits::default(), cancel);
    manifest.load_roles()?;
    let mut rows = Vec::new();
    let mut attempted = 0;
    let mut failed = 0;
    let mut unexamined = 0;
    let mut unsupported = 0;
    let mut hashes: BTreeMap<(u32, usize, String), Vec<SongId>> = BTreeMap::new();
    if let Some(id) = request.selection {
        ensure!(
            manifest.scan.song(id).is_some(),
            "selected catalog ID does not exist"
        );
    }
    for finding in manifest
        .scan
        .catalog()
        .filter(|finding| request.selection.is_none_or(|id| id == finding.id))
    {
        let id = finding.id;
        let playback = if !PreviewRequest::can_preview(&manifest, id) {
            unsupported += 1;
            json!({"status": "unsupported"})
        } else if attempted == request.limit {
            unexamined += 1;
            json!({"status": "not_examined", "reason": "selection_limit"})
        } else {
            attempted += 1;
            let evidence = validation::validate(
                || {
                    Ok(Box::new(
                        PreviewRequest::prepare_song(&input, &manifest, id, request.options)?
                            .renderer(request.options.sample_rate, cancel)?,
                    ))
                },
                frame_limit(request),
                cancel,
            );
            match evidence {
                Ok(evidence) => {
                    if evidence.deterministic() {
                        hashes
                            .entry((
                                evidence.pcm.sample_rate,
                                evidence.pcm.frames,
                                evidence.pcm.pcm_sha256.clone(),
                            ))
                            .or_default()
                            .push(id);
                    } else {
                        failed += 1;
                    }
                    json!({"status": if evidence.deterministic() { "rendered" } else { "nondeterministic" }, "evidence": evidence})
                }
                Err(error) => {
                    failed += 1;
                    json!({"status": "render_error", "message": format!("{error:#}")})
                }
            }
        };
        rows.push(json!({"finding": finding, "playback": playback}));
    }
    let duplicates: Vec<_> = hashes
        .into_iter()
        .filter(|(_, ids)| ids.len() > 1)
        .map(|((rate, frames, hash), ids)| {
            json!({"sample_rate": rate, "frames": frames,
            "pcm_sha256": hash, "selections": ids})
        })
        .collect();
    let passed = failed == 0 && attempted > 0;
    Ok((
        json!({"status": "scanned", "source": manifest.source,
        "analysis_profile": manifest.analysis_profile, "scan_status": manifest.scan.status,
        "attempted": attempted, "failed": failed, "unsupported": unsupported,
        "unexamined": unexamined, "rows": rows, "identical_rendered_intervals": duplicates}),
        passed,
    ))
}

fn capture(request: &Request, cancel: &AtomicBool) -> Result<(Value, bool)> {
    let artifact = CaptureArtifact::load(&request.input)?;
    let full_options = RenderOptions {
        max_seconds: request.options.max_seconds + 1,
        ..request.options
    };
    let result = validation::validate(
        || artifact.session(full_options, cancel),
        frame_limit(request),
        cancel,
    );
    let mut reference = Value::Null;
    let mut reference_passed = true;
    let (playback, passed) = match result {
        Ok(evidence) => {
            if let Some(path) = &request.reference {
                match validation::reference::compare_f32_file(path, &evidence.pcm, cancel) {
                    Ok(comparison) => {
                        reference_passed = comparison.projected_pcm_matches;
                        reference = json!({"status": if reference_passed { "matched" } else { "mismatch" },
                            "evidence": comparison});
                    }
                    Err(error) => {
                        reference_passed = false;
                        reference =
                            json!({"status": "reference_error", "message": format!("{error:#}")});
                    }
                }
            }
            let passed = evidence.deterministic();
            (
                json!({"status": if passed { "rendered" } else { "nondeterministic" }, "evidence": evidence}),
                passed,
            )
        }
        Err(error) => {
            if request.reference.is_some() {
                reference_passed = false;
                reference =
                    json!({"status": "not_compared", "reason": "playback_validation_failed"});
            }
            (
                json!({"status": "render_error", "message": format!("{error:#}")}),
                false,
            )
        }
    };
    let passed = passed && reference_passed;
    let writers = if passed {
        artifact.writer_evidence(request.options, cancel)?
    } else {
        Value::Null
    };
    let mut export = Value::Null;
    let mut export_passed = true;
    if let Some(path) = &request.wav {
        if passed {
            let result = artifact.session(request.options, cancel).and_then(|session| pcm::write_new(
                session, AudioFormat::Wav, request.options,
                json!({"schema": "zeff-audio-native-capture-render/1", "archive_sha256": artifact.archive_sha256,
                    "trace_sha256": artifact.trace_sha256, "capture_manifest": artifact.manifest}),
                path, cancel, &AtomicU32::new(0)));
            match result {
                Ok(()) => export = json!({"status": "written", "path": path}),
                Err(error) => {
                    export_passed = false;
                    export = json!({"status": "export_error", "message": format!("{error:#}")});
                }
            }
        } else {
            export = json!({"status": "not_written", "reason": "playback_validation_failed"});
        }
    }
    Ok((
        json!({"status": "integrity_verified", "archive_sha256": artifact.archive_sha256,
        "trace_sha256": artifact.trace_sha256, "capture_manifest": artifact.manifest,
        "playback": playback, "native_reference": reference, "wav_export": export,
        "writer_evidence": writers}),
        passed && export_passed,
    ))
}

fn frame_limit(request: &Request) -> usize {
    usize::from(request.options.max_seconds) * request.options.sample_rate as usize
}

#[cfg(test)]
#[path = "audio_discovery_validation_tests.rs"]
mod tests;
