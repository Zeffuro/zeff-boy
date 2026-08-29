use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, bail};

use crate::cli::TestFilter;
use crate::model::*;
use crate::util::{is_valid_sha256_hex, path_contains_component, path_starts_with};

pub(crate) fn load_tests(path: &Path) -> anyhow::Result<Vec<LoadedTest>> {
    let manifest_paths = collect_manifest_paths(path)?;
    let mut loaded = Vec::new();
    for manifest_path in manifest_paths {
        let text = fs::read_to_string(&manifest_path)
            .with_context(|| format!("failed to read {}", manifest_path.display()))?;
        let manifest: Manifest = toml::from_str(&text)
            .with_context(|| format!("failed to parse {}", manifest_path.display()))?;
        if manifest.manifest_version != 1 {
            bail!(
                "{} uses unsupported manifest_version {}",
                manifest_path.display(),
                manifest.manifest_version
            );
        }
        for test in expand_manifest_tests(manifest, &manifest_path)? {
            loaded.push(LoadedTest {
                manifest_path: manifest_path.clone(),
                suite: test.0,
                test: test.1,
            });
        }
    }
    Ok(loaded)
}

pub(crate) fn expand_manifest_tests(
    manifest: Manifest,
    manifest_path: &Path,
) -> anyhow::Result<Vec<(Suite, TestCase)>> {
    let mut tests = Vec::new();
    for test in manifest.tests {
        tests.push((manifest.suite.clone(), test));
    }

    for group in manifest.test_groups {
        if group.id_prefix.trim().is_empty() {
            bail!(
                "{} contains test_group with empty id_prefix",
                manifest_path.display()
            );
        }
        if group.cache_prefix.as_os_str().is_empty() {
            bail!(
                "{} contains test_group '{}' with empty cache_prefix",
                manifest_path.display(),
                group.id_prefix
            );
        }
        if group.roms.is_empty() {
            bail!(
                "{} contains test_group '{}' with no roms",
                manifest_path.display(),
                group.id_prefix
            );
        }

        for rom in &group.roms {
            if rom.id.trim().is_empty() {
                bail!(
                    "{} contains test_group '{}' ROM with empty id",
                    manifest_path.display(),
                    group.id_prefix
                );
            }
            if rom.archive_path.trim().is_empty() {
                bail!(
                    "{} contains test_group '{}' ROM '{}' with empty archive_path",
                    manifest_path.display(),
                    group.id_prefix,
                    rom.id
                );
            }

            let mut tags = group.tags.clone();
            tags.extend(rom.tags.clone());
            let mut input = group.input.clone();
            input.extend(rom.input.clone());
            let archive_path = rom.archive_path.clone();
            let path = rom
                .path
                .clone()
                .unwrap_or_else(|| group.cache_prefix.join(&archive_path));
            let test = TestCase {
                id: join_test_id(&group.id_prefix, &rom.id),
                core: group.core,
                tier: group.tier,
                model: group.model.clone(),
                max_frames: group.max_frames,
                no_apu: group.no_apu,
                input,
                tags,
                notes: rom.notes.clone().or_else(|| group.notes.clone()),
                artifact: group.artifact.clone(),
                rom: RomSpec {
                    path,
                    sha256: rom.sha256.clone(),
                    archive_path: Some(archive_path),
                    legacy_paths: rom.legacy_paths.clone(),
                },
                pass: rom.pass.clone().unwrap_or_else(|| group.pass.clone()),
                expectation: rom
                    .expectation
                    .clone()
                    .unwrap_or_else(|| group.expectation.clone()),
            };
            tests.push((manifest.suite.clone(), test));
        }
    }

    Ok(tests)
}

fn join_test_id(prefix: &str, suffix: &str) -> String {
    format!(
        "{}/{}",
        prefix.trim_end_matches('/'),
        suffix.trim_start_matches('/')
    )
}

fn collect_manifest_paths(path: &Path) -> anyhow::Result<Vec<PathBuf>> {
    if path.is_file() {
        return Ok(vec![path.to_path_buf()]);
    }
    if !path.exists() {
        bail!("manifest path does not exist: {}", path.display());
    }

    let mut paths = Vec::new();
    collect_manifest_paths_recursive(path, &mut paths)?;
    paths.sort();
    Ok(paths)
}

fn collect_manifest_paths_recursive(path: &Path, paths: &mut Vec<PathBuf>) -> anyhow::Result<()> {
    for entry in fs::read_dir(path).with_context(|| format!("failed to read {}", path.display()))? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_manifest_paths_recursive(&path, paths)?;
        } else if path.extension().is_some_and(|ext| ext == "toml") {
            paths.push(path);
        }
    }
    Ok(())
}

pub(crate) fn select_tests<'a>(
    tests: &'a [LoadedTest],
    filter: &TestFilter,
    include_games: bool,
) -> Vec<&'a LoadedTest> {
    tests
        .iter()
        .filter(|loaded| {
            (include_games || !matches!(loaded.test.artifact.kind, ArtifactKind::GameRom))
                && filter.core.is_none_or(|core| loaded.test.core == core)
                && filter.tier.is_none_or(|tier| loaded.test.tier == tier)
                && !filter.exclude_tiers.contains(&loaded.test.tier)
                && filter
                    .id_contains
                    .as_ref()
                    .is_none_or(|needle| loaded.test.id.contains(needle))
                && filter
                    .tag
                    .as_ref()
                    .is_none_or(|tag| loaded.test.tags.iter().any(|candidate| candidate == tag))
        })
        .collect()
}

pub(crate) fn validate_or_report(tests: &[LoadedTest]) -> anyhow::Result<()> {
    let errors = validate_tests(tests);
    if errors.is_empty() {
        println!("manifest check passed: {} test entries", tests.len());
        return Ok(());
    }

    for error in &errors {
        eprintln!("manifest error: {error}");
    }
    bail!("manifest check failed with {} error(s)", errors.len());
}

pub(crate) fn validate_tests(tests: &[LoadedTest]) -> Vec<String> {
    let mut errors = Vec::new();
    let mut ids = HashSet::new();

    for loaded in tests {
        let test = &loaded.test;
        let manifest = loaded.manifest_path.display();

        if loaded.suite.id.trim().is_empty() {
            errors.push(format!("{manifest}: suite id must not be empty"));
        }

        if loaded.suite.name.trim().is_empty() {
            errors.push(format!("{manifest}: suite name must not be empty"));
        }

        if loaded
            .suite
            .upstream_url
            .as_deref()
            .is_some_and(|value| value.trim().is_empty())
        {
            errors.push(format!("{manifest}: suite upstream_url must not be empty"));
        }

        if loaded
            .suite
            .license
            .as_deref()
            .is_some_and(|value| value.trim().is_empty())
        {
            errors.push(format!("{manifest}: suite license must not be empty"));
        }

        if test.id.trim().is_empty() {
            errors.push(format!("{manifest}: test id must not be empty"));
        } else if !ids.insert(test.id.clone()) {
            errors.push(format!("{manifest}: duplicate test id '{}'", test.id));
        }

        if let Some(suite_core) = loaded.suite.core
            && suite_core != test.core
        {
            errors.push(format!(
                "{manifest}: test '{}' core {} differs from suite core {}",
                test.id, test.core, suite_core
            ));
        }

        if test.rom.path.as_os_str().is_empty() {
            errors.push(format!("{manifest}: test '{}' ROM path is empty", test.id));
        }

        for input in &test.input {
            if input.trim().is_empty() {
                errors.push(format!(
                    "{manifest}: test '{}' input entries must not be empty",
                    test.id
                ));
            }
        }

        if matches!(test.artifact.kind, ArtifactKind::TestRom)
            && !path_starts_with(&test.rom.path, ["rom-tests", "cache"])
        {
            errors.push(format!(
                "{manifest}: test '{}' test_rom path must be under rom-tests/cache",
                test.id
            ));
        }

        if matches!(test.artifact.kind, ArtifactKind::GameRom)
            && path_starts_with(&test.rom.path, ["rom-tests", "cache"])
        {
            errors.push(format!(
                "{manifest}: game ROM test '{}' must not use rom-tests/cache",
                test.id
            ));
        }

        if test
            .notes
            .as_deref()
            .is_some_and(|value| value.trim().is_empty())
        {
            errors.push(format!(
                "{manifest}: test '{}' notes must not be empty",
                test.id
            ));
        }

        if test.artifact.license.trim().is_empty() {
            errors.push(format!(
                "{manifest}: test '{}' artifact license must not be empty",
                test.id
            ));
        }

        if matches!(test.artifact.kind, ArtifactKind::TestRom) {
            if test
                .artifact
                .source_url
                .as_deref()
                .is_none_or(|value| value.trim().is_empty())
            {
                errors.push(format!(
                    "{manifest}: test '{}' test_rom artifact must include source_url",
                    test.id
                ));
            }

            if test
                .artifact
                .source_version
                .as_deref()
                .is_none_or(|value| value.trim().is_empty())
            {
                errors.push(format!(
                    "{manifest}: test '{}' test_rom artifact must include source_version",
                    test.id
                ));
            }

            if test
                .artifact
                .source_id
                .as_deref()
                .is_some_and(|value| value.trim().is_empty())
            {
                errors.push(format!(
                    "{manifest}: test '{}' source_id must not be empty",
                    test.id
                ));
            }

            if test
                .rom
                .archive_path
                .as_deref()
                .is_some_and(|value| value.trim().is_empty())
            {
                errors.push(format!(
                    "{manifest}: test '{}' archive_path must not be empty",
                    test.id
                ));
            }
        }

        if matches!(test.artifact.kind, ArtifactKind::GameRom)
            && !path_contains_component(&loaded.manifest_path, "compat-games")
        {
            errors.push(format!(
                "{manifest}: game ROM test '{}' must live under rom-tests/manifests/compat-games",
                test.id
            ));
        }

        if matches!(test.artifact.kind, ArtifactKind::GameRom) && test.artifact.redistributable {
            errors.push(format!(
                "{manifest}: game ROM test '{}' must not be marked redistributable",
                test.id
            ));
        }

        if test.tier == Tier::Compat && !matches!(test.artifact.kind, ArtifactKind::GameRom) {
            errors.push(format!(
                "{manifest}: compat tier test '{}' should use artifact.kind = game_rom",
                test.id
            ));
        }

        if test.core == Core::Coleco && !matches!(test.tier, Tier::Local | Tier::Compat) {
            errors.push(format!(
                "{manifest}: ColecoVision test '{}' requires the user-supplied retail BIOS and must use local or compat tier",
                test.id
            ));
        }

        if test.tier == Tier::Smoke
            && matches!(test.artifact.license_confidence, LicenseConfidence::Unknown)
        {
            errors.push(format!(
                "{manifest}: smoke test '{}' cannot use unknown-license artifacts",
                test.id
            ));
        }

        if matches!(
            test.pass.kind,
            PassKind::GbSerialContains | PassKind::WsScreenText | PassKind::Sega8SdscContains
        ) && test.pass.contains.is_none()
        {
            errors.push(format!(
                "{manifest}: test '{}' uses {} but has no pass.contains",
                test.id, test.pass.kind
            ));
        }

        if matches!(
            test.pass.kind,
            PassKind::GbaScreenshot | PassKind::ScreenshotExact
        ) && test.pass.screenshot_sha256.is_none()
        {
            errors.push(format!(
                "{manifest}: test '{}' uses {} but has no pass.screenshot_sha256",
                test.id, test.pass.kind
            ));
        }

        if test
            .pass
            .screenshot_sha256
            .as_deref()
            .is_some_and(|value| !is_valid_sha256_hex(value))
        {
            errors.push(format!(
                "{manifest}: test '{}' pass.screenshot_sha256 must be a 64-character SHA-256 hex string",
                test.id
            ));
        }
    }

    errors
}

pub(crate) fn print_test_list(tests: &[&LoadedTest]) {
    let mut counts: BTreeMap<(Core, Tier), usize> = BTreeMap::new();
    for loaded in tests {
        *counts
            .entry((loaded.test.core, loaded.test.tier))
            .or_insert(0) += 1;
        println!(
            "{:<7} {:<8} {:<9} {:<16} {}",
            loaded.test.core,
            loaded.test.tier,
            loaded.test.artifact.kind,
            loaded.test.pass.kind,
            loaded.test.id
        );
    }

    println!();
    println!("selected: {}", tests.len());
    for ((core, tier), count) in counts {
        println!("  {core}/{tier}: {count}");
    }
}
