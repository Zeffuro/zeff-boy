use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, bail};
use serde::{Deserialize, Serialize};

use crate::DEFAULT_SCREENSHOT_OUTPUT_DIR;
use crate::cli::Cli;
use crate::model::*;
use crate::util::sha256_file;

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct RunReport {
    pub(crate) schema_version: u32,
    pub(crate) generated_unix_ms: u128,
    pub(crate) selected_count: usize,
    pub(crate) results: Vec<TestResult>,
}

impl RunReport {
    pub(crate) fn has_hard_failures(&self) -> bool {
        self.results.iter().any(|result| {
            matches!(
                result.status,
                TestStatus::Failed
                    | TestStatus::HashMismatch
                    | TestStatus::Missing
                    | TestStatus::UnexpectedPass
            )
        })
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct TestResult {
    pub(crate) id: String,
    #[serde(default)]
    pub(crate) suite_id: String,
    #[serde(default)]
    pub(crate) suite_name: String,
    pub(crate) core: Core,
    pub(crate) tier: Tier,
    pub(crate) artifact_kind: ArtifactKind,
    pub(crate) expectation: ExpectationKind,
    pub(crate) status: TestStatus,
    pub(crate) duration_ms: u128,
    pub(crate) command: Vec<String>,
    pub(crate) reason: Option<String>,
    pub(crate) exit_code: Option<i32>,
    pub(crate) stdout_tail: Option<String>,
    pub(crate) stderr_tail: Option<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum TestStatus {
    Passed,
    Failed,
    ExpectedFail,
    UnexpectedPass,
    Skipped,
    Missing,
    HashMismatch,
    DryRun,
}

pub(crate) fn run_tests(tests: &[&LoadedTest], cli: &Cli) -> anyhow::Result<RunReport> {
    let mut results = Vec::new();
    for loaded in tests {
        let result = run_one(loaded, cli)?;
        print_one_line_result(&result);
        results.push(result);
    }

    Ok(RunReport {
        schema_version: 1,
        generated_unix_ms: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or(Duration::ZERO)
            .as_millis(),
        selected_count: tests.len(),
        results,
    })
}

fn run_one(loaded: &LoadedTest, cli: &Cli) -> anyhow::Result<TestResult> {
    let test = &loaded.test;
    let start = SystemTime::now();

    if matches!(test.artifact.kind, ArtifactKind::GameRom) && !cli.include_games {
        return Ok(skipped_result(
            loaded,
            start,
            "game_rom entries require --include-games",
        ));
    }

    if test.expectation.kind == ExpectationKind::Skip {
        return Ok(skipped_result(
            loaded,
            start,
            test.expectation
                .reason
                .as_deref()
                .unwrap_or("manifest expectation is skip"),
        ));
    }

    let Some(mut invocation) = build_invocation(test, cli)? else {
        return Ok(skipped_result(
            loaded,
            start,
            "pass detector is not supported by the current headless CLI",
        ));
    };

    if !test.rom.path.exists() {
        return Ok(TestResult {
            status: if cli.allow_missing {
                TestStatus::Skipped
            } else {
                TestStatus::Missing
            },
            reason: Some(format!("ROM file is missing: {}", test.rom.path.display())),
            ..base_result(loaded, start, invocation.display_args(), None)
        });
    }

    if let Some(expected_sha256) = &test.rom.sha256 {
        let actual_sha256 = sha256_file(&test.rom.path)?;
        if !expected_sha256.eq_ignore_ascii_case(&actual_sha256) {
            return Ok(TestResult {
                status: TestStatus::HashMismatch,
                reason: Some(format!(
                    "sha256 mismatch: expected {expected_sha256}, got {actual_sha256}"
                )),
                ..base_result(loaded, start, invocation.display_args(), None)
            });
        }
    }

    if cli.dry_run {
        return Ok(TestResult {
            status: TestStatus::DryRun,
            reason: Some("dry run".to_string()),
            ..base_result(loaded, start, invocation.display_args(), None)
        });
    }

    let output = invocation
        .command
        .output()
        .with_context(|| format!("failed to run test '{}'", test.id))?;
    let mut reason = test.expectation.reason.clone();
    let mut success = output.status.success();
    if success
        && let Some(screenshot_path) = &invocation.screenshot_path
        && let Err(error) = verify_screenshot_result(test, screenshot_path)
    {
        success = false;
        reason = Some(match reason {
            Some(existing) => format!("{existing}; {error}"),
            None => error.to_string(),
        });
    }
    let status = match (test.expectation.kind, success) {
        (ExpectationKind::Pass, true) => TestStatus::Passed,
        (ExpectationKind::Pass, false) => TestStatus::Failed,
        (ExpectationKind::KnownFail, false) => TestStatus::ExpectedFail,
        (ExpectationKind::KnownFail, true) => TestStatus::UnexpectedPass,
        (ExpectationKind::Skip, _) => TestStatus::Skipped,
    };

    Ok(TestResult {
        status,
        reason,
        exit_code: output.status.code(),
        stdout_tail: tail_utf8(&output.stdout),
        stderr_tail: tail_utf8(&output.stderr),
        ..base_result(loaded, start, invocation.display_args(), None)
    })
}

fn verify_screenshot_result(test: &TestCase, screenshot_path: &Path) -> anyhow::Result<()> {
    let expected = test.pass.screenshot_sha256.as_deref().with_context(|| {
        format!(
            "{} requires pass.screenshot_sha256 for screenshot comparison",
            test.id
        )
    })?;
    if !screenshot_path.exists() {
        bail!("screenshot was not written: {}", screenshot_path.display());
    }
    let actual = sha256_file(screenshot_path)?;
    if !expected.eq_ignore_ascii_case(&actual) {
        bail!(
            "screenshot sha256 mismatch: expected {expected}, got {actual} ({})",
            screenshot_path.display()
        );
    }
    Ok(())
}

fn base_result(
    loaded: &LoadedTest,
    start: SystemTime,
    command: Vec<String>,
    status: Option<TestStatus>,
) -> TestResult {
    TestResult {
        id: loaded.test.id.clone(),
        suite_id: loaded.suite.id.clone(),
        suite_name: loaded.suite.name.clone(),
        core: loaded.test.core,
        tier: loaded.test.tier,
        artifact_kind: loaded.test.artifact.kind,
        expectation: loaded.test.expectation.kind,
        status: status.unwrap_or(TestStatus::Skipped),
        duration_ms: start.elapsed().unwrap_or(Duration::ZERO).as_millis(),
        command,
        reason: None,
        exit_code: None,
        stdout_tail: None,
        stderr_tail: None,
    }
}

fn skipped_result(loaded: &LoadedTest, start: SystemTime, reason: &str) -> TestResult {
    TestResult {
        status: TestStatus::Skipped,
        reason: Some(reason.to_string()),
        ..base_result(loaded, start, Vec::new(), None)
    }
}

struct Invocation {
    pub(crate) command: Command,
    pub(crate) display_args: Vec<String>,
    pub(crate) screenshot_path: Option<PathBuf>,
}

impl Invocation {
    fn display_args(&self) -> Vec<String> {
        self.display_args.clone()
    }
}

fn build_invocation(test: &TestCase, cli: &Cli) -> anyhow::Result<Option<Invocation>> {
    let mut zeff_args = Vec::new();
    let mut screenshot_path = None;
    zeff_args.push("--headless".to_string());
    zeff_args.push("--max-frames".to_string());
    zeff_args.push(test.max_frames.to_string());

    if test.artifact.kind == ArtifactKind::TestRom {
        zeff_args.push("--no-sram".to_string());
    }

    if test.no_apu {
        zeff_args.push("--no-apu".to_string());
    }

    for input in &test.input {
        zeff_args.push("--input".to_string());
        zeff_args.push(input.clone());
    }

    if let Some(model) = &test.model
        && test.core == Core::Gb
    {
        zeff_args.push("--mode".to_string());
        zeff_args.push(model.clone());
    }

    match test.pass.kind {
        PassKind::GbFibonacciLdBB
        | PassKind::GbScreenText
        | PassKind::GbMemoryStatus
        | PassKind::Nes6000Status
        | PassKind::GbaScreenText
        | PassKind::GbaMgbaSuiteSram
        | PassKind::PceMemoryStatus => {
            zeff_args.push("--expect-test-pass".to_string());
        }
        PassKind::GbSerialContains => {
            let contains = test
                .pass
                .contains
                .clone()
                .context("gb_serial_contains requires pass.contains")?;
            zeff_args.push("--expect-serial".to_string());
            zeff_args.push(contains);
        }
        PassKind::GbaScreenshot | PassKind::ScreenshotExact => {
            let path = screenshot_output_path(test);
            zeff_args.push("--screenshot".to_string());
            zeff_args.push(path.to_string_lossy().into_owned());
            if let Some(frame) = test.pass.screenshot_frame {
                zeff_args.push("--screenshot-frame".to_string());
                zeff_args.push(frame.to_string());
            }
            screenshot_path = Some(path);
        }
        PassKind::WsScreenText => {
            let contains = test
                .pass
                .contains
                .clone()
                .context("ws_screen_text requires pass.contains")?;
            zeff_args.push("--expect-ws-text".to_string());
            zeff_args.push(contains);
        }
        PassKind::WsPassFailTiles => {
            zeff_args.push("--expect-ws-pass-fail-tiles".to_string());
        }
        PassKind::Sega8SdscContains => {
            let contains = test
                .pass
                .contains
                .clone()
                .context("sega8_sdsc_contains requires pass.contains")?;
            zeff_args.push("--expect-sega8-sdsc".to_string());
            zeff_args.push(contains);
        }
        PassKind::Sega8AudioNonzero => {
            zeff_args.push("--expect-sega8-audio".to_string());
        }
        PassKind::HeadlessExit => {}
        PassKind::ScreenshotPerceptual | PassKind::Manual => return Ok(None),
    }

    zeff_args.push(test.rom.path.to_string_lossy().into_owned());

    let mut display_args;
    let mut command = if let Some(path) = &cli.zeff_boy {
        display_args = vec![path.to_string_lossy().into_owned()];
        Command::new(path)
    } else {
        display_args = vec![
            "cargo".to_string(),
            "run".to_string(),
            "--quiet".to_string(),
            "-p".to_string(),
            "zeff-boy".to_string(),
            "--bin".to_string(),
            "zeff-boy".to_string(),
            "--".to_string(),
        ];
        let mut command = Command::new("cargo");
        command.args([
            "run", "--quiet", "-p", "zeff-boy", "--bin", "zeff-boy", "--",
        ]);
        command
    };

    command.args(&zeff_args);
    display_args.extend(zeff_args);
    Ok(Some(Invocation {
        command,
        display_args,
        screenshot_path,
    }))
}

fn screenshot_output_path(test: &TestCase) -> PathBuf {
    PathBuf::from(DEFAULT_SCREENSHOT_OUTPUT_DIR).join(format!("{}.png", safe_file_stem(&test.id)))
}

pub(crate) fn safe_file_stem(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.') {
                ch
            } else {
                '_'
            }
        })
        .collect()
}

fn tail_utf8(bytes: &[u8]) -> Option<String> {
    if bytes.is_empty() {
        return None;
    }
    let text = String::from_utf8_lossy(bytes);
    let max_chars = 4000;
    if text.chars().count() <= max_chars {
        Some(text.into_owned())
    } else {
        Some(
            text.chars()
                .rev()
                .take(max_chars)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect(),
        )
    }
}

fn print_one_line_result(result: &TestResult) {
    println!(
        "{:<15} {:<7} {:<8} {}",
        test_status_name(result.status),
        result.core,
        result.tier,
        result.id
    );
}

pub(crate) fn print_run_summary(report: &RunReport) {
    let mut counts: BTreeMap<&'static str, usize> = BTreeMap::new();
    for result in &report.results {
        *counts.entry(test_status_name(result.status)).or_insert(0) += 1;
    }

    println!();
    println!("selected: {}", report.selected_count);
    for (status, count) in counts {
        println!("  {status}: {count}");
    }
}

pub(crate) fn test_status_name(status: TestStatus) -> &'static str {
    match status {
        TestStatus::Passed => "passed",
        TestStatus::Failed => "failed",
        TestStatus::ExpectedFail => "expected_fail",
        TestStatus::UnexpectedPass => "unexpected_pass",
        TestStatus::Skipped => "skipped",
        TestStatus::Missing => "missing",
        TestStatus::HashMismatch => "hash_mismatch",
        TestStatus::DryRun => "dry_run",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::Cli;

    fn cli() -> Cli {
        Cli::parse(["run"].map(Into::into)).unwrap()
    }

    fn test_case(artifact_kind: ArtifactKind) -> TestCase {
        TestCase {
            id: "runner/no-sram".to_string(),
            core: Core::Gba,
            tier: Tier::Accuracy,
            model: None,
            max_frames: 10,
            no_apu: false,
            input: Vec::new(),
            tags: Vec::new(),
            notes: None,
            artifact: Artifact {
                kind: artifact_kind,
                license: "test".to_string(),
                license_confidence: LicenseConfidence::Verified,
                redistributable: false,
                source_url: Some("https://example.invalid/test.gba".to_string()),
                source_version: Some("test".to_string()),
                source_id: None,
            },
            rom: RomSpec {
                path: PathBuf::from("rom-tests/cache/test.gba"),
                sha256: None,
                archive_path: None,
                legacy_paths: Vec::new(),
            },
            pass: PassSpec {
                kind: PassKind::HeadlessExit,
                contains: None,
                screenshot_frame: None,
                screenshot_sha256: None,
            },
            expectation: Expectation::default(),
        }
    }

    #[test]
    fn test_rom_invocations_disable_persistent_sram() {
        let invocation = build_invocation(&test_case(ArtifactKind::TestRom), &cli())
            .unwrap()
            .unwrap();

        assert!(invocation.display_args.contains(&"--no-sram".to_string()));
    }

    #[test]
    fn game_rom_invocations_keep_persistent_sram() {
        let invocation = build_invocation(&test_case(ArtifactKind::GameRom), &cli())
            .unwrap()
            .unwrap();

        assert!(!invocation.display_args.contains(&"--no-sram".to_string()));
    }

    #[test]
    fn manifest_input_events_are_forwarded_to_headless_cli() {
        let mut test = test_case(ArtifactKind::TestRom);
        test.input = vec!["a@10-12".to_string(), "down@20-22".to_string()];

        let invocation = build_invocation(&test, &cli()).unwrap().unwrap();

        assert!(
            invocation
                .display_args
                .windows(2)
                .any(|pair| pair == ["--input".to_string(), "a@10-12".to_string()])
        );
        assert!(
            invocation
                .display_args
                .windows(2)
                .any(|pair| pair == ["--input".to_string(), "down@20-22".to_string()])
        );
    }

    #[test]
    fn ws_screen_text_invocations_forward_expected_text() {
        let mut test = test_case(ArtifactKind::TestRom);
        test.core = Core::Ws;
        test.rom.path = PathBuf::from("rom-tests/cache/test.ws");
        test.pass = PassSpec {
            kind: PassKind::WsScreenText,
            contains: Some("00 ADD".to_string()),
            screenshot_frame: None,
            screenshot_sha256: None,
        };

        let invocation = build_invocation(&test, &cli()).unwrap().unwrap();

        assert!(
            invocation
                .display_args
                .windows(2)
                .any(|pair| pair == ["--expect-ws-text".to_string(), "00 ADD".to_string()])
        );
    }

    #[test]
    fn ws_pass_fail_tile_invocations_forward_expected_flag() {
        let mut test = test_case(ArtifactKind::TestRom);
        test.core = Core::Ws;
        test.rom.path = PathBuf::from("rom-tests/cache/test.ws");
        test.pass = PassSpec {
            kind: PassKind::WsPassFailTiles,
            contains: None,
            screenshot_frame: None,
            screenshot_sha256: None,
        };

        let invocation = build_invocation(&test, &cli()).unwrap().unwrap();

        assert!(
            invocation
                .display_args
                .contains(&"--expect-ws-pass-fail-tiles".to_string())
        );
    }

    #[test]
    fn sega8_sdsc_invocations_forward_expected_text() {
        let mut test = test_case(ArtifactKind::TestRom);
        test.core = Core::Sms;
        test.rom.path = PathBuf::from("rom-tests/cache/test.sms");
        test.pass = PassSpec {
            kind: PassKind::Sega8SdscContains,
            contains: Some("Tests complete".to_string()),
            screenshot_frame: None,
            screenshot_sha256: None,
        };

        let invocation = build_invocation(&test, &cli()).unwrap().unwrap();

        assert!(invocation.display_args.windows(2).any(|pair| pair
            == [
                "--expect-sega8-sdsc".to_string(),
                "Tests complete".to_string()
            ]));
    }

    #[test]
    fn sega8_audio_invocations_forward_expected_flag() {
        let mut test = test_case(ArtifactKind::TestRom);
        test.core = Core::Gg;
        test.rom.path = PathBuf::from("rom-tests/cache/test.gg");
        test.pass = PassSpec {
            kind: PassKind::Sega8AudioNonzero,
            contains: None,
            screenshot_frame: None,
            screenshot_sha256: None,
        };

        let invocation = build_invocation(&test, &cli()).unwrap().unwrap();

        assert!(
            invocation
                .display_args
                .contains(&"--expect-sega8-audio".to_string())
        );
    }

    #[test]
    fn pce_memory_status_invocations_forward_expected_flag() {
        let mut test = test_case(ArtifactKind::TestRom);
        test.core = Core::Pce;
        test.rom.path = PathBuf::from("rom-tests/cache/pce/test.cue");
        test.pass.kind = PassKind::PceMemoryStatus;

        let invocation = build_invocation(&test, &cli()).unwrap().unwrap();

        assert!(
            invocation
                .display_args
                .contains(&"--expect-test-pass".to_string())
        );
    }
}
