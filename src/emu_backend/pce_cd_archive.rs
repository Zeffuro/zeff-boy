#![cfg(not(target_arch = "wasm32"))]

use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering};
#[cfg(feature = "profile-cores")]
use std::time::Instant;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use sevenz_rust2::{
    ArchiveEntry, ArchiveLimits, ArchiveReader, EncoderMethod, Error as SevenzError, Password,
};
use sha2::{Digest, Sha256};

use super::ActiveSystem;
use super::pce_cd::{
    LoadedPceCd, PCE_CD_CUE_BYTES_LIMIT, PCE_CD_DATA_BYTES_LIMIT, PceCdLoadError,
    build_disc_with_mods, normalize_portable_path, parse_cue_bytes, pce_cd_mod_config,
};
use super::pce_cd_file::{
    CueFileSource, load_cached_cue_file_backed, try_load_cached_cue_ppf_overlay,
};
use crate::rom_archive::ArchiveRomEntry;

const SEVEN_ZIP_ARCHIVE_BYTES_LIMIT: u64 = 8_u64 * 1024 * 1024 * 1024;
const SEVEN_ZIP_DECODED_BYTES_LIMIT: u64 = 8_u64 * 1024 * 1024 * 1024;
const PCE_CD_7Z_DECODED_BYTES_LIMIT: u64 = 1024 * 1024 * 1024;
const PCE_CD_7Z_ENTRY_LIMIT: usize = 256;
const PCE_CD_7Z_METADATA_CANCEL_BOUND_BYTES: usize = 64 * 1024 * 1024;
const STREAM_BUFFER_BYTES: usize = 64 * 1024;
const GENERIC_ROM_BYTES_LIMIT: u64 = 128 * 1024 * 1024;
const CACHE_FORMAT_VERSION: u32 = 1;
const CACHE_COMPLETE_FILE: &str = "complete.json";
const CACHE_FILES_DIR: &str = "files";
const CACHE_ENTRY_BYTES_LIMIT: u64 = PCE_CD_7Z_DECODED_BYTES_LIMIT;
const CACHE_ENTRY_MEMBER_LIMIT: usize = PCE_CD_7Z_ENTRY_LIMIT;
const CACHE_MANIFEST_BYTES_LIMIT: u64 = 1024 * 1024;
const CACHE_MAX_COMPLETE_ENTRIES: usize = 2;
const WINDOWS_REPARSE_POINT: u32 = 0x400;
const UNIX_FILE_TYPE_MASK: u32 = 0o170000;
const UNIX_SYMBOLIC_LINK: u32 = 0o120000;
static CACHE_LOCK: Mutex<()> = Mutex::new(());
#[cfg(test)]
const DEFAULT_DECODER_MEMORY_LIMIT_MIB: usize = 128;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub(crate) enum PceCdPackageLoadPhase {
    Inspecting,
    ReadingCue,
    ReadingData,
    ReadingRom,
    Firmware,
    Building,
    Complete,
}

pub(crate) struct PceCdPackageProgress {
    phase: AtomicU8,
    completed_bytes: AtomicU64,
    total_bytes: AtomicU64,
    #[cfg(test)]
    cancel_after_completed_bytes: AtomicU64,
}

impl Default for PceCdPackageProgress {
    fn default() -> Self {
        Self {
            phase: AtomicU8::new(PceCdPackageLoadPhase::Inspecting as u8),
            completed_bytes: AtomicU64::new(0),
            total_bytes: AtomicU64::new(0),
            #[cfg(test)]
            cancel_after_completed_bytes: AtomicU64::new(0),
        }
    }
}

impl PceCdPackageProgress {
    pub(crate) fn phase(&self) -> PceCdPackageLoadPhase {
        match self.phase.load(Ordering::Acquire) {
            1 => PceCdPackageLoadPhase::ReadingCue,
            2 => PceCdPackageLoadPhase::ReadingData,
            3 => PceCdPackageLoadPhase::ReadingRom,
            4 => PceCdPackageLoadPhase::Firmware,
            5 => PceCdPackageLoadPhase::Building,
            6 => PceCdPackageLoadPhase::Complete,
            _ => PceCdPackageLoadPhase::Inspecting,
        }
    }

    pub(crate) fn completed_bytes(&self) -> u64 {
        self.completed_bytes.load(Ordering::Acquire)
    }

    pub(crate) fn total_bytes(&self) -> u64 {
        self.total_bytes.load(Ordering::Acquire)
    }

    pub(super) fn set_phase(&self, phase: PceCdPackageLoadPhase) {
        self.phase.store(phase as u8, Ordering::Release);
    }

    pub(super) fn set_total_bytes(&self, total: u64) {
        self.total_bytes.store(total, Ordering::Release);
    }

    pub(super) fn set_completed_bytes(&self, completed: u64) {
        self.completed_bytes.store(completed, Ordering::Release);
    }

    #[cfg(test)]
    fn set_cancel_after_completed_bytes(&self, completed: u64) {
        self.cancel_after_completed_bytes
            .store(completed, Ordering::Release);
    }

    fn update_decode_progress(&self, completed: u64, _cancel: &AtomicBool) {
        self.set_completed_bytes(completed);
        #[cfg(test)]
        {
            let threshold = self.cancel_after_completed_bytes.load(Ordering::Acquire);
            if threshold != 0 && completed >= threshold {
                _cancel.store(true, Ordering::Release);
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ArchiveManifest {
    cue_names: Vec<String>,
    entries: Vec<Member>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Member {
    index: usize,
    name: String,
    size: u64,
    is_directory: bool,
    is_anti: bool,
    is_link: bool,
    has_stream: bool,
    crc_checked: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct CachedArchive {
    version: u32,
    source_size: u64,
    source_modified_secs: u64,
    source_modified_nanos: u32,
    members: Vec<CachedMember>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct CachedMember {
    name: String,
    size: u64,
    sha256: [u8; 32],
}

struct CacheEntry {
    path: PathBuf,
    manifest: CachedArchive,
    extracted_bytes: u64,
}

#[derive(Clone, Copy)]
struct SourceFingerprint {
    size: u64,
    modified_secs: u64,
    modified_nanos: u32,
}

#[derive(Clone, Copy)]
struct CacheIdentity<'a> {
    source: SourceFingerprint,
    key: &'a str,
}

struct StagingCache {
    cache_root: PathBuf,
    path: PathBuf,
    published: bool,
}

impl Drop for StagingCache {
    fn drop(&mut self) {
        if !self.published {
            remove_cache_entry(&self.cache_root, &self.path);
        }
    }
}

#[derive(Clone, Copy)]
struct DecodePassPolicy {
    progress_base: u64,
    decoded_bytes_limit: u64,
}

pub(crate) enum SevenZipContents {
    Cd { cue_path: PathBuf },
    Roms(Vec<ArchiveRomEntry>),
}

#[cfg(test)]
pub(crate) fn inspect_7z_cue_path(path: &Path) -> Result<PathBuf, PceCdLoadError> {
    let (_, manifest) = open_validated(path, DEFAULT_DECODER_MEMORY_LIMIT_MIB)?;
    Ok(virtual_member_path(path, unique_cue_name(&manifest)?))
}

pub(crate) fn inspect_7z_contents(
    path: &Path,
    decoder_memory_limit_mib: usize,
) -> Result<SevenZipContents, PceCdLoadError> {
    let (_, manifest) = open_validated(path, decoder_memory_limit_mib)?;
    if !manifest.cue_names.is_empty() {
        return Ok(SevenZipContents::Cd {
            cue_path: virtual_member_path(path, unique_cue_name(&manifest)?),
        });
    }
    let entries = rom_entries(&manifest);
    if entries.is_empty() {
        Err(PceCdLoadError::NoSupportedArchiveContent)
    } else {
        Ok(SevenZipContents::Roms(entries))
    }
}

#[cfg(test)]
pub(crate) fn load_7z_cue(path: &Path) -> Result<LoadedPceCd, PceCdLoadError> {
    let cancel = AtomicBool::new(false);
    let progress = PceCdPackageProgress::default();
    load_7z_cue_with_control(path, &cancel, &progress, DEFAULT_DECODER_MEMORY_LIMIT_MIB)
        .map(|(_, loaded)| loaded)
}

#[cfg(test)]
pub(crate) fn load_7z_cue_with_control(
    path: &Path,
    cancel: &AtomicBool,
    progress: &PceCdPackageProgress,
    decoder_memory_limit_mib: usize,
) -> Result<(PathBuf, LoadedPceCd), PceCdLoadError> {
    load_7z_cue_with_control_and_mods(path, cancel, progress, decoder_memory_limit_mib, false)
}

pub(crate) fn load_7z_cue_with_control_and_mods(
    path: &Path,
    cancel: &AtomicBool,
    progress: &PceCdPackageProgress,
    decoder_memory_limit_mib: usize,
    apply_mods: bool,
) -> Result<(PathBuf, LoadedPceCd), PceCdLoadError> {
    let cache_root = pce_cd_cache_root();
    load_7z_cue_with_cache_root(
        path,
        cancel,
        progress,
        decoder_memory_limit_mib,
        apply_mods,
        &cache_root,
    )
}

#[cfg(feature = "profile-cores")]
pub(crate) fn profile_cache_load(path: &Path, cache_root: &Path) -> Result<(), String> {
    std::fs::create_dir_all(cache_root).map_err(|error| error.to_string())?;
    if std::fs::read_dir(cache_root)
        .map_err(|error| error.to_string())?
        .next()
        .is_some()
    {
        return Err("ZEFF_PROFILE_PCE_CD_CACHE_ROOT must be empty".to_owned());
    }

    let legacy_started = Instant::now();
    let legacy = profile_legacy_load(path).map_err(|error| error.to_string())?;
    let legacy_elapsed = legacy_started.elapsed();

    let load = || {
        let cancel = AtomicBool::new(false);
        let progress = PceCdPackageProgress::default();
        let started = Instant::now();
        let loaded = load_7z_cue_with_cache_root(path, &cancel, &progress, 512, false, cache_root)
            .map_err(|error| error.to_string())?;
        Ok::<_, String>((loaded, started.elapsed(), progress.total_bytes()))
    };

    let (cold, cold_elapsed, cold_bytes) = load()?;
    let (warm, warm_elapsed, warm_bytes) = load()?;
    if legacy.0 != cold.0
        || legacy.1.content_sha256 != cold.1.content_sha256
        || legacy.1.content_crc32 != cold.1.content_crc32
        || legacy.1.source_disc_sha256 != cold.1.source_disc_sha256
        || legacy.1.disc != cold.1.disc
        || cold.0 != warm.0
        || cold.1.content_sha256 != warm.1.content_sha256
        || cold.1.content_crc32 != warm.1.content_crc32
        || cold.1.source_disc_sha256 != warm.1.source_disc_sha256
        || cold.1.disc != warm.1.disc
    {
        return Err("cold and warm cache loads differ".to_owned());
    }

    println!(
        "pce_cd_cache legacy_ms={:.3} cold_ms={:.3} warm_ms={:.3} cold_progress_bytes={} warm_progress_bytes={}",
        legacy_elapsed.as_secs_f64() * 1_000.0,
        cold_elapsed.as_secs_f64() * 1_000.0,
        warm_elapsed.as_secs_f64() * 1_000.0,
        cold_bytes,
        warm_bytes,
    );
    Ok(())
}

#[cfg(feature = "profile-cores")]
fn profile_legacy_load(path: &Path) -> Result<(PathBuf, LoadedPceCd), PceCdLoadError> {
    let cancel = AtomicBool::new(false);
    let progress = PceCdPackageProgress::default();
    let (mut reader, manifest) = open_validated(path, 512)?;
    let cue_name = unique_cue_name(&manifest)?.to_owned();
    let decoded_per_pass = manifest
        .entries
        .iter()
        .try_fold(0_u64, |total, member| total.checked_add(member.size))
        .ok_or(PceCdLoadError::ArchiveDecodedLimit)?;
    let cue_target = BTreeSet::from([cue_name.clone()]);
    let mut cue_members = decode_pass(
        &mut reader,
        &manifest,
        &cue_target,
        &cancel,
        &progress,
        DecodePassPolicy {
            progress_base: 0,
            decoded_bytes_limit: PCE_CD_7Z_DECODED_BYTES_LIMIT,
        },
    )?;
    let cue_bytes = cue_members
        .remove(&cue_name)
        .ok_or_else(|| PceCdLoadError::ArchiveMemberMissing(cue_name.clone()))?;
    let sheet = parse_cue_bytes(&cue_bytes)?;

    let mut resolved = Vec::with_capacity(sheet.files.len());
    let mut targets = BTreeSet::from([cue_name.clone()]);
    for file in &sheet.files {
        let name = resolve_reference(&manifest, &cue_name, &file.reference)?;
        targets.insert(name.clone());
        resolved.push(name);
    }
    let mut members = decode_pass(
        &mut reader,
        &manifest,
        &targets,
        &cancel,
        &progress,
        DecodePassPolicy {
            progress_base: decoded_per_pass,
            decoded_bytes_limit: PCE_CD_7Z_DECODED_BYTES_LIMIT,
        },
    )?;
    let second_cue = members
        .remove(&cue_name)
        .ok_or(PceCdLoadError::ArchiveChanged)?;
    if second_cue != cue_bytes {
        return Err(PceCdLoadError::ArchiveChanged);
    }
    let files = resolved
        .into_iter()
        .map(|name| {
            members
                .remove(&name)
                .ok_or(PceCdLoadError::ArchiveMemberMissing(name))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let loaded = build_disc_with_mods(cue_bytes, &sheet, files, false)?;
    Ok((virtual_member_path(path, &cue_name), loaded))
}

fn load_7z_cue_with_cache_root(
    path: &Path,
    cancel: &AtomicBool,
    progress: &PceCdPackageProgress,
    decoder_memory_limit_mib: usize,
    apply_mods: bool,
    cache_root: &Path,
) -> Result<(PathBuf, LoadedPceCd), PceCdLoadError> {
    let _cache_guard = CACHE_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    check_cancelled(cancel)?;
    progress.set_phase(PceCdPackageLoadPhase::Inspecting);
    let (mut reader, manifest) = open_validated(path, decoder_memory_limit_mib)?;
    let cue_name = unique_cue_name(&manifest)?.to_owned();
    check_cancelled(cancel)?;
    validate_cacheable_manifest(&manifest)?;
    let decoded_bytes = manifest
        .entries
        .iter()
        .try_fold(0_u64, |total, member| total.checked_add(member.size))
        .ok_or(PceCdLoadError::ArchiveDecodedLimit)?;
    if decoded_bytes > CACHE_ENTRY_BYTES_LIMIT {
        return Err(PceCdLoadError::ArchiveDecodedLimit);
    }
    let source = source_fingerprint(path)?;
    let cache_key = cache_key(path, source);
    let identity = CacheIdentity {
        source,
        key: &cache_key,
    };
    let mut cache = prepare_cache(
        &mut reader,
        &manifest,
        identity,
        cache_root,
        cancel,
        progress,
    )?;

    for attempt in 0..2 {
        match load_cached_disc(&cache, &manifest, &cue_name, cancel, progress, apply_mods) {
            Ok(loaded) => {
                touch_cache_entry(&cache.path);
                prune_cache(cache_root, Some(&cache.path));
                return Ok((virtual_member_path(path, &cue_name), loaded));
            }
            Err(CachedDiscError::Load(error)) => return Err(error),
            Err(CachedDiscError::Corrupt) if attempt == 0 => {
                remove_cache_entry(cache_root, &cache.path);
                let (mut retry_reader, retry_manifest) =
                    open_validated(path, decoder_memory_limit_mib)?;
                if retry_manifest != manifest {
                    return Err(PceCdLoadError::ArchiveChanged);
                }
                cache = extract_cache(
                    &mut retry_reader,
                    &manifest,
                    identity,
                    cache_root,
                    cancel,
                    progress,
                )?;
            }
            Err(CachedDiscError::Corrupt) => return Err(PceCdLoadError::ArchiveChanged),
        }
    }
    unreachable!()
}

enum CachedDiscError {
    Corrupt,
    Load(PceCdLoadError),
}

#[cfg(test)]
fn pce_cd_cache_root() -> PathBuf {
    std::env::temp_dir().join(format!("zeff-pce-cd-cache-tests-{}", std::process::id()))
}

#[cfg(not(test))]
fn pce_cd_cache_root() -> PathBuf {
    crate::platform::cache_dir().join("pce-cd-7z-v1")
}

fn validate_cacheable_manifest(manifest: &ArchiveManifest) -> Result<(), PceCdLoadError> {
    if manifest.entries.len() > CACHE_ENTRY_MEMBER_LIMIT {
        return Err(PceCdLoadError::TooManyArchiveEntries(
            manifest.entries.len(),
        ));
    }
    for member in &manifest.entries {
        if member.is_link {
            return Err(PceCdLoadError::ArchiveLinkUnsupported(member.name.clone()));
        }
        if member.has_stream {
            validate_regular_member(member)?;
        }
    }
    Ok(())
}

fn source_fingerprint(path: &Path) -> Result<SourceFingerprint, PceCdLoadError> {
    let metadata = std::fs::metadata(path)
        .map_err(|_| PceCdLoadError::ArchiveUnreadable(path.to_path_buf()))?;
    let modified = metadata
        .modified()
        .unwrap_or(UNIX_EPOCH)
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    Ok(SourceFingerprint {
        size: metadata.len(),
        modified_secs: modified.as_secs(),
        modified_nanos: modified.subsec_nanos(),
    })
}

fn cache_key(path: &Path, source: SourceFingerprint) -> String {
    let canonical = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let mut hasher = Sha256::new();
    hasher.update(b"zeff-boy:pce-cd-7z-cache:v1\0");
    update_path_hash(&mut hasher, &canonical);
    hasher.update(source.size.to_le_bytes());
    hasher.update(source.modified_secs.to_le_bytes());
    hasher.update(source.modified_nanos.to_le_bytes());
    const_hex::encode(hasher.finalize())
}

#[cfg(windows)]
fn update_path_hash(hasher: &mut Sha256, path: &Path) {
    use std::os::windows::ffi::OsStrExt;

    for unit in path.as_os_str().encode_wide() {
        hasher.update(unit.to_le_bytes());
    }
}

#[cfg(not(windows))]
fn update_path_hash(hasher: &mut Sha256, path: &Path) {
    use std::os::unix::ffi::OsStrExt;

    hasher.update(path.as_os_str().as_bytes());
}

fn prepare_cache(
    reader: &mut ArchiveReader<File>,
    archive: &ArchiveManifest,
    identity: CacheIdentity<'_>,
    cache_root: &Path,
    cancel: &AtomicBool,
    progress: &PceCdPackageProgress,
) -> Result<CacheEntry, PceCdLoadError> {
    std::fs::create_dir_all(cache_root).map_err(cache_io_error)?;
    cleanup_stale_staging(cache_root);
    let path = cache_root.join(identity.key);
    if let Some(manifest) = read_cache_manifest(&path, archive, identity.source) {
        return Ok(CacheEntry {
            path,
            manifest,
            extracted_bytes: 0,
        });
    }
    remove_cache_entry(cache_root, &path);
    extract_cache(reader, archive, identity, cache_root, cancel, progress)
}

fn extract_cache(
    reader: &mut ArchiveReader<File>,
    archive: &ArchiveManifest,
    identity: CacheIdentity<'_>,
    cache_root: &Path,
    cancel: &AtomicBool,
    progress: &PceCdPackageProgress,
) -> Result<CacheEntry, PceCdLoadError> {
    static STAGING_NONCE: AtomicU64 = AtomicU64::new(0);

    check_cancelled(cancel)?;
    let decoded_total = archive
        .entries
        .iter()
        .try_fold(0_u64, |total, member| total.checked_add(member.size))
        .ok_or(PceCdLoadError::ArchiveDecodedLimit)?;
    if decoded_total > CACHE_ENTRY_BYTES_LIMIT || archive.entries.len() > CACHE_ENTRY_MEMBER_LIMIT {
        return Err(PceCdLoadError::ArchiveDecodedLimit);
    }
    progress.set_phase(PceCdPackageLoadPhase::ReadingData);
    progress.set_total_bytes(decoded_total);
    progress.set_completed_bytes(0);

    let nonce = STAGING_NONCE.fetch_add(1, Ordering::Relaxed);
    let staging_path = cache_root.join(format!(
        ".{}.tmp-{}-{nonce}",
        identity.key,
        std::process::id()
    ));
    std::fs::create_dir(&staging_path).map_err(cache_io_error)?;
    let mut staging = StagingCache {
        cache_root: cache_root.to_path_buf(),
        path: staging_path,
        published: false,
    };
    let files_root = staging.path.join(CACHE_FILES_DIR);
    std::fs::create_dir(&files_root).map_err(cache_io_error)?;

    let mut cached = Vec::with_capacity(archive.entries.len());
    let mut decoded = 0_u64;
    let mut failure = None;
    let result = reader.for_each_entries(|entry, input| {
        if failure.is_some() {
            return Err(cancel_error());
        }
        if cancel.load(Ordering::Acquire) {
            failure = Some(PceCdLoadError::ArchiveCancelled);
            return Err(cancel_error());
        }
        let name = match normalize_portable_path(entry.name()) {
            Ok(name) => name,
            Err(()) => {
                failure = Some(PceCdLoadError::UnsafeArchiveEntry(entry.name().to_owned()));
                return Err(cancel_error());
            }
        };
        let Some(expected) = archive.entries.iter().find(|member| member.name == name) else {
            failure = Some(PceCdLoadError::ArchiveChanged);
            return Err(cancel_error());
        };
        if !expected.has_stream {
            return Ok(true);
        }

        let output_path = cache_member_path(&files_root, &name);
        let Some(parent) = output_path.parent() else {
            failure = Some(PceCdLoadError::UnsafeArchiveEntry(name));
            return Err(cancel_error());
        };
        if let Err(error) = std::fs::create_dir_all(parent) {
            failure = Some(cache_io_error(error));
            return Err(cancel_error());
        }
        let mut output = match File::create(&output_path) {
            Ok(output) => output,
            Err(error) => {
                failure = Some(cache_io_error(error));
                return Err(cancel_error());
            }
        };
        let mut hasher = Sha256::new();
        let mut local = 0_u64;
        let mut buffer = [0; STREAM_BUFFER_BYTES];
        loop {
            if cancel.load(Ordering::Acquire) {
                failure = Some(PceCdLoadError::ArchiveCancelled);
                return Err(cancel_error());
            }
            let count = input.read(&mut buffer)?;
            if count == 0 {
                break;
            }
            if let Err(error) = output.write_all(&buffer[..count]) {
                failure = Some(cache_io_error(error));
                return Err(cancel_error());
            }
            hasher.update(&buffer[..count]);
            local = match local.checked_add(count as u64) {
                Some(value) => value,
                None => {
                    failure = Some(PceCdLoadError::ArchiveDecodedLimit);
                    return Err(cancel_error());
                }
            };
            decoded = match decoded.checked_add(count as u64) {
                Some(value) if value <= CACHE_ENTRY_BYTES_LIMIT => value,
                _ => {
                    failure = Some(PceCdLoadError::ArchiveDecodedLimit);
                    return Err(cancel_error());
                }
            };
            progress.update_decode_progress(decoded, cancel);
        }
        if local != expected.size {
            failure = Some(PceCdLoadError::ArchiveMemberSizeMismatch(name));
            return Err(cancel_error());
        }
        cached.push(CachedMember {
            name,
            size: local,
            sha256: hasher.finalize().into(),
        });
        Ok(true)
    });
    if let Some(error) = failure {
        return Err(error);
    }
    result.map_err(map_sevenz_error)?;
    check_cancelled(cancel)?;
    let stream_count = archive
        .entries
        .iter()
        .filter(|member| member.has_stream)
        .count();
    if decoded != decoded_total || cached.len() != stream_count {
        return Err(PceCdLoadError::ArchiveChanged);
    }

    let manifest = CachedArchive {
        version: CACHE_FORMAT_VERSION,
        source_size: identity.source.size,
        source_modified_secs: identity.source.modified_secs,
        source_modified_nanos: identity.source.modified_nanos,
        members: cached,
    };
    let manifest_bytes = serde_json::to_vec(&manifest).map_err(cache_io_error)?;
    if manifest_bytes.len() as u64 > CACHE_MANIFEST_BYTES_LIMIT {
        return Err(PceCdLoadError::ArchiveDecodedLimit);
    }
    std::fs::write(staging.path.join(CACHE_COMPLETE_FILE), manifest_bytes)
        .map_err(cache_io_error)?;
    check_cancelled(cancel)?;

    let final_path = cache_root.join(identity.key);
    if let Some(manifest) = read_cache_manifest(&final_path, archive, identity.source) {
        return Ok(CacheEntry {
            path: final_path,
            manifest,
            extracted_bytes: decoded_total,
        });
    }
    if final_path.exists() {
        remove_cache_entry(cache_root, &final_path);
    }
    if let Err(error) = std::fs::rename(&staging.path, &final_path) {
        if let Some(manifest) = read_cache_manifest(&final_path, archive, identity.source) {
            return Ok(CacheEntry {
                path: final_path,
                manifest,
                extracted_bytes: decoded_total,
            });
        }
        return Err(cache_io_error(error));
    }
    staging.published = true;
    prune_cache(cache_root, Some(&final_path));
    Ok(CacheEntry {
        path: final_path,
        manifest,
        extracted_bytes: decoded_total,
    })
}

fn read_cache_manifest(
    path: &Path,
    archive: &ArchiveManifest,
    source: SourceFingerprint,
) -> Option<CachedArchive> {
    let root_metadata = std::fs::symlink_metadata(path).ok()?;
    if !root_metadata.is_dir() || root_metadata.file_type().is_symlink() {
        return None;
    }
    let marker = path.join(CACHE_COMPLETE_FILE);
    let marker_metadata = std::fs::symlink_metadata(&marker).ok()?;
    if !marker_metadata.is_file()
        || marker_metadata.file_type().is_symlink()
        || marker_metadata.len() > CACHE_MANIFEST_BYTES_LIMIT
    {
        return None;
    }
    let bytes = std::fs::read(marker).ok()?;
    let cached: CachedArchive = serde_json::from_slice(&bytes).ok()?;
    if cached.version != CACHE_FORMAT_VERSION
        || cached.source_size != source.size
        || cached.source_modified_secs != source.modified_secs
        || cached.source_modified_nanos != source.modified_nanos
        || cached.members.len() > CACHE_ENTRY_MEMBER_LIMIT
    {
        return None;
    }
    let expected = archive
        .entries
        .iter()
        .filter(|member| member.has_stream)
        .collect::<Vec<_>>();
    if cached.members.len() != expected.len() {
        return None;
    }
    for (member, expected) in cached.members.iter().zip(expected) {
        if member.name != expected.name || member.size != expected.size {
            return None;
        }
        let file = cache_member_path(&path.join(CACHE_FILES_DIR), &member.name);
        let metadata = std::fs::symlink_metadata(file).ok()?;
        if !metadata.is_file()
            || metadata.file_type().is_symlink()
            || metadata.len() != member.size
            || metadata_is_reparse_point(&metadata)
        {
            return None;
        }
    }
    Some(cached)
}

fn load_cached_disc(
    cache: &CacheEntry,
    archive: &ArchiveManifest,
    cue_name: &str,
    cancel: &AtomicBool,
    progress: &PceCdPackageProgress,
    apply_mods: bool,
) -> Result<LoadedPceCd, CachedDiscError> {
    check_cancelled(cancel).map_err(CachedDiscError::Load)?;
    let cue = cache
        .manifest
        .members
        .iter()
        .find(|member| member.name == cue_name)
        .ok_or(CachedDiscError::Corrupt)?;
    progress.set_phase(PceCdPackageLoadPhase::ReadingCue);
    progress.set_completed_bytes(cache.extracted_bytes);
    progress.set_total_bytes(cache.extracted_bytes.saturating_add(cue.size));
    let cue_bytes = read_cached_member(cache, cue, cancel, progress, cache.extracted_bytes)?;
    let sheet = parse_cue_bytes(&cue_bytes).map_err(CachedDiscError::Load)?;

    let mut resolved = Vec::with_capacity(sheet.files.len());
    let mut data_total = 0_u64;
    for file in &sheet.files {
        let name =
            resolve_reference(archive, cue_name, &file.reference).map_err(CachedDiscError::Load)?;
        let member = archive
            .entries
            .iter()
            .find(|member| member.name == name)
            .ok_or_else(|| {
                CachedDiscError::Load(PceCdLoadError::ArchiveMemberMissing(name.clone()))
            })?;
        validate_data_member(member).map_err(CachedDiscError::Load)?;
        data_total = data_total
            .checked_add(member.size)
            .ok_or(CachedDiscError::Load(PceCdLoadError::DataTooLarge(
                u64::MAX,
            )))?;
        if data_total > PCE_CD_DATA_BYTES_LIMIT as u64 {
            return Err(CachedDiscError::Load(PceCdLoadError::DataTooLarge(
                data_total,
            )));
        }
        resolved.push(name);
    }

    let base = cache.extracted_bytes.saturating_add(cue.size);
    progress.set_phase(PceCdPackageLoadPhase::ReadingData);
    progress.set_total_bytes(base.saturating_add(data_total));
    let mut sources = Vec::with_capacity(resolved.len());
    for name in &resolved {
        let member = cache
            .manifest
            .members
            .iter()
            .find(|member| member.name == name.as_str())
            .ok_or(CachedDiscError::Corrupt)?;
        sources.push(cached_file_source(cache, member)?);
    }
    let overlay_sources = sources.clone();
    let mut verified = 0_u64;
    let mut loaded = load_cached_cue_file_backed(&cue_bytes, &sheet, sources, |count| {
        check_cancelled(cancel)?;
        verified = verified.saturating_add(count);
        progress.update_decode_progress(base.saturating_add(verified), cancel);
        check_cancelled(cancel)
    })
    .map_err(file_backed_cache_error)?;
    check_cancelled(cancel).map_err(CachedDiscError::Load)?;
    if !apply_mods {
        return Ok(loaded);
    }
    let (dir, mods, selected_crc32) = pce_cd_mod_config(
        crc32fast::hash(&loaded.source_disc_sha256),
        loaded.content_crc32,
    );
    if !mods.iter().any(|entry| entry.enabled) {
        loaded.mod_crc32 = selected_crc32;
        return Ok(loaded);
    }
    if let Some(disc) = try_load_cached_cue_ppf_overlay(&sheet, overlay_sources, &dir, &mods)
        .map_err(file_backed_cache_error)?
    {
        loaded.disc = disc;
        loaded.mod_crc32 = selected_crc32;
        return Ok(loaded);
    }
    drop(loaded);

    progress.set_total_bytes(base.saturating_add(data_total.saturating_mul(2)));
    let mut completed = base.saturating_add(data_total);
    let mut files = Vec::with_capacity(resolved.len());
    for name in resolved {
        let member = cache
            .manifest
            .members
            .iter()
            .find(|member| member.name == name)
            .ok_or(CachedDiscError::Corrupt)?;
        let bytes = read_cached_member(cache, member, cancel, progress, completed)?;
        completed = completed.saturating_add(member.size);
        files.push(bytes);
    }
    check_cancelled(cancel).map_err(CachedDiscError::Load)?;
    build_disc_with_mods(cue_bytes, &sheet, files, true).map_err(CachedDiscError::Load)
}

fn cached_file_source(
    cache: &CacheEntry,
    member: &CachedMember,
) -> Result<CueFileSource, CachedDiscError> {
    let path = cache_member_path(&cache.path.join(CACHE_FILES_DIR), &member.name);
    let metadata = std::fs::symlink_metadata(&path).map_err(|_| CachedDiscError::Corrupt)?;
    if !metadata.is_file()
        || metadata.file_type().is_symlink()
        || metadata_is_reparse_point(&metadata)
        || metadata.len() != member.size
    {
        return Err(CachedDiscError::Corrupt);
    }
    Ok(CueFileSource {
        path,
        bytes: member.size,
        sha256: member.sha256,
    })
}

fn file_backed_cache_error(error: PceCdLoadError) -> CachedDiscError {
    match error {
        PceCdLoadError::ArchiveChanged | PceCdLoadError::BinUnreadable(_) => {
            CachedDiscError::Corrupt
        }
        error => CachedDiscError::Load(error),
    }
}

fn read_cached_member(
    cache: &CacheEntry,
    member: &CachedMember,
    cancel: &AtomicBool,
    progress: &PceCdPackageProgress,
    progress_base: u64,
) -> Result<Vec<u8>, CachedDiscError> {
    let path = cache_member_path(&cache.path.join(CACHE_FILES_DIR), &member.name);
    let metadata = std::fs::symlink_metadata(&path).map_err(|_| CachedDiscError::Corrupt)?;
    if !metadata.is_file()
        || metadata.file_type().is_symlink()
        || metadata_is_reparse_point(&metadata)
        || metadata.len() != member.size
    {
        return Err(CachedDiscError::Corrupt);
    }
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(member.size as usize)
        .map_err(|_| CachedDiscError::Load(PceCdLoadError::ArchiveAllocationFailed))?;
    let mut file = File::open(path).map_err(|_| CachedDiscError::Corrupt)?;
    let mut hasher = Sha256::new();
    let mut local = 0_u64;
    let mut buffer = [0; STREAM_BUFFER_BYTES];
    loop {
        check_cancelled(cancel).map_err(CachedDiscError::Load)?;
        let count = file
            .read(&mut buffer)
            .map_err(|_| CachedDiscError::Corrupt)?;
        if count == 0 {
            break;
        }
        local = local
            .checked_add(count as u64)
            .ok_or(CachedDiscError::Corrupt)?;
        if local > member.size {
            return Err(CachedDiscError::Corrupt);
        }
        bytes.extend_from_slice(&buffer[..count]);
        hasher.update(&buffer[..count]);
        progress.update_decode_progress(progress_base.saturating_add(local), cancel);
    }
    if local != member.size || <[u8; 32]>::from(hasher.finalize()) != member.sha256 {
        return Err(CachedDiscError::Corrupt);
    }
    Ok(bytes)
}

fn cache_member_path(root: &Path, name: &str) -> PathBuf {
    name.split('/')
        .fold(root.to_path_buf(), |path, component| path.join(component))
}

fn touch_cache_entry(path: &Path) {
    let marker = path.join(CACHE_COMPLETE_FILE);
    let Ok(bytes) = std::fs::read(&marker) else {
        return;
    };
    let _ = std::fs::write(marker, bytes);
}

fn prune_cache(cache_root: &Path, keep: Option<&Path>) {
    let Ok(entries) = std::fs::read_dir(cache_root) else {
        return;
    };
    let mut complete = entries
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            let metadata = std::fs::symlink_metadata(&path).ok()?;
            if !metadata.is_dir() || metadata.file_type().is_symlink() {
                return None;
            }
            let marker = path.join(CACHE_COMPLETE_FILE);
            let modified = std::fs::metadata(marker).ok()?.modified().ok()?;
            Some((modified, path))
        })
        .collect::<Vec<_>>();
    complete.sort_by_key(|(modified, _)| *modified);
    while complete.len() > CACHE_MAX_COMPLETE_ENTRIES {
        let index = complete
            .iter()
            .position(|(_, path)| keep != Some(path.as_path()));
        let Some(index) = index else {
            break;
        };
        let (_, path) = complete.remove(index);
        remove_cache_entry(cache_root, &path);
    }
}

fn cleanup_stale_staging(cache_root: &Path) {
    let Ok(entries) = std::fs::read_dir(cache_root) else {
        return;
    };
    let now = SystemTime::now();
    for entry in entries.flatten() {
        let name = entry.file_name();
        if !name.to_string_lossy().contains(".tmp-") {
            continue;
        }
        let path = entry.path();
        let stale = std::fs::symlink_metadata(&path)
            .ok()
            .and_then(|metadata| metadata.modified().ok())
            .and_then(|modified| now.duration_since(modified).ok())
            .is_some_and(|age| age.as_secs() >= 24 * 60 * 60);
        if stale {
            remove_cache_entry(cache_root, &path);
        }
    }
}

fn remove_cache_entry(cache_root: &Path, path: &Path) {
    if path.parent() != Some(cache_root) {
        return;
    }
    let Ok(metadata) = std::fs::symlink_metadata(path) else {
        return;
    };
    if metadata.file_type().is_symlink() || metadata.is_file() {
        let _ = std::fs::remove_file(path);
    } else if metadata.is_dir() {
        let Ok(resolved_root) = std::fs::canonicalize(cache_root) else {
            return;
        };
        let Ok(resolved_target) = std::fs::canonicalize(path) else {
            return;
        };
        if resolved_target != resolved_root && resolved_target.parent() == Some(&resolved_root) {
            let _ = std::fs::remove_dir_all(resolved_target);
        }
    }
}

#[cfg(windows)]
fn metadata_is_reparse_point(metadata: &std::fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;

    metadata.file_attributes() & WINDOWS_REPARSE_POINT != 0
}

#[cfg(not(windows))]
fn metadata_is_reparse_point(_metadata: &std::fs::Metadata) -> bool {
    false
}

fn cache_io_error(error: impl std::fmt::Display) -> PceCdLoadError {
    PceCdLoadError::Archive(format!("CD cache: {error}"))
}

pub(crate) fn load_7z_rom_entry_with_control(
    path: &Path,
    entry_index: usize,
    cancel: &AtomicBool,
    progress: &PceCdPackageProgress,
    decoder_memory_limit_mib: usize,
) -> Result<(PathBuf, Vec<u8>, ActiveSystem), PceCdLoadError> {
    check_cancelled(cancel)?;
    progress.set_phase(PceCdPackageLoadPhase::Inspecting);
    let (mut reader, manifest) = open_validated(path, decoder_memory_limit_mib)?;
    if !manifest.cue_names.is_empty() {
        return Err(PceCdLoadError::MultipleArchiveCues);
    }
    let member = manifest
        .entries
        .iter()
        .find(|member| member.index == entry_index)
        .ok_or_else(|| PceCdLoadError::ArchiveMemberMissing(format!("#{entry_index}")))?;
    let system = ActiveSystem::from_path(Path::new(&member.name))
        .ok_or_else(|| PceCdLoadError::ArchiveMemberMissing(member.name.clone()))?;
    validate_regular_member(member)?;
    if member.size > GENERIC_ROM_BYTES_LIMIT {
        return Err(PceCdLoadError::DataTooLarge(member.size));
    }
    let name = member.name.clone();
    let decoded = manifest
        .entries
        .iter()
        .try_fold(0_u64, |total, member| total.checked_add(member.size))
        .ok_or(PceCdLoadError::ArchiveDecodedLimit)?;
    progress.set_total_bytes(decoded);
    progress.set_completed_bytes(0);
    progress.set_phase(PceCdPackageLoadPhase::ReadingRom);
    let mut retained = decode_pass(
        &mut reader,
        &manifest,
        &BTreeSet::from([name.clone()]),
        cancel,
        progress,
        DecodePassPolicy {
            progress_base: 0,
            decoded_bytes_limit: SEVEN_ZIP_DECODED_BYTES_LIMIT,
        },
    )?;
    let bytes = retained
        .remove(&name)
        .ok_or_else(|| PceCdLoadError::ArchiveMemberMissing(name.clone()))?;
    check_cancelled(cancel)?;
    Ok((virtual_member_path(path, &name), bytes, system))
}

fn open_validated(
    path: &Path,
    decoder_memory_limit_mib: usize,
) -> Result<(ArchiveReader<File>, ArchiveManifest), PceCdLoadError> {
    let metadata = std::fs::metadata(path)
        .map_err(|_| PceCdLoadError::ArchiveUnreadable(path.to_path_buf()))?;
    if !metadata.is_file() {
        return Err(PceCdLoadError::ArchiveUnreadable(path.to_path_buf()));
    }
    if metadata.len() > SEVEN_ZIP_ARCHIVE_BYTES_LIMIT {
        return Err(PceCdLoadError::ArchiveTooLarge(metadata.len()));
    }
    let limits = ArchiveLimits {
        max_header_bytes: PCE_CD_7Z_METADATA_CANCEL_BOUND_BYTES,
        max_decoder_memory_kb: decoder_memory_limit_mib.saturating_mul(1024),
        max_files: PCE_CD_7Z_ENTRY_LIMIT,
        max_blocks: PCE_CD_7Z_ENTRY_LIMIT,
        max_pack_streams: PCE_CD_7Z_ENTRY_LIMIT,
        max_substreams: PCE_CD_7Z_ENTRY_LIMIT,
        max_coders_per_block: 32,
        max_streams_per_block: PCE_CD_7Z_ENTRY_LIMIT,
        max_property_bytes: PCE_CD_CUE_BYTES_LIMIT,
        max_name_bytes: PCE_CD_CUE_BYTES_LIMIT,
    };
    let mut reader = ArchiveReader::open_with_limits(path, Password::empty(), limits)
        .map_err(map_sevenz_error)?;
    reader.set_thread_count(1);
    let archive = reader.archive();
    if archive.files.len() > PCE_CD_7Z_ENTRY_LIMIT {
        return Err(PceCdLoadError::TooManyArchiveEntries(archive.files.len()));
    }
    for block in &archive.blocks {
        for coder in &block.coders {
            let method = EncoderMethod::by_id(coder.encoder_method_id()).ok_or_else(|| {
                PceCdLoadError::ArchiveCodecUnsupported(format!(
                    "{:02x?}",
                    coder.encoder_method_id()
                ))
            })?;
            if !matches!(
                method,
                EncoderMethod::COPY | EncoderMethod::LZMA | EncoderMethod::LZMA2
            ) {
                return Err(PceCdLoadError::ArchiveCodecUnsupported(
                    method.name().to_owned(),
                ));
            }
        }
    }

    let mut entries = Vec::with_capacity(archive.files.len());
    let mut exact = BTreeSet::new();
    let mut folded = BTreeSet::new();
    let mut cue_names = Vec::new();
    let mut decoded = 0_u64;
    for (index, entry) in archive.files.iter().enumerate() {
        let name = normalize_portable_path(entry.name())
            .map_err(|_| PceCdLoadError::UnsafeArchiveEntry(entry.name().to_owned()))?;
        if !exact.insert(name.clone()) || !folded.insert(name.to_ascii_lowercase()) {
            return Err(PceCdLoadError::DuplicateArchiveEntry(name));
        }
        decoded = decoded
            .checked_add(entry.size())
            .ok_or(PceCdLoadError::ArchiveDecodedLimit)?;
        if decoded > SEVEN_ZIP_DECODED_BYTES_LIMIT {
            return Err(PceCdLoadError::ArchiveDecodedLimit);
        }
        let member = Member {
            index,
            name: name.clone(),
            size: entry.size(),
            is_directory: entry.is_directory(),
            is_anti: entry.is_anti_item(),
            is_link: is_link(entry),
            has_stream: entry.has_stream(),
            crc_checked: archive.entry_has_verifiable_crc(index),
        };
        if member.has_stream && !member.crc_checked {
            return Err(PceCdLoadError::ArchiveCrcRequired(name));
        }
        if extension_is(&name, "cue") && !member.is_directory && !member.is_anti {
            validate_cue_member(&member)?;
            cue_names.push(name.clone());
        }
        entries.push(member);
    }
    Ok((reader, ArchiveManifest { cue_names, entries }))
}

fn unique_cue_name(manifest: &ArchiveManifest) -> Result<&str, PceCdLoadError> {
    match manifest.cue_names.as_slice() {
        [] => Err(PceCdLoadError::NoArchiveCue),
        [name] => Ok(name),
        _ => Err(PceCdLoadError::MultipleArchiveCues),
    }
}

fn rom_entries(manifest: &ArchiveManifest) -> Vec<ArchiveRomEntry> {
    manifest
        .entries
        .iter()
        .filter_map(|member| {
            if member.is_directory || member.is_anti || member.is_link || !member.has_stream {
                return None;
            }
            Some(ArchiveRomEntry {
                index: member.index,
                name: member.name.clone(),
                system: ActiveSystem::from_path(Path::new(&member.name))?,
                uncompressed_size: member.size,
            })
        })
        .collect()
}

fn decode_pass(
    reader: &mut ArchiveReader<File>,
    manifest: &ArchiveManifest,
    targets: &BTreeSet<String>,
    cancel: &AtomicBool,
    progress: &PceCdPackageProgress,
    policy: DecodePassPolicy,
) -> Result<BTreeMap<String, Vec<u8>>, PceCdLoadError> {
    let mut retained = BTreeMap::new();
    let mut decoded = 0_u64;
    let mut failure = None;
    let result = reader.for_each_entries(|entry, input| {
        if failure.is_some() {
            return Err(cancel_error());
        }
        if cancel.load(Ordering::Acquire) {
            failure = Some(PceCdLoadError::ArchiveCancelled);
            return Err(cancel_error());
        }
        let name = match normalize_portable_path(entry.name()) {
            Ok(name) => name,
            Err(()) => {
                failure = Some(PceCdLoadError::UnsafeArchiveEntry(entry.name().to_owned()));
                return Err(cancel_error());
            }
        };
        let expected = match manifest.entries.iter().find(|member| member.name == name) {
            Some(member) => member,
            None => {
                failure = Some(PceCdLoadError::ArchiveChanged);
                return Err(cancel_error());
            }
        };
        let retain = targets.contains(&name);
        let mut bytes = if retain {
            let mut bytes = Vec::new();
            if bytes.try_reserve_exact(expected.size as usize).is_err() {
                failure = Some(PceCdLoadError::ArchiveAllocationFailed);
                return Err(cancel_error());
            }
            Some(bytes)
        } else {
            None
        };
        let mut local = 0_u64;
        let mut buffer = [0; STREAM_BUFFER_BYTES];
        loop {
            if cancel.load(Ordering::Acquire) {
                failure = Some(PceCdLoadError::ArchiveCancelled);
                return Err(cancel_error());
            }
            let count = input.read(&mut buffer)?;
            if count == 0 {
                break;
            }
            local = match local.checked_add(count as u64) {
                Some(local) => local,
                None => {
                    failure = Some(PceCdLoadError::ArchiveDecodedLimit);
                    return Err(cancel_error());
                }
            };
            decoded = match decoded.checked_add(count as u64) {
                Some(decoded) => decoded,
                None => {
                    failure = Some(PceCdLoadError::ArchiveDecodedLimit);
                    return Err(cancel_error());
                }
            };
            if decoded > policy.decoded_bytes_limit {
                failure = Some(PceCdLoadError::ArchiveDecodedLimit);
                return Err(cancel_error());
            }
            progress.update_decode_progress(policy.progress_base.saturating_add(decoded), cancel);
            if let Some(bytes) = bytes.as_mut() {
                bytes.extend_from_slice(&buffer[..count]);
            }
        }
        if local != expected.size {
            failure = Some(PceCdLoadError::ArchiveMemberSizeMismatch(name));
            return Err(cancel_error());
        }
        if let Some(bytes) = bytes {
            retained.insert(name, bytes);
        }
        Ok(true)
    });
    if let Some(error) = failure {
        return Err(error);
    }
    result.map_err(map_sevenz_error)?;
    if targets.iter().any(|name| !retained.contains_key(name)) {
        return Err(PceCdLoadError::ArchiveChanged);
    }
    Ok(retained)
}

fn check_cancelled(cancel: &AtomicBool) -> Result<(), PceCdLoadError> {
    if cancel.load(Ordering::Acquire) {
        Err(PceCdLoadError::ArchiveCancelled)
    } else {
        Ok(())
    }
}

fn resolve_reference(
    manifest: &ArchiveManifest,
    cue_name: &str,
    reference: &str,
) -> Result<String, PceCdLoadError> {
    let prefix = cue_name.rsplit_once('/').map_or("", |(prefix, _)| prefix);
    let candidate = if prefix.is_empty() {
        reference.to_owned()
    } else {
        format!("{prefix}/{reference}")
    };
    if manifest.entries.iter().any(|entry| entry.name == candidate) {
        return Ok(candidate);
    }
    let mut matches = manifest
        .entries
        .iter()
        .filter(|entry| entry.name.eq_ignore_ascii_case(&candidate));
    let Some(found) = matches.next() else {
        return Err(PceCdLoadError::ArchiveMemberMissing(candidate));
    };
    if matches.next().is_some() {
        return Err(PceCdLoadError::DuplicateArchiveEntry(candidate));
    }
    Ok(found.name.clone())
}

fn validate_cue_member(member: &Member) -> Result<(), PceCdLoadError> {
    validate_regular_member(member)?;
    if member.size > PCE_CD_CUE_BYTES_LIMIT as u64 {
        return Err(PceCdLoadError::CueTooLarge(member.size));
    }
    Ok(())
}

fn validate_data_member(member: &Member) -> Result<(), PceCdLoadError> {
    validate_regular_member(member)?;
    if member.size > PCE_CD_DATA_BYTES_LIMIT as u64 {
        return Err(PceCdLoadError::DataTooLarge(member.size));
    }
    Ok(())
}

fn validate_regular_member(member: &Member) -> Result<(), PceCdLoadError> {
    if member.is_link {
        return Err(PceCdLoadError::ArchiveLinkUnsupported(member.name.clone()));
    }
    if member.is_directory || member.is_anti || !member.has_stream {
        return Err(PceCdLoadError::ArchiveMemberMissing(member.name.clone()));
    }
    if !member.crc_checked {
        return Err(PceCdLoadError::ArchiveCrcRequired(member.name.clone()));
    }
    Ok(())
}

fn is_link(entry: &ArchiveEntry) -> bool {
    if !entry.has_windows_attributes {
        return false;
    }
    let attributes = entry.windows_attributes();
    attributes & WINDOWS_REPARSE_POINT != 0
        || ((attributes >> 16) & UNIX_FILE_TYPE_MASK) == UNIX_SYMBOLIC_LINK
}

fn extension_is(name: &str, expected: &str) -> bool {
    name.rsplit_once('.')
        .is_some_and(|(_, extension)| extension.eq_ignore_ascii_case(expected))
}

fn virtual_member_path(archive: &Path, member_name: &str) -> PathBuf {
    member_name
        .split('/')
        .fold(archive.to_path_buf(), |path, component| {
            path.join(component)
        })
}

fn cancel_error() -> SevenzError {
    std::io::Error::other("PC Engine CD package load stopped").into()
}

fn map_sevenz_error(error: SevenzError) -> PceCdLoadError {
    match error {
        SevenzError::MaxMemLimited { max_kb, actaul_kb } => PceCdLoadError::ArchiveMemoryLimit {
            allowed_mib: max_kb.div_ceil(1024),
            required_mib: actaul_kb.div_ceil(1024),
        },
        SevenzError::ChecksumVerificationFailed | SevenzError::NextHeaderCrcMismatch => {
            PceCdLoadError::ArchiveChecksumMismatch
        }
        SevenzError::UnsupportedCompressionMethod(method) => {
            PceCdLoadError::ArchiveCodecUnsupported(method)
        }
        SevenzError::PasswordRequired => {
            PceCdLoadError::ArchiveCodecUnsupported("encrypted".to_owned())
        }
        other if format!("{other:?}").contains("ChecksumVerificationFailed") => {
            PceCdLoadError::ArchiveChecksumMismatch
        }
        other => PceCdLoadError::Archive(other.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;
    use crate::emu_backend::{BackendLoadConfig, EmuBackend, pce_cd::load_direct_cue};
    use crate::emu_core_trait::EmulatorCore;
    use sevenz_rust2::{
        ArchiveEntry, ArchiveWriter, EncoderConfiguration, EncoderMethod, SourceReader,
        encoder_options::Lzma2Options,
    };

    fn temp_archive(name: &str, entries: &[(&str, Vec<u8>)], solid: bool) -> PathBuf {
        temp_archive_with_methods(
            name,
            entries,
            solid,
            vec![EncoderConfiguration::new(EncoderMethod::LZMA2)],
        )
    }

    fn temp_archive_with_methods(
        name: &str,
        entries: &[(&str, Vec<u8>)],
        solid: bool,
        methods: Vec<EncoderConfiguration>,
    ) -> PathBuf {
        let path =
            std::env::temp_dir().join(format!("zeff-pce-cd-7z-{}-{name}.7z", std::process::id()));
        let mut writer = ArchiveWriter::create(&path).unwrap();
        writer.set_content_methods(methods);
        if solid {
            writer
                .push_archive_entries(
                    entries
                        .iter()
                        .map(|(name, _)| ArchiveEntry::new_file(name))
                        .collect(),
                    entries
                        .iter()
                        .map(|(_, bytes)| SourceReader::new(Cursor::new(bytes.clone())))
                        .collect(),
                )
                .unwrap();
        } else {
            for (name, bytes) in entries {
                writer
                    .push_archive_entry(
                        ArchiveEntry::new_file(name),
                        Some(Cursor::new(bytes.clone())),
                    )
                    .unwrap();
            }
        }
        writer.finish().unwrap();
        path
    }

    fn cue() -> Vec<u8> {
        b"FILE \"disc.bin\" BINARY\nTRACK 01 MODE1/2048\nINDEX 01 00:00:00\n".to_vec()
    }

    fn temp_cache(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("zeff-pce-cd-cache-{}-{name}", std::process::id()))
    }

    fn load_with_cache(
        archive: &Path,
        cache: &Path,
        cancel: &AtomicBool,
        progress: &PceCdPackageProgress,
    ) -> Result<(PathBuf, LoadedPceCd), PceCdLoadError> {
        load_7z_cue_with_cache_root(
            archive,
            cancel,
            progress,
            DEFAULT_DECODER_MEMORY_LIMIT_MIB,
            false,
            cache,
        )
    }

    fn load_with_cache_and_mods(
        archive: &Path,
        cache: &Path,
        apply_mods: bool,
    ) -> Result<(PathBuf, LoadedPceCd), PceCdLoadError> {
        load_7z_cue_with_cache_root(
            archive,
            &AtomicBool::new(false),
            &PceCdPackageProgress::default(),
            DEFAULT_DECODER_MEMORY_LIMIT_MIB,
            apply_mods,
            cache,
        )
    }

    fn complete_cache_dirs(cache: &Path) -> Vec<PathBuf> {
        std::fs::read_dir(cache)
            .into_iter()
            .flatten()
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| path.join(CACHE_COMPLETE_FILE).is_file())
            .collect()
    }

    #[test]
    fn solid_and_non_solid_packages_match_direct_content_identity() {
        let bin = vec![0x5A; 2_048];
        let direct_root = std::env::temp_dir().join(format!(
            "zeff-pce-cd-direct-equivalence-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&direct_root).unwrap();
        std::fs::write(direct_root.join("disc.cue"), cue()).unwrap();
        std::fs::write(direct_root.join("disc.bin"), &bin).unwrap();
        let direct = load_direct_cue(&direct_root.join("disc.cue")).unwrap();

        for solid in [false, true] {
            let archive = temp_archive(
                if solid { "solid" } else { "non-solid" },
                &[("set/disc.bin", bin.clone()), ("set/disc.cue", cue())],
                solid,
            );
            let loaded = load_7z_cue(&archive).unwrap();
            assert_eq!(loaded.content_sha256, direct.content_sha256);
            assert_eq!(loaded.content_crc32, direct.content_crc32);
            assert_eq!(loaded.disc, direct.disc);
            assert_eq!(
                inspect_7z_cue_path(&archive).unwrap(),
                archive.join("set").join("disc.cue")
            );
        }
    }

    #[test]
    fn multifile_index_zero_payload_matches_direct_and_archive_identity() {
        let cue = b"FILE \"a.bin\" BINARY\nTRACK 01 MODE1/2048\nINDEX 01 00:00:00\nFILE \"b.bin\" BINARY\nTRACK 02 MODE1/2352\nINDEX 00 00:00:00\nINDEX 01 00:00:01\n";
        let first = vec![0x11; 2_048];
        let mut second = vec![0; 2 * 2_352];
        second[16] = 0x20;
        second[2_352 + 16] = 0x21;
        let direct_root = std::env::temp_dir().join(format!(
            "zeff-pce-cd-index-zero-equivalence-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&direct_root).unwrap();
        std::fs::write(direct_root.join("disc.cue"), cue).unwrap();
        std::fs::write(direct_root.join("a.bin"), &first).unwrap();
        std::fs::write(direct_root.join("b.bin"), &second).unwrap();
        let direct = load_direct_cue(&direct_root.join("disc.cue")).unwrap();

        for solid in [false, true] {
            let archive = temp_archive(
                if solid {
                    "index-zero-solid"
                } else {
                    "index-zero-non-solid"
                },
                &[
                    ("set/disc.cue", cue.to_vec()),
                    ("set/a.bin", first.clone()),
                    ("set/b.bin", second.clone()),
                ],
                solid,
            );
            let loaded = load_7z_cue(&archive).unwrap();
            assert_eq!(loaded.content_sha256, direct.content_sha256);
            assert_eq!(loaded.content_crc32, direct.content_crc32);
            assert_eq!(loaded.disc, direct.disc);
            let second_track = loaded.disc.track(2).unwrap();
            assert_eq!(
                loaded
                    .disc
                    .read_user_sector(second_track.stored_start_lba())
                    .unwrap()[0],
                0x20
            );
            assert_eq!(
                loaded
                    .disc
                    .read_user_sector(second_track.index1_lba())
                    .unwrap()[0],
                0x21
            );
        }
    }

    #[test]
    fn shared_file_virtual_pregap_matches_direct_identity_and_audio() {
        let cue = b"FILE \"disc.bin\" BINARY\nTRACK 01 MODE1/2352\nINDEX 01 00:00:00\nTRACK 02 AUDIO\nPREGAP 00:00:01\nINDEX 01 00:00:02\n";
        let mut bin = vec![0; 4 * 2_352];
        bin[16] = 0x11;
        bin[2_352 + 16] = 0x22;
        bin[2 * 2_352..2 * 2_352 + 2].copy_from_slice(&0x3456_i16.to_le_bytes());
        let direct_root = std::env::temp_dir().join(format!(
            "zeff-pce-cd-pregap-equivalence-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&direct_root).unwrap();
        std::fs::write(direct_root.join("disc.cue"), cue).unwrap();
        std::fs::write(direct_root.join("disc.bin"), &bin).unwrap();
        let direct = load_direct_cue(&direct_root.join("disc.cue")).unwrap();
        let archive = temp_archive(
            "shared-virtual-pregap",
            &[("disc.cue", cue.to_vec()), ("disc.bin", bin)],
            true,
        );
        let loaded = load_7z_cue(&archive).unwrap();

        assert_eq!(loaded.content_sha256, direct.content_sha256);
        assert_eq!(loaded.content_crc32, direct.content_crc32);
        assert_eq!(loaded.source_disc_sha256, direct.source_disc_sha256);
        assert_eq!(loaded.disc, direct.disc);
        assert!(loaded.disc.read_audio_sample(2, 0).is_err());
        assert_eq!(loaded.disc.read_audio_sample(3, 0).unwrap().0, 0x3456);
    }

    #[test]
    fn ordinary_solid_archive_lists_and_extracts_multiple_roms() {
        let first = vec![0x11; 32 * 1024];
        let second = vec![0x22; 64 * 1024];
        let archive = temp_archive(
            "ordinary-multi-rom",
            &[
                ("games/first.gb", first.clone()),
                ("games/second.gbc", second.clone()),
                ("notes/readme.txt", b"ignored".to_vec()),
            ],
            true,
        );
        let SevenZipContents::Roms(entries) =
            inspect_7z_contents(&archive, DEFAULT_DECODER_MEMORY_LIMIT_MIB).unwrap()
        else {
            panic!("ordinary ROM archive was classified as a CD set");
        };
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].name, "games/first.gb");
        assert_eq!(entries[1].name, "games/second.gbc");

        let progress = PceCdPackageProgress::default();
        let (virtual_path, bytes, system) = load_7z_rom_entry_with_control(
            &archive,
            entries[1].index,
            &AtomicBool::new(false),
            &progress,
            DEFAULT_DECODER_MEMORY_LIMIT_MIB,
        )
        .unwrap();
        assert_eq!(virtual_path, archive.join("games").join("second.gbc"));
        assert_eq!(bytes, second);
        assert_eq!(system, ActiveSystem::GameBoy);
        assert_eq!(progress.phase(), PceCdPackageLoadPhase::ReadingRom);
        assert_eq!(progress.completed_bytes(), progress.total_bytes());
    }

    #[test]
    fn ordinary_single_rom_archive_builds_a_backend_transactionally() {
        let mut rom = vec![0xEA; 0x2000];
        rom[..4].copy_from_slice(&[0xD4, 0xEA, 0x80, 0xFD]);
        rom[0x1FFE..].copy_from_slice(&0xE000_u16.to_le_bytes());
        let archive = temp_archive("ordinary-pce", &[("game.pce", rom)], true);
        let progress = PceCdPackageProgress::default();
        let prepared = crate::emu_backend::loader::prepare_seven_zip_backend(
            &archive,
            None,
            None,
            &BackendLoadConfig::default(),
            &AtomicBool::new(false),
            &progress,
        )
        .unwrap();
        let crate::emu_backend::loader::PreparedSevenZipBackend::Ready {
            rom_path,
            system,
            loaded,
        } = prepared
        else {
            panic!("single ROM unexpectedly requested a selection");
        };
        assert_eq!(rom_path, archive.join("game.pce"));
        assert_eq!(system, ActiveSystem::Pce);
        assert!(matches!(loaded.backend, EmuBackend::Pce(_)));
        assert_eq!(progress.phase(), PceCdPackageLoadPhase::Complete);
    }

    #[test]
    fn raw_lzma_package_matches_direct_content_identity() {
        let bin = vec![0xA5; 2_048];
        let direct_root = std::env::temp_dir().join(format!(
            "zeff-pce-cd-direct-lzma-equivalence-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&direct_root).unwrap();
        std::fs::write(direct_root.join("disc.cue"), cue()).unwrap();
        std::fs::write(direct_root.join("disc.bin"), &bin).unwrap();
        let direct = load_direct_cue(&direct_root.join("disc.cue")).unwrap();

        let archive = temp_archive_with_methods(
            "raw-lzma",
            &[("disc.bin", bin), ("disc.cue", cue())],
            true,
            vec![EncoderConfiguration::new(EncoderMethod::LZMA)],
        );
        let loaded = load_7z_cue(&archive).unwrap();

        assert_eq!(loaded.content_sha256, direct.content_sha256);
        assert_eq!(loaded.content_crc32, direct.content_crc32);
        assert_eq!(loaded.disc, direct.disc);
    }

    #[test]
    fn package_reference_uses_exact_then_unique_ascii_case_match() {
        let archive = temp_archive(
            "case-match",
            &[("set/DISC.BIN", vec![0; 2_048]), ("set/disc.cue", cue())],
            true,
        );
        assert!(load_7z_cue(&archive).is_ok());

        let duplicate = temp_archive(
            "case-collision",
            &[
                ("set/DISC.BIN", vec![0; 2_048]),
                ("set/disc.bin", vec![0; 2_048]),
                ("set/disc.cue", cue()),
            ],
            false,
        );
        assert!(matches!(
            inspect_7z_cue_path(&duplicate),
            Err(PceCdLoadError::DuplicateArchiveEntry(_))
        ));
    }

    #[test]
    fn missing_multiple_unsafe_and_cancelled_packages_are_typed() {
        let missing = temp_archive("no-cue", &[("disc.bin", vec![0; 2_048])], false);
        assert_eq!(
            inspect_7z_cue_path(&missing),
            Err(PceCdLoadError::NoArchiveCue)
        );

        let multiple = temp_archive(
            "multi-cue",
            &[
                ("a.cue", cue()),
                ("b.cue", cue()),
                ("disc.bin", vec![0; 2_048]),
            ],
            false,
        );
        assert_eq!(
            inspect_7z_cue_path(&multiple),
            Err(PceCdLoadError::MultipleArchiveCues)
        );

        let unsafe_path = temp_archive(
            "unsafe",
            &[("../disc.cue", cue()), ("disc.bin", vec![0; 2_048])],
            false,
        );
        assert!(matches!(
            inspect_7z_cue_path(&unsafe_path),
            Err(PceCdLoadError::UnsafeArchiveEntry(_))
        ));

        let valid = temp_archive(
            "cancel",
            &[("disc.bin", vec![0; 2_048]), ("disc.cue", cue())],
            true,
        );
        let cancel = AtomicBool::new(true);
        let progress = PceCdPackageProgress::default();
        assert_eq!(
            load_7z_cue_with_control(&valid, &cancel, &progress, DEFAULT_DECODER_MEMORY_LIMIT_MIB,)
                .err(),
            Some(PceCdLoadError::ArchiveCancelled)
        );
    }

    #[test]
    fn controlled_load_reports_complete_cached_preparation() {
        let valid = temp_archive(
            "progress",
            &[("disc.bin", vec![0; 2_048]), ("disc.cue", cue())],
            true,
        );
        let cancel = AtomicBool::new(false);
        let progress = PceCdPackageProgress::default();
        let (virtual_path, _) =
            load_7z_cue_with_control(&valid, &cancel, &progress, DEFAULT_DECODER_MEMORY_LIMIT_MIB)
                .unwrap();
        assert!(virtual_path.ends_with("disc.cue"));
        assert_eq!(progress.phase(), PceCdPackageLoadPhase::ReadingData);
        assert!(progress.total_bytes() > 0);
        assert_eq!(progress.completed_bytes(), progress.total_bytes());
    }

    #[test]
    fn cold_and_warm_cache_loads_preserve_identity_and_virtual_path() {
        let archive = temp_archive(
            "cache-cold-warm",
            &[
                ("set/disc.bin", vec![0x5A; 8 * 2_048]),
                ("set/disc.cue", cue()),
            ],
            true,
        );
        let cache = temp_cache("cold-warm");
        let _ = std::fs::remove_dir_all(&cache);
        let cold_progress = PceCdPackageProgress::default();
        let (cold_path, cold) =
            load_with_cache(&archive, &cache, &AtomicBool::new(false), &cold_progress).unwrap();
        let warm_progress = PceCdPackageProgress::default();
        let (warm_path, warm) =
            load_with_cache(&archive, &cache, &AtomicBool::new(false), &warm_progress).unwrap();

        assert_eq!(cold_path, archive.join("set").join("disc.cue"));
        assert_eq!(warm_path, cold_path);
        assert_eq!(warm.content_sha256, cold.content_sha256);
        assert_eq!(warm.content_crc32, cold.content_crc32);
        assert_eq!(warm.source_disc_sha256, cold.source_disc_sha256);
        assert_eq!(warm.disc, cold.disc);
        assert!(cold_progress.total_bytes() > warm_progress.total_bytes());
        assert_eq!(complete_cache_dirs(&cache).len(), 1);
        assert_eq!(
            crate::mods::mods_dir_for_rom(ActiveSystem::Pce, warm.content_crc32),
            crate::mods::mods_dir_for_rom(ActiveSystem::Pce, cold.content_crc32)
        );
        let _ = std::fs::remove_dir_all(cache);
    }

    #[test]
    fn unmodified_cache_loads_keep_file_backed_mutation_guards() {
        let mut bin = vec![0x5A; 4 * 2_048];
        bin[..8].copy_from_slice(
            &SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
                .to_le_bytes()[..8],
        );
        let archive = temp_archive(
            "cache-file-backed",
            &[("disc.bin", bin.clone()), ("disc.cue", cue())],
            true,
        );
        let cache = temp_cache("file-backed");
        let _ = std::fs::remove_dir_all(&cache);
        let (_, loaded) = load_with_cache_and_mods(&archive, &cache, false).unwrap();
        let entry = complete_cache_dirs(&cache).pop().unwrap();
        let data_path = entry.join(CACHE_FILES_DIR).join("disc.bin");
        let mut changed = std::fs::read(&data_path).unwrap();
        changed.extend_from_slice(&[0; 2_048]);
        std::fs::write(&data_path, changed).unwrap();

        assert!(loaded.disc.read_user_sector(0).is_err());
        drop(loaded);
        let (_, recovered) = load_with_cache_and_mods(&archive, &cache, false).unwrap();
        assert_eq!(recovered.disc.read_user_sector(0).unwrap()[0..8], bin[..8]);
        let _ = std::fs::remove_dir_all(cache);
    }

    #[test]
    fn source_metadata_change_creates_a_new_cache_identity() {
        let archive = temp_archive(
            "cache-source-change",
            &[("disc.bin", vec![0x11; 2_048]), ("disc.cue", cue())],
            true,
        );
        let cache = temp_cache("source-change");
        let _ = std::fs::remove_dir_all(&cache);
        let (_, first) = load_with_cache(
            &archive,
            &cache,
            &AtomicBool::new(false),
            &PceCdPackageProgress::default(),
        )
        .unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        let replacement = temp_archive(
            "cache-source-change-replacement",
            &[("disc.bin", vec![0x22; 2_048]), ("disc.cue", cue())],
            true,
        );
        std::fs::copy(replacement, &archive).unwrap();
        let (_, second) = load_with_cache(
            &archive,
            &cache,
            &AtomicBool::new(false),
            &PceCdPackageProgress::default(),
        )
        .unwrap();

        assert_ne!(first.content_sha256, second.content_sha256);
        assert_eq!(complete_cache_dirs(&cache).len(), 2);
        let _ = std::fs::remove_dir_all(cache);
    }

    #[test]
    fn corrupt_manifest_falls_back_to_clean_extraction() {
        let archive = temp_archive(
            "cache-corruption",
            &[("disc.bin", vec![0x33; 2_048]), ("disc.cue", cue())],
            true,
        );
        let cache = temp_cache("corruption");
        let _ = std::fs::remove_dir_all(&cache);
        let (_, expected) = load_with_cache(
            &archive,
            &cache,
            &AtomicBool::new(false),
            &PceCdPackageProgress::default(),
        )
        .unwrap();
        let entry = complete_cache_dirs(&cache).pop().unwrap();
        std::fs::write(entry.join(CACHE_COMPLETE_FILE), b"not json").unwrap();
        let (_, after_manifest) = load_with_cache(
            &archive,
            &cache,
            &AtomicBool::new(false),
            &PceCdPackageProgress::default(),
        )
        .unwrap();
        assert_eq!(after_manifest.disc, expected.disc);
        let _ = std::fs::remove_dir_all(cache);
    }

    #[test]
    fn same_length_cached_data_tamper_reextracts() {
        let archive = temp_archive(
            "cache-data-tamper",
            &[("disc.bin", vec![0x33; 2_048]), ("disc.cue", cue())],
            true,
        );
        let cache = temp_cache("data-tamper");
        let _ = std::fs::remove_dir_all(&cache);
        let (_, expected) = load_with_cache(
            &archive,
            &cache,
            &AtomicBool::new(false),
            &PceCdPackageProgress::default(),
        )
        .unwrap();
        let entry = complete_cache_dirs(&cache).pop().unwrap();
        std::fs::write(
            entry.join(CACHE_FILES_DIR).join("disc.bin"),
            vec![0x99; 2_048],
        )
        .unwrap();
        let (_, after_member) = load_with_cache(
            &archive,
            &cache,
            &AtomicBool::new(false),
            &PceCdPackageProgress::default(),
        )
        .unwrap();
        assert_eq!(after_member.disc, expected.disc);
        assert_eq!(after_member.content_sha256, expected.content_sha256);
        assert_eq!(after_member.disc.read_user_sector(0).unwrap()[0], 0x33);
        let _ = std::fs::remove_dir_all(cache);
    }

    #[test]
    fn cancelled_extraction_publishes_no_partial_cache() {
        let archive = temp_archive_with_methods(
            "cache-cancel",
            &[
                ("disc.bin", vec![0x5A; STREAM_BUFFER_BYTES * 4]),
                ("disc.cue", cue()),
            ],
            true,
            vec![EncoderConfiguration::new(EncoderMethod::LZMA)],
        );
        let cache = temp_cache("cancel");
        let _ = std::fs::remove_dir_all(&cache);
        let cancel = AtomicBool::new(false);
        let progress = PceCdPackageProgress::default();
        progress.set_cancel_after_completed_bytes(STREAM_BUFFER_BYTES as u64);

        assert_eq!(
            load_with_cache(&archive, &cache, &cancel, &progress).err(),
            Some(PceCdLoadError::ArchiveCancelled)
        );
        assert!(complete_cache_dirs(&cache).is_empty());
        assert!(
            std::fs::read_dir(&cache)
                .into_iter()
                .flatten()
                .flatten()
                .next()
                .is_none()
        );
        let _ = std::fs::remove_dir_all(cache);
    }

    #[test]
    fn cache_prunes_to_two_complete_entries() {
        let cache = temp_cache("prune");
        let _ = std::fs::remove_dir_all(&cache);
        for index in 0..3_u8 {
            let archive = temp_archive(
                &format!("cache-prune-{index}"),
                &[("disc.bin", vec![index; 2_048]), ("disc.cue", cue())],
                true,
            );
            load_with_cache(
                &archive,
                &cache,
                &AtomicBool::new(false),
                &PceCdPackageProgress::default(),
            )
            .unwrap();
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert_eq!(
            complete_cache_dirs(&cache).len(),
            CACHE_MAX_COMPLETE_ENTRIES
        );
        let _ = std::fs::remove_dir_all(cache);
    }

    #[test]
    fn live_file_source_survives_cache_pruning() {
        let cache = temp_cache("prune-live");
        let _ = std::fs::remove_dir_all(&cache);
        let first_archive = temp_archive(
            "cache-prune-live-first",
            &[("disc.bin", vec![0xA1; 2 * 2_048]), ("disc.cue", cue())],
            true,
        );
        let (_, first) = load_with_cache(
            &first_archive,
            &cache,
            &AtomicBool::new(false),
            &PceCdPackageProgress::default(),
        )
        .unwrap();
        std::thread::sleep(std::time::Duration::from_millis(5));
        for index in 0..2_u8 {
            let archive = temp_archive(
                &format!("cache-prune-live-{index}"),
                &[
                    ("disc.bin", vec![0xB0 + index; 2 * 2_048]),
                    ("disc.cue", cue()),
                ],
                true,
            );
            load_with_cache(
                &archive,
                &cache,
                &AtomicBool::new(false),
                &PceCdPackageProgress::default(),
            )
            .unwrap();
            std::thread::sleep(std::time::Duration::from_millis(5));
        }

        assert_eq!(first.disc.read_user_sector(0).unwrap()[0], 0xA1);
        drop(first);
        let _ = std::fs::remove_dir_all(cache);
    }

    #[test]
    fn cache_cleanup_rejects_root_and_out_of_root_targets() {
        let base = temp_cache("delete-containment");
        let root = base.join("root");
        let outside = base.join("outside");
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(outside.join("keep"), b"keep").unwrap();

        remove_cache_entry(&root, &root);
        remove_cache_entry(&root, &outside);

        assert!(root.is_dir());
        assert_eq!(std::fs::read(outside.join("keep")).unwrap(), b"keep");
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn controlled_load_cancels_at_a_decode_chunk_boundary() {
        let valid = temp_archive_with_methods(
            "mid-stream-cancel",
            &[
                ("disc.bin", vec![0x5A; STREAM_BUFFER_BYTES * 4]),
                ("disc.cue", cue()),
            ],
            true,
            vec![EncoderConfiguration::new(EncoderMethod::LZMA)],
        );
        let cancel = AtomicBool::new(false);
        let progress = PceCdPackageProgress::default();
        progress.set_cancel_after_completed_bytes(STREAM_BUFFER_BYTES as u64);

        assert_eq!(
            load_7z_cue_with_control(&valid, &cancel, &progress, DEFAULT_DECODER_MEMORY_LIMIT_MIB,)
                .err(),
            Some(PceCdLoadError::ArchiveCancelled)
        );
        assert!(cancel.load(Ordering::Acquire));
        assert!(progress.completed_bytes() >= STREAM_BUFFER_BYTES as u64);
        assert!(progress.completed_bytes() < progress.total_bytes());
    }

    #[test]
    fn parser_rejects_entry_counts_before_application_allocation() {
        let entries = (0..=PCE_CD_7Z_ENTRY_LIMIT)
            .map(|index| (format!("empty-{index}.bin"), Vec::new()))
            .collect::<Vec<_>>();
        let borrowed = entries
            .iter()
            .map(|(name, bytes)| (name.as_str(), bytes.clone()))
            .collect::<Vec<_>>();
        let archive = temp_archive("entry-limit", &borrowed, false);
        assert!(matches!(
            inspect_7z_cue_path(&archive),
            Err(PceCdLoadError::Archive(_))
        ));
    }

    #[test]
    fn unsupported_codec_memory_limit_and_crc_corruption_are_typed() {
        let entries = [("disc.bin", vec![0; 2_048]), ("disc.cue", cue())];
        let unsupported = temp_archive_with_methods(
            "unsupported-codec",
            &entries,
            false,
            vec![EncoderConfiguration::new(EncoderMethod::DELTA_FILTER)],
        );
        assert!(matches!(
            load_7z_cue(&unsupported),
            Err(PceCdLoadError::ArchiveCodecUnsupported(_))
        ));

        let mut options = Lzma2Options::from_level(1);
        options.set_dictionary_size(65 * 1024 * 1024);
        let excessive_memory =
            temp_archive_with_methods("memory-limit", &entries, false, vec![options.into()]);
        assert_eq!(
            load_7z_cue_with_control(
                &excessive_memory,
                &AtomicBool::new(false),
                &PceCdPackageProgress::default(),
                64,
            )
            .err(),
            Some(PceCdLoadError::ArchiveMemoryLimit {
                allowed_mib: 64,
                required_mib: 65,
            })
        );

        let corrupt = temp_archive_with_methods(
            "crc",
            &entries,
            true,
            vec![EncoderConfiguration::new(EncoderMethod::COPY)],
        );
        let mut bytes = std::fs::read(&corrupt).unwrap();
        bytes[32] ^= 0x80;
        std::fs::write(&corrupt, bytes).unwrap();
        assert_eq!(
            load_7z_cue(&corrupt).err(),
            Some(PceCdLoadError::ArchiveChecksumMismatch)
        );
    }

    #[test]
    fn packaged_loader_preserves_virtual_cue_and_real_source_paths() {
        let archive = temp_archive(
            "backend",
            &[("set/disc.bin", vec![0; 2_048]), ("set/disc.cue", cue())],
            true,
        );
        let system_card: &'static [u8] = Box::leak(vec![0; 262_144].into_boxed_slice());
        let cancel = AtomicBool::new(false);
        let progress = PceCdPackageProgress::default();
        let (cue_path, loaded) = crate::emu_backend::loader::prepare_pce_cd_7z_backend(
            &archive,
            None,
            &BackendLoadConfig {
                pce_cd_system_card_override: Some(system_card),
                pce_cd_system_card_sha256_override: Some(
                    zeff_firmware::PCE_SYSTEM_CARD_V3_USA_SHA256,
                ),
                pce_console_wiring: Some(zeff_pce_core::hardware::PceConsoleWiring::TurboGrafx16),
                ..BackendLoadConfig::default()
            },
            &cancel,
            &progress,
        )
        .unwrap();
        let EmuBackend::Pce(backend) = loaded.backend else {
            panic!("7z CUE loader returned a non-PCE backend");
        };
        assert_eq!(backend.rom_path(), cue_path);
        assert_eq!(backend.source_path(), archive);
        assert_eq!(
            backend.hucard_board(),
            zeff_pce_core::hardware::PceHuCardBoard::SystemCardV3
        );
        assert_eq!(progress.phase(), PceCdPackageLoadPhase::Complete);
    }

    #[test]
    #[ignore = "requires ZEFF_PCE_CD_AUDIO_7Z_SMOKE with a 96 MiB dictionary archive"]
    fn local_96_mib_dictionary_loads_mixed_mode_disc() {
        let archive = PathBuf::from(std::env::var("ZEFF_PCE_CD_AUDIO_7Z_SMOKE").unwrap());
        let cancel = AtomicBool::new(false);
        let progress = PceCdPackageProgress::default();
        let (_, loaded) = load_7z_cue_with_control(&archive, &cancel, &progress, 128).unwrap();
        assert!(
            loaded
                .disc
                .tracks()
                .iter()
                .any(|track| track.mode() == zeff_pce_core::hardware::CdTrackMode::Audio)
        );
    }
}
