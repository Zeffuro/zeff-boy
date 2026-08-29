use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::baseline::{BaselineDiffKind, BaselineReport, compare_baseline};
use crate::cli::TestFilter;
use crate::fixtures::{FixtureKind, fixture_bytes};
use crate::manifest::{expand_manifest_tests, select_tests, validate_tests};
use crate::model::*;
use crate::report::{junit_failure_status, junit_report};
use crate::runner::{RunReport, TestResult, TestStatus, safe_file_stem};
use crate::util::is_valid_sha256_hex;

fn sample_result(id: &str, status: TestStatus, expectation: ExpectationKind) -> TestResult {
    TestResult {
        id: id.to_string(),
        suite_id: "sample-suite".to_string(),
        suite_name: "Sample Suite".to_string(),
        core: Core::Gba,
        tier: Tier::Accuracy,
        artifact_kind: ArtifactKind::TestRom,
        expectation,
        status,
        duration_ms: 123,
        command: vec!["zeff-boy".to_string(), id.to_string()],
        reason: None,
        exit_code: Some(if junit_failure_status(status) { 1 } else { 0 }),
        stdout_tail: None,
        stderr_tail: None,
    }
}

fn sample_report(result: TestResult) -> RunReport {
    RunReport {
        schema_version: 1,
        generated_unix_ms: 1,
        selected_count: 1,
        results: vec![result],
    }
}

fn sample_loaded_test(id: &str, tier: Tier) -> LoadedTest {
    LoadedTest {
        manifest_path: PathBuf::from("rom-tests/manifests/test-roms/sample.toml"),
        suite: Suite {
            id: "sample".to_string(),
            name: "Sample".to_string(),
            core: Some(Core::Gb),
            upstream_url: None,
            license: None,
        },
        test: TestCase {
            id: id.to_string(),
            core: Core::Gb,
            tier,
            model: None,
            max_frames: 1,
            no_apu: false,
            input: Vec::new(),
            tags: Vec::new(),
            notes: None,
            artifact: Artifact {
                kind: ArtifactKind::TestRom,
                license: "MIT".to_string(),
                license_confidence: LicenseConfidence::Verified,
                redistributable: false,
                source_url: Some("https://example.invalid".to_string()),
                source_version: Some("test".to_string()),
                source_id: Some("source".to_string()),
            },
            rom: RomSpec {
                path: PathBuf::from("rom-tests/cache/sample.gb"),
                sha256: None,
                archive_path: None,
                legacy_paths: Vec::new(),
            },
            pass: PassSpec {
                kind: PassKind::GbFibonacciLdBB,
                contains: None,
                screenshot_frame: None,
                screenshot_sha256: None,
            },
            expectation: Expectation::default(),
        },
    }
}

#[test]
fn parses_manifest() {
    let manifest: Manifest = toml::from_str(
        r#"
manifest_version = 1

[suite]
id = "sample"
name = "Sample"
core = "gb"

[[tests]]
id = "gb/sample"
core = "gb"
tier = "smoke"
max_frames = 1

[tests.artifact]
kind = "test_rom"
license = "MIT"
license_confidence = "verified"
redistributable = false

[tests.rom]
path = "test-roms/sample.gb"
sha256 = "abc"

[tests.pass]
kind = "gb_fibonacci_ld_b_b"

[tests.expectation]
kind = "pass"
"#,
    )
    .unwrap();

    assert_eq!(manifest.tests.len(), 1);
    assert_eq!(manifest.tests[0].core, Core::Gb);
    assert_eq!(manifest.tests[0].tier, Tier::Smoke);
    assert_eq!(manifest.tests[0].pass.kind, PassKind::GbFibonacciLdBB);
}

#[test]
fn parses_headless_exit_pass_kind() {
    let manifest: Manifest = toml::from_str(
        r#"
manifest_version = 1

[suite]
id = "compat"
name = "Compat"
core = "gba"

[[tests]]
id = "compat/local/gba/game"
core = "gba"
tier = "compat"
max_frames = 60

[tests.artifact]
kind = "game_rom"
license = "copyrighted"
license_confidence = "user_owned"
redistributable = false

[tests.rom]
path = "test-roms/GBA/game.gba"

[tests.pass]
kind = "headless_exit"
"#,
    )
    .unwrap();

    assert_eq!(manifest.tests[0].pass.kind, PassKind::HeadlessExit);
}

#[test]
fn parses_ws_screen_text_pass_kind() {
    let manifest: Manifest = toml::from_str(
        r#"
manifest_version = 1

[suite]
id = "ws-sample"
name = "WS Sample"
core = "ws"

[[tests]]
id = "ws/sample/text"
core = "ws"
tier = "local"
max_frames = 60

[tests.artifact]
kind = "test_rom"
license = "unknown"
license_confidence = "unknown"
redistributable = false

[tests.rom]
path = "rom-tests/cache/ws/sample.ws"

[tests.pass]
kind = "ws_screen_text"
contains = "Ok"
"#,
    )
    .unwrap();

    assert_eq!(manifest.tests[0].core, Core::Ws);
    assert_eq!(manifest.tests[0].pass.kind, PassKind::WsScreenText);
    assert_eq!(manifest.tests[0].pass.contains.as_deref(), Some("Ok"));
}

#[test]
fn parses_ws_pass_fail_tiles_pass_kind() {
    let manifest: Manifest = toml::from_str(
        r#"
manifest_version = 1

[suite]
id = "ws-sample"
name = "WS Sample"
core = "ws"

[[tests]]
id = "ws/sample/pass-fail"
core = "ws"
tier = "local"
max_frames = 60

[tests.artifact]
kind = "test_rom"
license = "MIT"
license_confidence = "verified"
redistributable = false

[tests.rom]
path = "rom-tests/cache/ws/sample.ws"

[tests.pass]
kind = "ws_pass_fail_tiles"
"#,
    )
    .unwrap();

    assert_eq!(manifest.tests[0].core, Core::Ws);
    assert_eq!(manifest.tests[0].pass.kind, PassKind::WsPassFailTiles);
}

#[test]
fn parses_pce_memory_status_pass_kind() {
    let manifest: Manifest = toml::from_str(
        r#"
manifest_version = 1

[suite]
id = "pce-sample"
name = "PCE Sample"
core = "pce"

[[tests]]
id = "pce/sample/status"
core = "pce"
tier = "local"
max_frames = 180

[tests.artifact]
kind = "test_rom"
license = "MIT OR Apache-2.0"
license_confidence = "verified"
redistributable = true
source_url = "crates/zeff-romtest/src/fixtures.rs"
source_version = "repository Rust fixture builder"

[tests.rom]
path = "rom-tests/cache/pce/sample.cue"

[tests.pass]
kind = "pce_memory_status"
"#,
    )
    .unwrap();

    assert_eq!(manifest.tests[0].core, Core::Pce);
    assert_eq!(manifest.tests[0].pass.kind, PassKind::PceMemoryStatus);
}

#[test]
fn parses_sega8_sdsc_contains_pass_kind() {
    let manifest: Manifest = toml::from_str(
        r#"
manifest_version = 1

[suite]
id = "sega8-sample"
name = "Sega8 Sample"
core = "sms"

[[tests]]
id = "sega8/sample/sdsc"
core = "sms"
tier = "local"
max_frames = 60

[tests.artifact]
kind = "test_rom"
license = "GPL-2.0"
license_confidence = "verified"
redistributable = false
source_url = "https://example.invalid"
source_version = "test"

[tests.rom]
path = "rom-tests/cache/sega8/sample.sms"

[tests.pass]
kind = "sega8_sdsc_contains"
contains = "Tests complete"
"#,
    )
    .unwrap();

    assert_eq!(manifest.tests[0].core, Core::Sms);
    assert_eq!(manifest.tests[0].pass.kind, PassKind::Sega8SdscContains);
    assert_eq!(
        manifest.tests[0].pass.contains.as_deref(),
        Some("Tests complete")
    );
}

#[test]
fn parses_sega8_audio_nonzero_pass_kind() {
    let manifest: Manifest = toml::from_str(
        r#"
manifest_version = 1

[suite]
id = "sega8-audio"
name = "Sega8 Audio"
core = "gg"

[[tests]]
id = "sega8/sample/audio"
core = "gg"
tier = "local"
max_frames = 60

[tests.artifact]
kind = "test_rom"
license = "unknown"
license_confidence = "unknown"
redistributable = false
source_url = "https://example.invalid"
source_version = "test"

[tests.rom]
path = "rom-tests/cache/sega8/sample.gg"

[tests.pass]
kind = "sega8_audio_nonzero"
"#,
    )
    .unwrap();

    assert_eq!(manifest.tests[0].core, Core::Gg);
    assert_eq!(manifest.tests[0].pass.kind, PassKind::Sega8AudioNonzero);
}

#[test]
fn parses_coleco_audio_nonzero_pass_kind() {
    let manifest: Manifest = toml::from_str(
        r#"
manifest_version = 1

[suite]
id = "coleco-audio"
name = "ColecoVision Audio"
core = "coleco"

[[tests]]
id = "coleco/sample/audio"
core = "coleco"
tier = "local"
max_frames = 60

[tests.artifact]
kind = "test_rom"
license = "unknown"
license_confidence = "unknown"
redistributable = false
source_url = "https://example.invalid"
source_version = "test"

[tests.rom]
path = "rom-tests/cache/coleco/sample.col"

[tests.pass]
kind = "coleco_audio_nonzero"
"#,
    )
    .unwrap();

    assert_eq!(manifest.tests[0].core, Core::Coleco);
    assert_eq!(manifest.tests[0].pass.kind, PassKind::ColecoAudioNonzero);
}

#[test]
fn expands_grouped_manifest_entries() {
    let manifest: Manifest = toml::from_str(
        r#"
manifest_version = 1

[suite]
id = "grouped"
name = "Grouped"
core = "gb"

[[test_groups]]
id_prefix = "gb/grouped"
cache_prefix = "rom-tests/cache/gb"
core = "gb"
tier = "accuracy"
model = "dmg"
max_frames = 1
tags = ["gb", "grouped"]
input = ["a@10-12"]

[test_groups.artifact]
kind = "test_rom"
license = "MIT"
license_confidence = "verified"
redistributable = false
source_url = "https://example.invalid"
source_version = "test"
source_id = "source"

[test_groups.pass]
kind = "gb_fibonacci_ld_b_b"

[[test_groups.roms]]
id = "one"
archive_path = "suite/one.gb"
sha256 = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"

[[test_groups.roms]]
id = "two"
archive_path = "suite/two.gb"
sha256 = "1123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
input = ["down@20-22"]
"#,
    )
    .unwrap();

    let tests = expand_manifest_tests(manifest, Path::new("grouped.toml")).unwrap();
    assert_eq!(tests.len(), 2);
    assert_eq!(tests[0].1.id, "gb/grouped/one");
    assert_eq!(
        tests[0].1.rom.path,
        PathBuf::from("rom-tests/cache/gb").join("suite/one.gb")
    );
    assert_eq!(tests[1].1.id, "gb/grouped/two");
    assert_eq!(tests[0].1.input, vec!["a@10-12"]);
    assert_eq!(tests[1].1.input, vec!["a@10-12", "down@20-22"]);
}

#[test]
fn select_tests_can_exclude_tiers() {
    let tests = vec![
        sample_loaded_test("gb/smoke", Tier::Smoke),
        sample_loaded_test("nes/local", Tier::Local),
    ];
    let filter = TestFilter {
        exclude_tiers: vec![Tier::Local],
        ..TestFilter::default()
    };

    let selected = select_tests(&tests, &filter, false);
    assert_eq!(selected.len(), 1);
    assert_eq!(selected[0].test.id, "gb/smoke");
}

#[test]
fn select_tests_excludes_game_roms_by_default() {
    let mut game = sample_loaded_test("compat/game", Tier::Compat);
    game.manifest_path = PathBuf::from("rom-tests/manifests/compat-games/local.toml");
    game.test.artifact.kind = ArtifactKind::GameRom;
    game.test.artifact.license = "user-owned commercial game".to_string();
    game.test.artifact.license_confidence = LicenseConfidence::UserOwned;
    game.test.artifact.source_url = None;
    game.test.artifact.source_version = None;
    game.test.artifact.source_id = None;
    game.test.rom.path = PathBuf::from("test-roms/compat/game.gba");

    let tests = vec![sample_loaded_test("gb/smoke", Tier::Smoke), game];

    let selected = select_tests(&tests, &TestFilter::default(), false);
    assert_eq!(selected.len(), 1);
    assert_eq!(selected[0].test.id, "gb/smoke");

    let selected_with_games = select_tests(&tests, &TestFilter::default(), true);
    assert_eq!(selected_with_games.len(), 2);
}

#[test]
fn junit_report_treats_expected_fail_as_success() {
    let report = RunReport {
        schema_version: 1,
        generated_unix_ms: 1,
        selected_count: 2,
        results: vec![
            sample_result(
                "gba/jsmolka/arm",
                TestStatus::ExpectedFail,
                ExpectationKind::KnownFail,
            ),
            sample_result("gba/bad", TestStatus::Failed, ExpectationKind::Pass),
        ],
    };

    let xml = junit_report(&report);
    assert!(xml.contains("tests=\"2\""));
    assert!(xml.contains("failures=\"1\""));
    assert!(xml.contains("name=\"gba/jsmolka/arm\""));
    assert!(xml.contains("type=\"failed\""));
}

#[test]
fn baseline_compare_detects_status_change() {
    let baseline_source = sample_report(sample_result(
        "gba/jsmolka/memory",
        TestStatus::Passed,
        ExpectationKind::Pass,
    ));
    let baseline = BaselineReport::from(&baseline_source);
    let actual = sample_report(sample_result(
        "gba/jsmolka/memory",
        TestStatus::Failed,
        ExpectationKind::Pass,
    ));

    let report = compare_baseline(&baseline, &actual).unwrap();
    assert!(report.has_differences());
    assert_eq!(report.diffs.len(), 1);
    assert_eq!(report.diffs[0].kind, BaselineDiffKind::StatusChanged);
}

#[test]
fn baseline_compare_detects_suite_change() {
    let baseline_source = sample_report(sample_result(
        "gba/jsmolka/memory",
        TestStatus::Passed,
        ExpectationKind::Pass,
    ));
    let baseline = BaselineReport::from(&baseline_source);
    let mut actual_result = sample_result(
        "gba/jsmolka/memory",
        TestStatus::Passed,
        ExpectationKind::Pass,
    );
    actual_result.suite_id = "other-suite".to_string();
    let actual = sample_report(actual_result);

    let report = compare_baseline(&baseline, &actual).unwrap();
    assert!(report.has_differences());
    assert_eq!(report.diffs.len(), 1);
    assert_eq!(report.diffs[0].kind, BaselineDiffKind::MetadataChanged);
}

#[test]
fn screenshot_output_names_are_path_safe() {
    assert_eq!(
        safe_file_stem("gb/acid2:dmg?frame=60"),
        "gb_acid2_dmg_frame_60"
    );
}

#[test]
fn sha256_validation_rejects_bad_values() {
    assert!(is_valid_sha256_hex(
        "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
    ));
    assert!(!is_valid_sha256_hex("not-a-sha"));
    assert!(!is_valid_sha256_hex(
        "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdeg"
    ));
}

#[test]
fn generated_fixture_bytes_match_committed_manifest_hashes() {
    for kind in [
        FixtureKind::Sega8,
        FixtureKind::PceCdAdpcmIrq,
        FixtureKind::PceVdcFetchContention,
    ] {
        for (name, bytes, expected_hash) in fixture_bytes(kind) {
            let actual_hash = Sha256::digest(&bytes)
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>();
            assert_eq!(actual_hash, expected_hash, "fixture {name}");
        }
    }
}

#[test]
fn rejects_game_rom_outside_compat_manifest_dir() {
    let loaded = LoadedTest {
        manifest_path: PathBuf::from("rom-tests/manifests/test-roms/bad.toml"),
        suite: Suite {
            id: "suite".to_string(),
            name: "Suite".to_string(),
            core: Some(Core::Gba),
            upstream_url: None,
            license: None,
        },
        test: TestCase {
            id: "compat/bad".to_string(),
            core: Core::Gba,
            tier: Tier::Compat,
            model: None,
            max_frames: 1,
            no_apu: false,
            input: Vec::new(),
            tags: Vec::new(),
            notes: None,
            artifact: Artifact {
                kind: ArtifactKind::GameRom,
                license: "copyrighted".to_string(),
                license_confidence: LicenseConfidence::UserOwned,
                redistributable: false,
                source_url: None,
                source_version: None,
                source_id: None,
            },
            rom: RomSpec {
                path: PathBuf::from("test-roms/game.gba"),
                sha256: None,
                archive_path: None,
                legacy_paths: Vec::new(),
            },
            pass: PassSpec {
                kind: PassKind::Manual,
                contains: None,
                screenshot_frame: None,
                screenshot_sha256: None,
            },
            expectation: Expectation {
                kind: ExpectationKind::Skip,
                reason: None,
            },
        },
    };

    let errors = validate_tests(&[loaded]);
    assert!(errors.iter().any(|error| error.contains("compat-games")));
}

#[test]
fn rejects_clone_friendly_coleco_test_without_a_provisioned_bios() {
    let mut loaded = sample_loaded_test("coleco/needs-bios", Tier::Accuracy);
    loaded.test.core = Core::Coleco;

    let errors = validate_tests(&[loaded]);

    assert!(errors.iter().any(|error| error.contains("retail BIOS")));
}
