use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use anyhow::{Context, bail};

use crate::model::{Core, Tier};
use crate::runner::{RunReport, TestResult, TestStatus, test_status_name};
use crate::util::ensure_parent_dir;

pub(crate) fn write_json_report(path: &Path, report: &RunReport) -> anyhow::Result<()> {
    ensure_parent_dir(path)?;
    let json = serde_json::to_string_pretty(report)?;
    fs::write(path, json).with_context(|| format!("failed to write {}", path.display()))?;
    println!("report: {}", path.display());
    Ok(())
}

pub(crate) fn write_markdown_report(path: &Path, report: &RunReport) -> anyhow::Result<()> {
    ensure_parent_dir(path)?;
    fs::write(path, markdown_report(report))
        .with_context(|| format!("failed to write {}", path.display()))?;
    println!("markdown-report: {}", path.display());
    Ok(())
}

pub(crate) fn write_junit_report(path: &Path, report: &RunReport) -> anyhow::Result<()> {
    ensure_parent_dir(path)?;
    fs::write(path, junit_report(report))
        .with_context(|| format!("failed to write {}", path.display()))?;
    println!("junit-report: {}", path.display());
    Ok(())
}

pub(crate) fn read_json_report(path: &Path) -> anyhow::Result<RunReport> {
    let text =
        fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?;
    let report: RunReport = serde_json::from_str(&text)
        .with_context(|| format!("failed to parse {}", path.display()))?;
    if report.schema_version != 1 {
        bail!(
            "{} uses unsupported run report schema_version {}",
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

fn markdown_report(report: &RunReport) -> String {
    let mut status_counts: BTreeMap<&'static str, usize> = BTreeMap::new();
    let mut coverage_counts: BTreeMap<(Core, Tier, &'static str), usize> = BTreeMap::new();
    let mut suite_counts: BTreeMap<(String, Core, Tier, &'static str), usize> = BTreeMap::new();
    for result in &report.results {
        let status = test_status_name(result.status);
        *status_counts.entry(status).or_insert(0) += 1;
        *coverage_counts
            .entry((result.core, result.tier, status))
            .or_insert(0) += 1;
        *suite_counts
            .entry((result.suite_id.clone(), result.core, result.tier, status))
            .or_insert(0) += 1;
    }

    let mut markdown = String::new();
    markdown.push_str("# Zeff Boy ROM test report\n\n");
    markdown.push_str(&format!(
        "- Schema version: {}\n- Generated Unix ms: {}\n- Selected tests: {}\n\n",
        report.schema_version, report.generated_unix_ms, report.selected_count
    ));

    markdown.push_str("## Status summary\n\n");
    markdown.push_str("| Status | Count |\n| --- | ---: |\n");
    for (status, count) in status_counts {
        markdown.push_str(&format!("| `{status}` | {count} |\n"));
    }

    markdown.push_str("\n## Coverage summary\n\n");
    markdown.push_str("| Core | Tier | Status | Count |\n| --- | --- | --- | ---: |\n");
    for ((core, tier, status), count) in coverage_counts {
        markdown.push_str(&format!("| `{core}` | `{tier}` | `{status}` | {count} |\n"));
    }

    markdown.push_str("\n## Suite summary\n\n");
    markdown
        .push_str("| Suite | Core | Tier | Status | Count |\n| --- | --- | --- | --- | ---: |\n");
    for ((suite_id, core, tier, status), count) in suite_counts {
        markdown.push_str(&format!(
            "| `{}` | `{core}` | `{tier}` | `{status}` | {count} |\n",
            markdown_escape(&suite_id)
        ));
    }

    markdown.push_str("\n## Tests\n\n");
    markdown.push_str(
        "| Status | Core | Tier | Expectation | Duration ms | Suite | Test | Reason |\n| --- | --- | --- | --- | ---: | --- | --- | --- |\n",
    );
    for result in &report.results {
        markdown.push_str(&format!(
            "| `{}` | `{}` | `{}` | `{}` | {} | `{}` | `{}` | {} |\n",
            test_status_name(result.status),
            result.core,
            result.tier,
            result.expectation,
            result.duration_ms,
            markdown_escape(&result.suite_id),
            markdown_escape(&result.id),
            result
                .reason
                .as_deref()
                .map(markdown_escape)
                .unwrap_or_else(|| "-".to_string())
        ));
    }

    markdown
}

pub(crate) fn junit_report(report: &RunReport) -> String {
    let failures = report
        .results
        .iter()
        .filter(|result| junit_failure_status(result.status))
        .count();
    let skipped = report
        .results
        .iter()
        .filter(|result| junit_skipped_status(result.status))
        .count();
    let total_time = report
        .results
        .iter()
        .map(|result| result.duration_ms)
        .sum::<u128>() as f64
        / 1000.0;

    let mut xml = String::new();
    xml.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    xml.push_str(&format!(
        "<testsuite name=\"zeff-romtest\" tests=\"{}\" failures=\"{}\" skipped=\"{}\" time=\"{:.3}\">\n",
        report.results.len(),
        failures,
        skipped,
        total_time
    ));

    for result in &report.results {
        xml.push_str(&format!(
            "  <testcase classname=\"zeff-romtest.{}.{}.{}\" name=\"{}\" time=\"{:.3}\">\n",
            result.core,
            result.tier,
            xml_escape(&result.suite_id),
            xml_escape(&result.id),
            result.duration_ms as f64 / 1000.0
        ));

        if junit_skipped_status(result.status) {
            let message = result
                .reason
                .as_deref()
                .unwrap_or_else(|| test_status_name(result.status));
            xml.push_str(&format!(
                "    <skipped message=\"{}\" />\n",
                xml_escape(message)
            ));
        } else if junit_failure_status(result.status) {
            let message = result
                .reason
                .as_deref()
                .unwrap_or_else(|| test_status_name(result.status));
            xml.push_str(&format!(
                "    <failure type=\"{}\" message=\"{}\">",
                test_status_name(result.status),
                xml_escape(message)
            ));
            xml.push_str(&xml_escape(&junit_failure_body(result)));
            xml.push_str("</failure>\n");
        }

        if let Some(stdout) = &result.stdout_tail {
            xml.push_str("    <system-out>");
            xml.push_str(&xml_escape(stdout));
            xml.push_str("</system-out>\n");
        }
        if let Some(stderr) = &result.stderr_tail {
            xml.push_str("    <system-err>");
            xml.push_str(&xml_escape(stderr));
            xml.push_str("</system-err>\n");
        }

        xml.push_str("  </testcase>\n");
    }

    xml.push_str("</testsuite>\n");
    xml
}

pub(crate) fn junit_failure_status(status: TestStatus) -> bool {
    matches!(
        status,
        TestStatus::Failed
            | TestStatus::HashMismatch
            | TestStatus::Missing
            | TestStatus::UnexpectedPass
    )
}

fn junit_skipped_status(status: TestStatus) -> bool {
    matches!(status, TestStatus::Skipped | TestStatus::DryRun)
}

fn junit_failure_body(result: &TestResult) -> String {
    let mut body = String::new();
    body.push_str(&format!("status: {}\n", test_status_name(result.status)));
    body.push_str(&format!("suite: {}\n", result.suite_id));
    body.push_str(&format!("expectation: {}\n", result.expectation));
    if let Some(reason) = &result.reason {
        body.push_str(&format!("reason: {reason}\n"));
    }
    if let Some(code) = result.exit_code {
        body.push_str(&format!("exit_code: {code}\n"));
    }
    if !result.command.is_empty() {
        body.push_str("command: ");
        body.push_str(&result.command.join(" "));
        body.push('\n');
    }
    if let Some(stdout) = &result.stdout_tail {
        body.push_str("\nstdout_tail:\n");
        body.push_str(stdout);
        body.push('\n');
    }
    if let Some(stderr) = &result.stderr_tail {
        body.push_str("\nstderr_tail:\n");
        body.push_str(stderr);
        body.push('\n');
    }
    body
}

fn markdown_escape(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('|', "\\|")
        .replace(['\r', '\n'], " ")
}

fn xml_escape(value: &str) -> String {
    let mut escaped = String::new();
    for ch in value.chars() {
        match ch {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&apos;"),
            '\t' | '\n' | '\r' => escaped.push(ch),
            ch if ch < ' ' => escaped.push(' '),
            ch => escaped.push(ch),
        }
    }
    escaped
}
