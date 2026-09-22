use std::collections::BTreeSet;
use std::ffi::OsString;
use std::io::{Read, Seek};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use anyhow::{Context, ensure};
use serde::{Deserialize, Serialize};
use zeff_audio_discovery::coverage::{Observation, observe};

use super::audio_discovery_input::{ensure_distinct_output_path, load_input, sha256_hex};
use super::{AudioDiscoveryRequest, ScanLimits};

#[path = "audio_discovery_corpus_report.rs"]
mod report;
use report::{Comparison, Summary};

#[path = "audio_discovery_corpus_input.rs"]
mod input_failure;

const SCHEMA: &str = "zeff-audio-corpus/1";
const MAX_INPUTS: usize = 100_000;
const MAX_JSON_BYTES: u64 = 256 * 1024 * 1024;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct CorpusInput {
    id: String,
    path: PathBuf,
    #[serde(default)]
    archive_member: Option<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Limits {
    max_work: u64,
    max_candidates: u32,
}

impl Default for Limits {
    fn default() -> Self {
        let limits = ScanLimits::default();
        Self {
            max_work: limits.max_work,
            max_candidates: limits.max_candidates,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct InputManifest {
    schema: String,
    #[serde(default)]
    limits: Limits,
    inputs: Vec<CorpusInput>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct Row {
    input: CorpusInput,
    #[serde(flatten)]
    result: InputResult,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum InputResult {
    Scanned {
        analysis_profile: String,
        scan_sha256: String,
        observation: Box<Observation>,
    },
    InputError {
        reason: String,
        message: String,
    },
}

#[derive(Debug, Deserialize, Serialize)]
struct CorpusReport {
    schema: String,
    application_version: String,
    limits: Limits,
    summary: Summary,
    comparison: Option<Comparison>,
    rows: Vec<Row>,
}

pub(super) fn run_if_requested(args: &[OsString]) -> anyhow::Result<bool> {
    let requested = args.iter().any(|arg| arg == "--audio-corpus");
    ensure!(
        requested || !args.iter().any(|arg| arg == "--audio-corpus-baseline"),
        "--audio-corpus-baseline requires --audio-corpus"
    );
    if !requested {
        return Ok(false);
    }
    ensure!(
        matches!(args.len(), 3 | 5) && args[0] == "--audio-corpus",
        "use --audio-corpus REPORT.json INPUTS.json [--audio-corpus-baseline OLD.json]"
    );
    let output = Path::new(super::required_path_value(
        args,
        1,
        "missing corpus output",
    )?);
    let manifest = Path::new(super::required_path_value(
        args,
        2,
        "missing corpus inputs",
    )?);
    let baseline = if args.len() == 5 {
        ensure!(
            args[3] == "--audio-corpus-baseline",
            "unexpected corpus option"
        );
        Some(Path::new(super::required_path_value(
            args,
            4,
            "missing corpus baseline",
        )?))
    } else {
        None
    };
    run(output, manifest, baseline)?;
    Ok(true)
}

fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> anyhow::Result<T> {
    let file =
        std::fs::File::open(path).with_context(|| format!("could not open {}", path.display()))?;
    ensure!(
        file.metadata()?.len() <= MAX_JSON_BYTES,
        "corpus JSON exceeds size limit"
    );
    let mut bytes = Vec::new();
    file.take(MAX_JSON_BYTES + 1).read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() as u64 <= MAX_JSON_BYTES,
        "corpus JSON exceeds size limit"
    );
    serde_json::from_slice(&bytes)
        .with_context(|| format!("invalid corpus JSON: {}", path.display()))
}

fn run(output: &Path, manifest_path: &Path, baseline_path: Option<&Path>) -> anyhow::Result<()> {
    ensure_distinct_output_path(output, manifest_path)?;
    ensure!(!output.try_exists()?, "corpus output already exists");
    let mut manifest: InputManifest = read_json(manifest_path)?;
    ensure!(
        manifest.schema == "zeff-audio-corpus-inputs/1",
        "unsupported corpus input schema"
    );
    validate_limits(manifest.limits)?;
    validate_inputs(manifest.inputs.iter().map(|input| input.id.as_str()))?;
    let baseline: Option<CorpusReport> = baseline_path
        .map(|path| {
            ensure_distinct_output_path(output, path)?;
            let report: CorpusReport = read_json(path)?;
            ensure!(
                report.schema == SCHEMA,
                "unsupported corpus baseline schema"
            );
            validate_limits(report.limits)?;
            validate_inputs(report.rows.iter().map(|row| row.input.id.as_str()))?;
            Ok::<_, anyhow::Error>(report)
        })
        .transpose()?;
    let parent = manifest_path.parent().unwrap_or(Path::new("."));
    for input in &mut manifest.inputs {
        if input.path.is_relative() {
            input.path = parent.join(&input.path);
        }
        ensure_distinct_output_path(output, &input.path)?;
    }
    manifest.inputs.sort_by(|a, b| a.id.cmp(&b.id));
    let rows = scan_inputs(&manifest.inputs, output, manifest.limits)?;
    let summary = Summary::from_rows(&rows);
    let comparison = baseline
        .as_ref()
        .map(|old| Comparison::between(old, &rows, manifest.limits));
    let report = CorpusReport {
        schema: SCHEMA.to_owned(),
        application_version: env!("CARGO_PKG_VERSION").to_owned(),
        limits: manifest.limits,
        summary,
        comparison,
        rows,
    };
    let bytes = serde_json::to_vec_pretty(&report)?;
    ensure!(
        bytes.len() as u64 <= MAX_JSON_BYTES,
        "corpus report exceeds size limit"
    );
    crate::platform::write_new_file_atomically_validated(output, &bytes, |file| {
        file.rewind()?;
        let _: CorpusReport = serde_json::from_reader(file)?;
        Ok(())
    })?;
    println!(
        "[audio-corpus] inputs={} wrote={}",
        report.rows.len(),
        output.display()
    );
    Ok(())
}

fn scan_inputs(inputs: &[CorpusInput], output: &Path, limits: Limits) -> anyhow::Result<Vec<Row>> {
    let next = AtomicUsize::new(0);
    let completed = AtomicUsize::new(0);
    let workers = std::thread::available_parallelism()
        .map_or(1, usize::from)
        .min(4)
        .min(inputs.len());
    let mut rows = std::thread::scope(|scope| {
        let handles = (0..workers)
            .map(|_| {
                scope.spawn(|| {
                    let mut rows = Vec::new();
                    while let Some(input) = inputs.get(next.fetch_add(1, Ordering::Relaxed)) {
                        let result = scan_input(input, output, limits);
                        rows.push(Row {
                            input: input.clone(),
                            result,
                        });
                        let count = completed.fetch_add(1, Ordering::Relaxed) + 1;
                        if count.is_multiple_of(100) {
                            println!("[audio-corpus] scanned={count}");
                        }
                    }
                    rows
                })
            })
            .collect::<Vec<_>>();
        let mut rows = Vec::with_capacity(inputs.len());
        for handle in handles {
            rows.extend(
                handle
                    .join()
                    .map_err(|_| anyhow::anyhow!("corpus scanner panicked"))?,
            );
        }
        Ok::<_, anyhow::Error>(rows)
    })?;
    rows.sort_by(|a, b| a.input.id.cmp(&b.input.id));
    Ok(rows)
}

fn validate_limits(limits: Limits) -> anyhow::Result<()> {
    ensure!(
        limits.max_work <= zeff_audio_discovery::MAX_SCAN_WORK,
        "invalid corpus work limit"
    );
    ensure!(
        (1..=zeff_audio_discovery::MAX_CANDIDATES).contains(&limits.max_candidates),
        "invalid corpus candidate limit"
    );
    Ok(())
}

fn validate_inputs<'a>(ids: impl Iterator<Item = &'a str>) -> anyhow::Result<()> {
    let mut seen = BTreeSet::new();
    for id in ids {
        ensure!(
            !id.trim().is_empty() && id.len() <= 1024,
            "invalid corpus input id"
        );
        ensure!(seen.insert(id), "duplicate corpus input id: {id}");
        ensure!(seen.len() <= MAX_INPUTS, "too many corpus inputs");
    }
    ensure!(!seen.is_empty(), "corpus inputs are empty");
    Ok(())
}

fn scan_input(input: &CorpusInput, output: &Path, limits: Limits) -> InputResult {
    let archive_member = match input
        .archive_member
        .as_deref()
        .map(super::normalize_archive_member)
        .transpose()
    {
        Ok(member) => member,
        Err(error) => {
            return InputResult::InputError {
                reason: "invalid_archive_selection".to_owned(),
                message: format!("{error:#}"),
            };
        }
    };
    let request = AudioDiscoveryRequest {
        output_path: output.to_owned(),
        input_path: input.path.clone(),
        archive_member,
        max_work: Some(limits.max_work),
        max_candidates: Some(limits.max_candidates),
        export: None,
        relations: None,
        driver_evidence: None,
    };
    let loaded = match load_input(&request) {
        Ok(loaded) => loaded,
        Err(error) => {
            return InputResult::InputError {
                reason: input_failure::failure_reason(input, &error).to_owned(),
                message: format!("{error:#}"),
            };
        }
    };
    let scan = || -> anyhow::Result<InputResult> {
        let manifest = loaded.analyze(
            ScanLimits {
                max_work: limits.max_work,
                max_candidates: limits.max_candidates,
            },
            &AtomicBool::new(false),
        );
        Ok(InputResult::Scanned {
            analysis_profile: manifest.analysis_profile.to_owned(),
            scan_sha256: sha256_hex(&serde_json::to_vec(&serde_json::to_value(&manifest.scan)?)?),
            observation: Box::new(observe(&manifest.scan)),
        })
    };
    scan().unwrap_or_else(|error| InputResult::InputError {
        reason: "report_encoding_failed".to_owned(),
        message: format!("{error:#}"),
    })
}

#[cfg(test)]
#[path = "audio_discovery_corpus_tests.rs"]
mod tests;
