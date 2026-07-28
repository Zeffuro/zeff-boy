use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use anyhow::{Context, bail};
use serde::{Deserialize, Serialize};

use crate::model::{Core, ExpectationKind, Tier};
use crate::runner::{RunReport, TestStatus, test_status_name};
use crate::util::ensure_parent_dir;

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct BaselineReport {
    pub(crate) schema_version: u32,
    pub(crate) selected_count: usize,
    pub(crate) results: Vec<BaselineResult>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct BaselineResult {
    pub(crate) id: String,
    #[serde(default)]
    pub(crate) suite_id: String,
    pub(crate) core: Core,
    pub(crate) tier: Tier,
    pub(crate) expectation: ExpectationKind,
    pub(crate) status: TestStatus,
}

impl From<&RunReport> for BaselineReport {
    fn from(report: &RunReport) -> Self {
        Self {
            schema_version: 1,
            selected_count: report.selected_count,
            results: report
                .results
                .iter()
                .map(|result| BaselineResult {
                    id: result.id.clone(),
                    suite_id: result.suite_id.clone(),
                    core: result.core,
                    tier: result.tier,
                    expectation: result.expectation,
                    status: result.status,
                })
                .collect(),
        }
    }
}

pub(crate) fn write_baseline_report(path: &Path, report: &RunReport) -> anyhow::Result<()> {
    ensure_parent_dir(path)?;
    let baseline = BaselineReport::from(report);
    let json = serde_json::to_string_pretty(&baseline)?;
    fs::write(path, json).with_context(|| format!("failed to write {}", path.display()))?;
    println!("baseline-report: {}", path.display());
    Ok(())
}

pub(crate) fn read_baseline_report(path: &Path) -> anyhow::Result<BaselineReport> {
    let text =
        fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?;
    let report: BaselineReport = serde_json::from_str(&text)
        .with_context(|| format!("failed to parse {}", path.display()))?;
    if report.schema_version != 1 {
        bail!(
            "{} uses unsupported baseline schema_version {}",
            path.display(),
            report.schema_version
        );
    }
    if report.selected_count != report.results.len() {
        bail!(
            "{} selected_count {} does not match result count {}",
            path.display(),
            report.selected_count,
            report.results.len()
        );
    }
    Ok(report)
}

#[derive(Debug)]
pub(crate) struct BaselineCompareReport {
    pub(crate) baseline_count: usize,
    pub(crate) actual_count: usize,
    pub(crate) diffs: Vec<BaselineDiff>,
}

impl BaselineCompareReport {
    pub(crate) fn has_differences(&self) -> bool {
        !self.diffs.is_empty()
    }
}

#[derive(Debug)]
pub(crate) struct BaselineDiff {
    pub(crate) kind: BaselineDiffKind,
    pub(crate) id: String,
    pub(crate) baseline_status: Option<TestStatus>,
    pub(crate) actual_status: Option<TestStatus>,
    pub(crate) detail: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum BaselineDiffKind {
    StatusChanged,
    MetadataChanged,
    MissingInActual,
    NewInActual,
}

pub(crate) fn compare_baseline(
    baseline: &BaselineReport,
    actual: &RunReport,
) -> anyhow::Result<BaselineCompareReport> {
    let mut baseline_by_id = BTreeMap::new();
    for result in &baseline.results {
        if baseline_by_id.insert(result.id.as_str(), result).is_some() {
            bail!("baseline contains duplicate test id '{}'", result.id);
        }
    }

    let mut actual_by_id = BTreeMap::new();
    for result in &actual.results {
        if actual_by_id.insert(result.id.as_str(), result).is_some() {
            bail!("actual report contains duplicate test id '{}'", result.id);
        }
    }

    let mut diffs = Vec::new();
    for (id, baseline_result) in &baseline_by_id {
        let Some(actual_result) = actual_by_id.get(id) else {
            diffs.push(BaselineDiff {
                kind: BaselineDiffKind::MissingInActual,
                id: (*id).to_string(),
                baseline_status: Some(baseline_result.status),
                actual_status: None,
                detail: "test exists in baseline but not actual report".to_string(),
            });
            continue;
        };

        if baseline_result.suite_id != actual_result.suite_id
            || baseline_result.core != actual_result.core
            || baseline_result.tier != actual_result.tier
            || baseline_result.expectation != actual_result.expectation
        {
            diffs.push(BaselineDiff {
                kind: BaselineDiffKind::MetadataChanged,
                id: (*id).to_string(),
                baseline_status: Some(baseline_result.status),
                actual_status: Some(actual_result.status),
                detail: format!(
                    "metadata changed: baseline=({}/{}/{}/{}) actual=({}/{}/{}/{})",
                    baseline_result.suite_id,
                    baseline_result.core,
                    baseline_result.tier,
                    baseline_result.expectation,
                    actual_result.suite_id,
                    actual_result.core,
                    actual_result.tier,
                    actual_result.expectation
                ),
            });
        }

        if baseline_result.status != actual_result.status {
            diffs.push(BaselineDiff {
                kind: BaselineDiffKind::StatusChanged,
                id: (*id).to_string(),
                baseline_status: Some(baseline_result.status),
                actual_status: Some(actual_result.status),
                detail: "status changed".to_string(),
            });
        }
    }

    for (id, actual_result) in &actual_by_id {
        if !baseline_by_id.contains_key(id) {
            diffs.push(BaselineDiff {
                kind: BaselineDiffKind::NewInActual,
                id: (*id).to_string(),
                baseline_status: None,
                actual_status: Some(actual_result.status),
                detail: "test exists in actual report but not baseline".to_string(),
            });
        }
    }

    diffs.sort_by(|a, b| a.kind.cmp(&b.kind).then_with(|| a.id.cmp(&b.id)));

    Ok(BaselineCompareReport {
        baseline_count: baseline.results.len(),
        actual_count: actual.results.len(),
        diffs,
    })
}

pub(crate) fn print_baseline_compare_summary(report: &BaselineCompareReport) {
    println!(
        "baseline tests: {}, actual tests: {}",
        report.baseline_count, report.actual_count
    );
    if report.diffs.is_empty() {
        println!("baseline comparison passed");
        return;
    }

    let mut counts: BTreeMap<&'static str, usize> = BTreeMap::new();
    for diff in &report.diffs {
        *counts
            .entry(baseline_diff_kind_name(diff.kind))
            .or_insert(0) += 1;
    }

    println!(
        "baseline comparison failed: {} difference(s)",
        report.diffs.len()
    );
    for (kind, count) in counts {
        println!("  {kind}: {count}");
    }
    for diff in &report.diffs {
        let baseline_status = diff.baseline_status.map(test_status_name).unwrap_or("-");
        let actual_status = diff.actual_status.map(test_status_name).unwrap_or("-");
        println!(
            "  {} {}: baseline={} actual={} {}",
            baseline_diff_kind_name(diff.kind),
            diff.id,
            baseline_status,
            actual_status,
            diff.detail
        );
    }
}

fn baseline_diff_kind_name(kind: BaselineDiffKind) -> &'static str {
    match kind {
        BaselineDiffKind::StatusChanged => "status_changed",
        BaselineDiffKind::MetadataChanged => "metadata_changed",
        BaselineDiffKind::MissingInActual => "missing_in_actual",
        BaselineDiffKind::NewInActual => "new_in_actual",
    }
}
