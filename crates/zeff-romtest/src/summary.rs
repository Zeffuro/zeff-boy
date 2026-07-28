use std::collections::BTreeMap;

use crate::baseline::BaselineReport;
use crate::model::{Core, Tier};
use crate::runner::{RunReport, TestStatus, test_status_name};

pub(crate) fn print_run_report_summary(report: &RunReport) {
    print_summary(
        "run",
        report.selected_count,
        report.results.iter().map(|result| SummaryEntry {
            id: result.id.as_str(),
            suite_id: result.suite_id.as_str(),
            core: result.core,
            tier: result.tier,
            status: result.status,
            reason: result.reason.as_deref(),
        }),
    );
}

pub(crate) fn print_baseline_report_summary(report: &BaselineReport) {
    print_summary(
        "baseline",
        report.selected_count,
        report.results.iter().map(|result| SummaryEntry {
            id: result.id.as_str(),
            suite_id: result.suite_id.as_str(),
            core: result.core,
            tier: result.tier,
            status: result.status,
            reason: None,
        }),
    );
}

#[derive(Clone, Copy)]
struct SummaryEntry<'a> {
    id: &'a str,
    suite_id: &'a str,
    core: Core,
    tier: Tier,
    status: TestStatus,
    reason: Option<&'a str>,
}

fn print_summary<'a>(
    source_kind: &str,
    selected_count: usize,
    entries: impl IntoIterator<Item = SummaryEntry<'a>>,
) {
    let mut status_counts: BTreeMap<&'static str, usize> = BTreeMap::new();
    let mut coverage_counts: BTreeMap<(Core, Tier, &'static str), usize> = BTreeMap::new();
    let mut suite_counts: BTreeMap<(String, Core, Tier, &'static str), usize> = BTreeMap::new();
    let mut non_passing = Vec::new();

    for entry in entries {
        let status = test_status_name(entry.status);
        *status_counts.entry(status).or_insert(0) += 1;
        *coverage_counts
            .entry((entry.core, entry.tier, status))
            .or_insert(0) += 1;
        *suite_counts
            .entry((entry.suite_id.to_string(), entry.core, entry.tier, status))
            .or_insert(0) += 1;
        if !matches!(entry.status, TestStatus::Passed) {
            non_passing.push(entry);
        }
    }

    println!("source: {source_kind}");
    println!("selected: {selected_count}");

    println!();
    println!("Status summary");
    println!("| Status | Count |");
    println!("| --- | ---: |");
    for (status, count) in status_counts {
        println!("| `{status}` | {count} |");
    }

    println!();
    println!("Coverage summary");
    println!("| Core | Tier | Status | Count |");
    println!("| --- | --- | --- | ---: |");
    for ((core, tier, status), count) in coverage_counts {
        println!("| `{core}` | `{tier}` | `{status}` | {count} |");
    }

    println!();
    println!("Suite summary");
    println!("| Suite | Core | Tier | Status | Count |");
    println!("| --- | --- | --- | --- | ---: |");
    for ((suite_id, core, tier, status), count) in suite_counts {
        println!("| `{suite_id}` | `{core}` | `{tier}` | `{status}` | {count} |");
    }

    if !non_passing.is_empty() {
        println!();
        println!("Non-passing tests");
        println!("| Status | Core | Tier | Suite | Test | Reason |");
        println!("| --- | --- | --- | --- | --- | --- |");
        for entry in non_passing {
            println!(
                "| `{}` | `{}` | `{}` | `{}` | `{}` | {} |",
                test_status_name(entry.status),
                entry.core,
                entry.tier,
                markdown_escape(entry.suite_id),
                markdown_escape(entry.id),
                entry
                    .reason
                    .map(markdown_escape)
                    .unwrap_or_else(|| "-".to_string())
            );
        }
    }
}

fn markdown_escape(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('|', "\\|")
        .replace(['\r', '\n'], " ")
}
