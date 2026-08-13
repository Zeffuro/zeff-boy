use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};

use anyhow::Context;

use crate::USER_AGENT;
use crate::cli::Cli;
use crate::model::*;
use crate::sources::{SourceKind, SourceSpec};
use crate::util::{HashCheck, verify_file_hash, verify_rom_hash};

const SOURCE_DOWNLOAD_ATTEMPTS: usize = 3;

#[derive(Debug)]
pub(crate) struct FetchReport {
    pub(crate) selected_count: usize,
    pub(crate) results: Vec<FetchResult>,
}

impl FetchReport {
    pub(crate) fn has_failures(&self) -> bool {
        self.results.iter().any(|result| {
            matches!(
                result.status,
                FetchStatus::MissingSource
                    | FetchStatus::DownloadFailed
                    | FetchStatus::HashMismatch
                    | FetchStatus::ArchiveEntryMissing
                    | FetchStatus::ExtractFailed
            )
        })
    }
}

#[derive(Debug)]
pub(crate) struct FetchResult {
    pub(crate) id: String,
    pub(crate) core: Core,
    pub(crate) tier: Tier,
    pub(crate) status: FetchStatus,
    pub(crate) target: PathBuf,
    pub(crate) source_id: Option<String>,
    pub(crate) reason: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FetchStatus {
    Present,
    Downloaded,
    Extracted,
    MissingSource,
    DownloadFailed,
    HashMismatch,
    ArchiveEntryMissing,
    ExtractFailed,
    Skipped,
    DryRun,
}

pub(crate) fn fetch_tests(
    tests: &[&LoadedTest],
    sources: &HashMap<String, SourceSpec>,
    cli: &Cli,
) -> anyhow::Result<FetchReport> {
    let mut results = Vec::new();
    for loaded in tests {
        let result = fetch_one(loaded, sources, cli)?;
        println!(
            "{:<21} {:<7} {:<8} {}",
            fetch_status_name(result.status),
            result.core,
            result.tier,
            result.id
        );
        results.push(result);
    }

    Ok(FetchReport {
        selected_count: tests.len(),
        results,
    })
}

fn fetch_one(
    loaded: &LoadedTest,
    sources: &HashMap<String, SourceSpec>,
    cli: &Cli,
) -> anyhow::Result<FetchResult> {
    let test = &loaded.test;
    let target = test.rom.path.clone();

    if matches!(test.artifact.kind, ArtifactKind::GameRom) {
        return Ok(fetch_skipped(
            test,
            "game_rom entries are not managed by fetch",
        ));
    }

    if test.expectation.kind == ExpectationKind::Skip {
        return Ok(fetch_skipped(
            test,
            test.expectation
                .reason
                .as_deref()
                .unwrap_or("manifest expectation is skip"),
        ));
    }

    if target.exists() {
        return match verify_rom_hash(test, &target)? {
            HashCheck::Ok | HashCheck::NoExpectedHash => Ok(FetchResult {
                id: test.id.clone(),
                core: test.core,
                tier: test.tier,
                status: FetchStatus::Present,
                target,
                source_id: test.artifact.source_id.clone(),
                reason: None,
            }),
            HashCheck::Mismatch { expected, actual } => Ok(FetchResult {
                id: test.id.clone(),
                core: test.core,
                tier: test.tier,
                status: FetchStatus::HashMismatch,
                target,
                source_id: test.artifact.source_id.clone(),
                reason: Some(format!(
                    "target sha256 mismatch: expected {expected}, got {actual}"
                )),
            }),
        };
    }

    let Some(source_id) = &test.artifact.source_id else {
        if test.tier == Tier::Local
            || matches!(test.artifact.license_confidence, LicenseConfidence::Unknown)
        {
            return Ok(fetch_skipped(
                test,
                "local-only test has no artifact.source_id; use prepare with a local legacy cache",
            ));
        }

        return Ok(FetchResult {
            id: test.id.clone(),
            core: test.core,
            tier: test.tier,
            status: if cli.allow_missing {
                FetchStatus::Skipped
            } else {
                FetchStatus::MissingSource
            },
            target,
            source_id: None,
            reason: Some("manifest entry has no artifact.source_id".to_string()),
        });
    };

    let Some(source) = sources.get(source_id) else {
        return Ok(FetchResult {
            id: test.id.clone(),
            core: test.core,
            tier: test.tier,
            status: FetchStatus::MissingSource,
            target,
            source_id: Some(source_id.clone()),
            reason: Some(format!("source catalog has no source '{source_id}'")),
        });
    };

    if matches!(source.kind, SourceKind::Zip) && !source.contains_built_roms {
        let mut reason =
            "source archive is build-only and does not contain built ROM artifacts".to_string();
        if let Some(notes) = &source.notes {
            reason.push_str("; ");
            reason.push_str(notes);
        }
        return Ok(fetch_skipped(test, &reason));
    }

    let archive_path = match source.kind {
        SourceKind::Zip => match &test.rom.archive_path {
            Some(path) => Some(path),
            None => {
                return Ok(FetchResult {
                    id: test.id.clone(),
                    core: test.core,
                    tier: test.tier,
                    status: if cli.allow_missing {
                        FetchStatus::Skipped
                    } else {
                        FetchStatus::MissingSource
                    },
                    target,
                    source_id: Some(source_id.clone()),
                    reason: Some("zip source manifest entry has no rom.archive_path".to_string()),
                });
            }
        },
        SourceKind::File => None,
    };

    if !source_fetch_allowed_for_test(source, test) {
        return Ok(FetchResult {
            id: test.id.clone(),
            core: test.core,
            tier: test.tier,
            status: FetchStatus::Skipped,
            target,
            source_id: Some(source_id.clone()),
            reason: Some(format!(
                "source '{}' is not fetch-enabled by policy: license_confidence={}, redistributable={}",
                source.id, source.license_confidence, source.redistributable
            )),
        });
    }

    let archive_file = source_archive_path(source, &cli.source_cache_dir);
    let archive_status = ensure_source_archive(source, &archive_file, cli)?;
    if matches!(
        archive_status,
        FetchStatus::DownloadFailed | FetchStatus::HashMismatch
    ) {
        return Ok(FetchResult {
            id: test.id.clone(),
            core: test.core,
            tier: test.tier,
            status: archive_status,
            target,
            source_id: Some(source_id.clone()),
            reason: Some(format!("failed to prepare source archive {}", source.id)),
        });
    }

    if cli.dry_run {
        let action = match (source.kind, archive_path) {
            (SourceKind::File, _) => {
                format!("would copy downloaded file {}", archive_file.display())
            }
            (SourceKind::Zip, Some(entry)) => {
                format!("would extract {entry} from {}", archive_file.display())
            }
            (SourceKind::Zip, None) => format!("would extract from {}", archive_file.display()),
        };
        return Ok(FetchResult {
            id: test.id.clone(),
            core: test.core,
            tier: test.tier,
            status: FetchStatus::DryRun,
            target,
            source_id: Some(source_id.clone()),
            reason: Some(action),
        });
    }

    match source.kind {
        SourceKind::File => {
            if let Some(parent) = target.parent()
                && !parent.as_os_str().is_empty()
            {
                fs::create_dir_all(parent)
                    .with_context(|| format!("failed to create {}", parent.display()))?;
            }
            fs::copy(&archive_file, &target).with_context(|| {
                format!(
                    "failed to copy {} to {}",
                    archive_file.display(),
                    target.display()
                )
            })?;
        }
        SourceKind::Zip => {
            let entry_path = source_archive_entry_path(source, archive_path.unwrap());
            let extract_result = extract_zip_entry(&archive_file, &entry_path, &target);
            if let Err(err) = extract_result {
                let message = err.to_string();
                let status = if message.contains("file not found in archive") {
                    FetchStatus::ArchiveEntryMissing
                } else {
                    FetchStatus::ExtractFailed
                };
                return Ok(FetchResult {
                    id: test.id.clone(),
                    core: test.core,
                    tier: test.tier,
                    status,
                    target,
                    source_id: Some(source_id.clone()),
                    reason: Some(message),
                });
            }
        }
    }

    match verify_rom_hash(test, &target)? {
        HashCheck::Ok | HashCheck::NoExpectedHash => Ok(FetchResult {
            id: test.id.clone(),
            core: test.core,
            tier: test.tier,
            status: if archive_status == FetchStatus::Downloaded {
                FetchStatus::Downloaded
            } else {
                FetchStatus::Extracted
            },
            target,
            source_id: Some(source_id.clone()),
            reason: None,
        }),
        HashCheck::Mismatch { expected, actual } => Ok(FetchResult {
            id: test.id.clone(),
            core: test.core,
            tier: test.tier,
            status: FetchStatus::HashMismatch,
            target,
            source_id: Some(source_id.clone()),
            reason: Some(format!(
                "extracted ROM sha256 mismatch: expected {expected}, got {actual}"
            )),
        }),
    }
}

fn source_fetch_allowed_for_test(source: &SourceSpec, test: &TestCase) -> bool {
    if source.redistributable {
        return false;
    }

    !matches!(source.license_confidence, LicenseConfidence::Unknown) || test.tier == Tier::Local
}

fn source_archive_entry_path(source: &SourceSpec, archive_path: &str) -> String {
    let archive_path = archive_path.trim_start_matches('/');
    let archive_path = match &source.archive_path_strip_prefix {
        Some(strip_prefix) => archive_path
            .strip_prefix(strip_prefix.trim_matches('/'))
            .map(|path| path.trim_start_matches('/'))
            .unwrap_or(archive_path),
        None => archive_path,
    };

    match &source.archive_prefix {
        Some(prefix) => format!("{}/{}", prefix.trim_matches('/'), archive_path),
        None => archive_path.to_string(),
    }
}

fn fetch_skipped(test: &TestCase, reason: &str) -> FetchResult {
    FetchResult {
        id: test.id.clone(),
        core: test.core,
        tier: test.tier,
        status: FetchStatus::Skipped,
        target: test.rom.path.clone(),
        source_id: test.artifact.source_id.clone(),
        reason: Some(reason.to_string()),
    }
}

fn source_archive_path(source: &SourceSpec, source_cache_dir: &Path) -> PathBuf {
    let file_name = source
        .url
        .rsplit('/')
        .next()
        .filter(|value| !value.is_empty())
        .unwrap_or("source-archive");
    source_cache_dir.join(&source.id).join(file_name)
}

fn ensure_source_archive(
    source: &SourceSpec,
    path: &Path,
    cli: &Cli,
) -> anyhow::Result<FetchStatus> {
    if path.exists() {
        return match verify_file_hash(path, &source.sha256)? {
            HashCheck::Ok | HashCheck::NoExpectedHash => Ok(FetchStatus::Present),
            HashCheck::Mismatch { .. } => Ok(FetchStatus::HashMismatch),
        };
    }

    if cli.dry_run {
        return Ok(FetchStatus::DryRun);
    }

    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }

    download_source_archive_with_retries(source, path, download_bytes)
}

fn download_source_archive_with_retries(
    source: &SourceSpec,
    path: &Path,
    mut download: impl FnMut(&str) -> anyhow::Result<Vec<u8>>,
) -> anyhow::Result<FetchStatus> {
    let mut last_download_error = None;
    let mut last_status = FetchStatus::DownloadFailed;

    for _ in 0..SOURCE_DOWNLOAD_ATTEMPTS {
        let bytes = match download(&source.url) {
            Ok(bytes) => bytes,
            Err(err) => {
                last_download_error = Some(err);
                last_status = FetchStatus::DownloadFailed;
                continue;
            }
        };

        fs::write(path, bytes).with_context(|| format!("failed to write {}", path.display()))?;

        match verify_file_hash(path, &source.sha256)? {
            HashCheck::Ok | HashCheck::NoExpectedHash => return Ok(FetchStatus::Downloaded),
            HashCheck::Mismatch { .. } => {
                last_download_error = None;
                last_status = FetchStatus::HashMismatch;
            }
        }
    }

    if let Some(err) = last_download_error {
        eprintln!(
            "failed to download source archive {} after {} attempts: {err}",
            source.id, SOURCE_DOWNLOAD_ATTEMPTS
        );
    }

    Ok(last_status)
}

fn download_bytes(url: &str) -> anyhow::Result<Vec<u8>> {
    ureq::get(url)
        .header("User-Agent", USER_AGENT)
        .call()
        .map_err(|err| anyhow::anyhow!("HTTP request failed ({url}): {err}"))?
        .into_body()
        .read_to_vec()
        .with_context(|| format!("failed to read HTTP response body from {url}"))
}

fn extract_zip_entry(archive_path: &Path, entry_path: &str, target: &Path) -> anyhow::Result<()> {
    let archive_bytes = fs::read(archive_path)
        .with_context(|| format!("failed to read {}", archive_path.display()))?;
    let reader = Cursor::new(archive_bytes);
    let mut archive = zip::ZipArchive::new(reader)
        .with_context(|| format!("failed to open zip {}", archive_path.display()))?;
    let mut entry = archive
        .by_name(entry_path)
        .with_context(|| format!("file not found in archive: {entry_path}"))?;

    if let Some(parent) = target.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }

    let mut bytes = Vec::new();
    entry
        .read_to_end(&mut bytes)
        .with_context(|| format!("failed to read archive entry {entry_path}"))?;
    fs::write(target, bytes).with_context(|| format!("failed to write {}", target.display()))?;
    Ok(())
}

pub(crate) fn print_fetch_summary(report: &FetchReport) {
    let mut counts: BTreeMap<&'static str, usize> = BTreeMap::new();
    for result in &report.results {
        *counts.entry(fetch_status_name(result.status)).or_insert(0) += 1;
    }

    println!();
    println!("selected: {}", report.selected_count);
    for (status, count) in counts {
        println!("  {status}: {count}");
    }

    for result in &report.results {
        if result.reason.is_some() || result.source_id.is_some() {
            let source = result.source_id.as_deref().unwrap_or("-");
            let reason = result.reason.as_deref().unwrap_or("-");
            println!(
                "  {}: source={} target={} reason={}",
                result.id,
                source,
                result.target.display(),
                reason
            );
        }
    }
}

fn fetch_status_name(status: FetchStatus) -> &'static str {
    match status {
        FetchStatus::Present => "present",
        FetchStatus::Downloaded => "downloaded",
        FetchStatus::Extracted => "extracted",
        FetchStatus::MissingSource => "missing_source",
        FetchStatus::DownloadFailed => "download_failed",
        FetchStatus::HashMismatch => "hash_mismatch",
        FetchStatus::ArchiveEntryMissing => "archive_entry_missing",
        FetchStatus::ExtractFailed => "extract_failed",
        FetchStatus::Skipped => "skipped",
        FetchStatus::DryRun => "dry_run",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};

    fn source(license_confidence: LicenseConfidence) -> SourceSpec {
        source_with_sha256(license_confidence, "0".repeat(64))
    }

    fn source_with_sha256(license_confidence: LicenseConfidence, sha256: String) -> SourceSpec {
        SourceSpec {
            id: "source".to_string(),
            kind: SourceKind::Zip,
            url: "https://example.invalid/source.zip".to_string(),
            sha256,
            archive_prefix: Some("archive-root".to_string()),
            archive_path_strip_prefix: Some("local-cache-root".to_string()),
            contains_built_roms: true,
            license: "unknown".to_string(),
            license_confidence,
            redistributable: false,
            notes: None,
        }
    }

    fn sha256_bytes(bytes: &[u8]) -> String {
        let digest = Sha256::digest(bytes);
        digest.iter().map(|byte| format!("{byte:02x}")).collect()
    }

    fn test_case(tier: Tier) -> TestCase {
        TestCase {
            id: "nes/test".to_string(),
            core: Core::Nes,
            tier,
            model: None,
            max_frames: 1,
            no_apu: false,
            input: Vec::new(),
            tags: Vec::new(),
            notes: None,
            artifact: Artifact {
                kind: ArtifactKind::TestRom,
                license: "unknown".to_string(),
                license_confidence: LicenseConfidence::Unknown,
                redistributable: false,
                source_url: Some("https://example.invalid/source.zip".to_string()),
                source_version: Some("test".to_string()),
                source_id: Some("source".to_string()),
            },
            rom: RomSpec {
                path: PathBuf::from("rom-tests/cache/nes/test.nes"),
                sha256: None,
                archive_path: Some("nes-test-roms/test.nes".to_string()),
                legacy_paths: Vec::new(),
            },
            pass: PassSpec {
                kind: PassKind::Nes6000Status,
                contains: None,
                screenshot_frame: None,
                screenshot_sha256: None,
            },
            expectation: Expectation::default(),
        }
    }

    #[test]
    fn archive_prefix_is_prepended_to_zip_entry_path() {
        assert_eq!(
            source_archive_entry_path(
                &source(LicenseConfidence::Unknown),
                "local-cache-root/nes-test-roms/a.nes"
            ),
            "archive-root/nes-test-roms/a.nes"
        );
    }

    #[test]
    fn source_archive_download_retries_transient_failure() {
        let bytes = b"ok".to_vec();
        let source = source_with_sha256(LicenseConfidence::Verified, sha256_bytes(&bytes));
        let test_dir = std::env::temp_dir().join(format!(
            "zeff-romtest-fetch-retry-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system time should be after unix epoch")
                .as_nanos()
        ));
        std::fs::create_dir_all(&test_dir).expect("failed to create temporary test directory");
        let target = test_dir.join("source.zip");
        let mut attempts = 0;

        let status = download_source_archive_with_retries(&source, &target, |_| {
            attempts += 1;
            if attempts == 1 {
                anyhow::bail!("transient failure");
            }
            Ok(bytes.clone())
        })
        .expect("download should eventually succeed");

        assert_eq!(status, FetchStatus::Downloaded);
        assert_eq!(attempts, 2);
        assert_eq!(
            std::fs::read(&target).expect("downloaded source should exist"),
            bytes
        );

        let _ = std::fs::remove_dir_all(&test_dir);
    }

    #[test]
    fn unknown_license_sources_are_allowed_only_for_local_tier() {
        let source = source(LicenseConfidence::Unknown);

        assert!(source_fetch_allowed_for_test(
            &source,
            &test_case(Tier::Local)
        ));
        assert!(!source_fetch_allowed_for_test(
            &source,
            &test_case(Tier::Accuracy)
        ));
    }
}
