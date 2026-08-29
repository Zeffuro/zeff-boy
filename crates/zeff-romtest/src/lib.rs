use std::ffi::OsString;

use anyhow::{Context, bail};

mod baseline;
mod cli;
mod compat;
mod fetch;
mod fixtures;
mod manifest;
mod model;
mod prepare;
mod regional;
mod report;
mod runner;
mod sources;
mod summary;
mod tooling_policy;
mod util;

#[cfg(test)]
mod tests;

const DEFAULT_MANIFEST_DIR: &str = "rom-tests/manifests";
const DEFAULT_SOURCES_PATH: &str = "rom-tests/sources.toml";
const DEFAULT_SOURCE_CACHE_DIR: &str = "rom-tests/cache/_sources";
const DEFAULT_SCREENSHOT_OUTPUT_DIR: &str = "rom-tests/results/screenshots";
const DEFAULT_BASELINE_PATH: &str = "rom-tests/baselines/current.json";
const USER_AGENT: &str = "zeff-romtest";

pub fn run(args: impl IntoIterator<Item = OsString>) -> anyhow::Result<()> {
    let cli = cli::Cli::parse(args)?;
    match cli.command {
        cli::CommandKind::Help => {
            cli::print_help();
            Ok(())
        }
        cli::CommandKind::List => {
            let tests = manifest::load_tests(&cli.manifest_dir)?;
            let selected = manifest::select_tests(&tests, &cli.filter, cli.include_games);
            manifest::print_test_list(&selected);
            Ok(())
        }
        cli::CommandKind::Check => {
            let tests = manifest::load_tests(&cli.manifest_dir)?;
            manifest::validate_or_report(&tests)
        }
        cli::CommandKind::Prepare => {
            let tests = manifest::load_tests(&cli.manifest_dir)?;
            manifest::validate_or_report(&tests)?;
            let selected = manifest::select_tests(&tests, &cli.filter, cli.include_games);
            let report = prepare::prepare_tests(&selected, &cli)?;
            prepare::print_prepare_summary(&report);
            if report.has_missing_required() {
                bail!("one or more selected test ROMs could not be prepared");
            }
            Ok(())
        }
        cli::CommandKind::Fetch => {
            let tests = manifest::load_tests(&cli.manifest_dir)?;
            manifest::validate_or_report(&tests)?;
            let sources = sources::load_sources(&cli.sources_path)?;
            let selected = manifest::select_tests(&tests, &cli.filter, cli.include_games);
            let report = fetch::fetch_tests(&selected, &sources, &cli)?;
            fetch::print_fetch_summary(&report);
            if report.has_failures() {
                bail!("one or more selected test ROMs could not be fetched");
            }
            Ok(())
        }
        cli::CommandKind::Run => {
            let tests = manifest::load_tests(&cli.manifest_dir)?;
            manifest::validate_or_report(&tests)?;
            let selected = manifest::select_tests(&tests, &cli.filter, cli.include_games);
            let report = runner::run_tests(&selected, &cli)?;
            runner::print_run_summary(&report);
            if let Some(path) = &cli.report_json {
                report::write_json_report(path, &report)?;
            }
            if let Some(path) = &cli.report_md {
                report::write_markdown_report(path, &report)?;
            }
            if let Some(path) = &cli.report_junit {
                report::write_junit_report(path, &report)?;
            }
            if let Some(path) = &cli.report_baseline {
                baseline::write_baseline_report(path, &report)?;
            }
            if report.has_hard_failures() {
                bail!("one or more selected ROM tests failed");
            }
            Ok(())
        }
        cli::CommandKind::Compare => {
            let baseline_path = cli
                .baseline
                .as_deref()
                .context("compare requires --baseline PATH")?;
            let actual_path = cli
                .actual_json
                .as_deref()
                .context("compare requires --actual-json PATH")?;
            let baseline = baseline::read_baseline_report(baseline_path)?;
            let actual = report::read_json_report(actual_path)?;
            let report = baseline::compare_baseline(&baseline, &actual)?;
            baseline::print_baseline_compare_summary(&report);
            if report.has_differences() {
                bail!("ROM test baseline comparison failed");
            }
            Ok(())
        }
        cli::CommandKind::BuildFixture => {
            let fixture = cli
                .fixture
                .context("build-fixture requires --fixture NAME")?;
            let out_dir = cli
                .fixture_out_dir
                .as_deref()
                .context("build-fixture requires --out-dir PATH")?;
            fixtures::build_fixture(fixture, out_dir)
        }
        cli::CommandKind::BuildNesRegional => {
            let out_dir = cli
                .fixture_out_dir
                .as_deref()
                .context("build-nes-regional requires --out-dir PATH")?;
            regional::build_nes_regional(out_dir)
        }
        cli::CommandKind::AuditTooling => tooling_policy::audit_repository(),
        cli::CommandKind::GenerateCompat => compat::generate_compat_manifest(&cli),
        cli::CommandKind::Summary => {
            if let Some(actual_path) = &cli.actual_json {
                let report = report::read_json_report(actual_path)?;
                summary::print_run_report_summary(&report);
            } else {
                let baseline_path = cli
                    .baseline
                    .as_deref()
                    .unwrap_or_else(|| std::path::Path::new(DEFAULT_BASELINE_PATH));
                let report = baseline::read_baseline_report(baseline_path)?;
                summary::print_baseline_report_summary(&report);
            }
            Ok(())
        }
    }
}
