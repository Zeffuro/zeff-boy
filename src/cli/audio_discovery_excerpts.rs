use std::collections::BTreeSet;
use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;

use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};

use super::audio_discovery_input::{
    ensure_distinct_output_path, ensure_outside_output_directory, required_audio_u32,
    required_path_value,
};
use crate::audio_discovery::{capture_artifact::CaptureArtifact, capture_excerpts};

struct Request {
    output: PathBuf,
    input: PathBuf,
    sample_rate: u32,
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
    if !args.iter().any(|arg| arg == "--audio-capture-excerpts") {
        return Ok(None);
    }
    ensure!(
        args.first()
            .is_some_and(|arg| arg == "--audio-capture-excerpts"),
        "use --audio-capture-excerpts NEW_DIRECTORY CAPTURE.zip --audio-sample-rate RATE"
    );
    let output = required_path_value(args, 1, "excerpts require a new output directory")?.into();
    let input = required_path_value(args, 2, "excerpts require an input capture")?.into();
    let mut request = Request {
        output,
        input,
        sample_rate: 0,
        reference: None,
    };
    let mut seen = BTreeSet::new();
    let mut index = 3;
    while index < args.len() {
        let flag = args[index]
            .to_str()
            .context("excerpt options must be valid Unicode")?;
        ensure!(seen.insert(flag), "duplicate excerpt option: {flag}");
        match flag {
            "--audio-sample-rate" => {
                request.sample_rate = required_audio_u32(args, index + 1, flag)?
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
            _ => anyhow::bail!("unsupported excerpt option: {flag}"),
        }
        index += 2;
    }
    ensure!(
        seen.contains("--audio-sample-rate"),
        "excerpt extraction requires an explicit sample rate"
    );
    crate::audio_discovery::render::validate_sample_rate(request.sample_rate)?;
    ensure_distinct_output_path(&request.output, &request.input)?;
    ensure_outside_output_directory(&request.input, &request.output)?;
    if let Some(reference) = &request.reference {
        ensure_distinct_output_path(&request.output, reference)?;
        ensure_outside_output_directory(reference, &request.output)?;
    }
    ensure!(
        !request.output.exists(),
        "excerpt output directory already exists"
    );
    Ok(Some(request))
}

fn run(request: &Request) -> Result<()> {
    if let Some(parent) = request
        .output
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
    {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::create_dir(&request.output)
        .context("could not reserve new excerpt output directory")?;
    let directory =
        crate::platform::StableDirectory::open_or_create(&request.output, "audio excerpts")?;
    let cancel = AtomicBool::new(false);
    let outcome = CaptureArtifact::load(&request.input).and_then(|artifact| {
        capture_excerpts::extract(
            &artifact,
            &directory,
            request.sample_rate,
            request.reference.as_deref(),
            &cancel,
        )
    });
    let (outcome, passed) = match outcome {
        Ok(outcome) => (outcome, true),
        Err(error) => (
            json!({"status": "failed", "message": format!("{error:#}"),
            "publication": "Any retained excerpt files are partial output; extraction did not complete."}),
            false,
        ),
    };
    let report = json!({
        "schema": "zeff-audio-capture-excerpts/1",
        "application_version": env!("CARGO_PKG_VERSION"),
        "input": request.input,
        "sample_rate": request.sample_rate,
        "native_reference_f32": request.reference,
        "outcome": outcome,
    });
    directory.revalidate()?;
    let bytes = serde_json::to_vec_pretty(&report)?;
    crate::platform::write_new_file_atomically_validated(
        &directory.path().join("report.json"),
        &bytes,
        |file| {
            use std::io::Seek;
            directory.revalidate()?;
            file.rewind()?;
            let _: Value = serde_json::from_reader(file)?;
            Ok(())
        },
    )?;
    println!(
        "[audio-excerpts] passed={passed} wrote={}",
        directory.path().display()
    );
    ensure!(
        passed,
        "excerpt extraction failed; see {}",
        directory.path().display()
    );
    Ok(())
}

#[cfg(test)]
#[path = "audio_discovery_excerpts_tests.rs"]
mod tests;
