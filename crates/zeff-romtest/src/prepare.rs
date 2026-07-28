use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use anyhow::Context;

use crate::cli::Cli;
use crate::model::*;
use crate::util::{HashCheck, verify_rom_hash};

#[derive(Debug)]
pub(crate) struct PrepareReport {
    pub(crate) selected_count: usize,
    pub(crate) results: Vec<PrepareResult>,
}

impl PrepareReport {
    pub(crate) fn has_missing_required(&self) -> bool {
        self.results.iter().any(|result| {
            matches!(
                result.status,
                PrepareStatus::Missing | PrepareStatus::HashMismatch
            )
        })
    }
}

#[derive(Debug)]
pub(crate) struct PrepareResult {
    pub(crate) id: String,
    pub(crate) core: Core,
    pub(crate) tier: Tier,
    pub(crate) status: PrepareStatus,
    pub(crate) target: PathBuf,
    pub(crate) source: Option<PathBuf>,
    pub(crate) reason: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PrepareStatus {
    Present,
    Copied,
    Missing,
    HashMismatch,
    Skipped,
    DryRun,
}

pub(crate) fn prepare_tests(tests: &[&LoadedTest], cli: &Cli) -> anyhow::Result<PrepareReport> {
    let mut results = Vec::new();
    for loaded in tests {
        let result = prepare_one(loaded, cli)?;
        println!(
            "{:<13} {:<7} {:<8} {}",
            status_name(result.status),
            result.core,
            result.tier,
            result.id
        );
        results.push(result);
    }

    Ok(PrepareReport {
        selected_count: tests.len(),
        results,
    })
}

fn prepare_one(loaded: &LoadedTest, cli: &Cli) -> anyhow::Result<PrepareResult> {
    let test = &loaded.test;
    let target = test.rom.path.clone();

    if matches!(test.artifact.kind, ArtifactKind::GameRom) {
        return Ok(PrepareResult {
            id: test.id.clone(),
            core: test.core,
            tier: test.tier,
            status: PrepareStatus::Skipped,
            target,
            source: None,
            reason: Some("game_rom entries are not managed by prepare".to_string()),
        });
    }

    if test.expectation.kind == ExpectationKind::Skip {
        return Ok(PrepareResult {
            id: test.id.clone(),
            core: test.core,
            tier: test.tier,
            status: PrepareStatus::Skipped,
            target,
            source: None,
            reason: test.expectation.reason.clone(),
        });
    }

    if target.exists() {
        return match verify_rom_hash(test, &target)? {
            HashCheck::Ok | HashCheck::NoExpectedHash => Ok(PrepareResult {
                id: test.id.clone(),
                core: test.core,
                tier: test.tier,
                status: PrepareStatus::Present,
                target,
                source: None,
                reason: None,
            }),
            HashCheck::Mismatch { expected, actual } => Ok(PrepareResult {
                id: test.id.clone(),
                core: test.core,
                tier: test.tier,
                status: PrepareStatus::HashMismatch,
                target,
                source: None,
                reason: Some(format!(
                    "target sha256 mismatch: expected {expected}, got {actual}"
                )),
            }),
        };
    }

    for legacy_path in &test.rom.legacy_paths {
        if !legacy_path.exists() {
            continue;
        }

        match verify_rom_hash(test, legacy_path)? {
            HashCheck::Ok | HashCheck::NoExpectedHash => {
                if cli.dry_run {
                    return Ok(PrepareResult {
                        id: test.id.clone(),
                        core: test.core,
                        tier: test.tier,
                        status: PrepareStatus::DryRun,
                        target,
                        source: Some(legacy_path.clone()),
                        reason: Some(
                            "would copy legacy local cache to canonical cache".to_string(),
                        ),
                    });
                }

                if let Some(parent) = target.parent()
                    && !parent.as_os_str().is_empty()
                {
                    fs::create_dir_all(parent)
                        .with_context(|| format!("failed to create {}", parent.display()))?;
                }
                fs::copy(legacy_path, &target).with_context(|| {
                    format!(
                        "failed to copy {} to {}",
                        legacy_path.display(),
                        target.display()
                    )
                })?;
                return Ok(PrepareResult {
                    id: test.id.clone(),
                    core: test.core,
                    tier: test.tier,
                    status: PrepareStatus::Copied,
                    target,
                    source: Some(legacy_path.clone()),
                    reason: None,
                });
            }
            HashCheck::Mismatch { expected, actual } => {
                return Ok(PrepareResult {
                    id: test.id.clone(),
                    core: test.core,
                    tier: test.tier,
                    status: PrepareStatus::HashMismatch,
                    target,
                    source: Some(legacy_path.clone()),
                    reason: Some(format!(
                        "legacy source sha256 mismatch: expected {expected}, got {actual}"
                    )),
                });
            }
        }
    }

    Ok(PrepareResult {
        id: test.id.clone(),
        core: test.core,
        tier: test.tier,
        status: if cli.allow_missing {
            PrepareStatus::Skipped
        } else {
            PrepareStatus::Missing
        },
        target,
        source: None,
        reason: Some(acquisition_hint(test)),
    })
}

fn acquisition_hint(test: &TestCase) -> String {
    let mut hint = format!(
        "missing {}; obtain from {}",
        test.rom.path.display(),
        test.artifact
            .source_url
            .as_deref()
            .unwrap_or("the manifest source_url")
    );
    if let Some(version) = &test.artifact.source_version {
        hint.push_str(&format!(" at {version}"));
    }
    if !test.rom.legacy_paths.is_empty() {
        hint.push_str("; local legacy paths checked:");
        for path in &test.rom.legacy_paths {
            hint.push_str(&format!(" {}", path.display()));
        }
    }
    hint
}

pub(crate) fn print_prepare_summary(report: &PrepareReport) {
    let mut counts: BTreeMap<&'static str, usize> = BTreeMap::new();
    for result in &report.results {
        *counts.entry(status_name(result.status)).or_insert(0) += 1;
    }

    println!();
    println!("selected: {}", report.selected_count);
    for (status, count) in counts {
        println!("  {status}: {count}");
    }

    for result in &report.results {
        if matches!(
            result.status,
            PrepareStatus::Missing | PrepareStatus::HashMismatch
        ) && let Some(reason) = &result.reason
        {
            println!("  {}: {reason}", result.id);
        }
    }

    for result in &report.results {
        if matches!(result.status, PrepareStatus::Copied | PrepareStatus::DryRun)
            && let Some(source) = &result.source
        {
            println!(
                "  {}: {} -> {}",
                result.id,
                source.display(),
                result.target.display()
            );
        }
    }
}

fn status_name(status: PrepareStatus) -> &'static str {
    match status {
        PrepareStatus::Present => "present",
        PrepareStatus::Copied => "copied",
        PrepareStatus::Missing => "missing",
        PrepareStatus::HashMismatch => "hash_mismatch",
        PrepareStatus::Skipped => "skipped",
        PrepareStatus::DryRun => "dry_run",
    }
}
