#![cfg(not(target_arch = "wasm32"))]

use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
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
    try_load_cached_cue_ppf_overlay_byte_slices,
};
use crate::rom_archive::ArchiveRomEntry;

mod cache;
mod loading;
pub(super) mod ppf;

#[cfg(test)]
#[path = "pce_cd_archive/tests.rs"]
mod tests;

use cache::{
    CacheIdentity, CachedDiscError, SourceFingerprint, cache_key, extract_cache,
    load_cached_archive_ppf_disc, load_cached_disc, pce_cd_cache_root, prepare_cache, prune_cache,
    remove_cache_entry, source_fingerprint, touch_cache_entry, validate_cacheable_manifest,
};

use loading::DecodePassPolicy;
pub(crate) use loading::{
    SevenZipContents, inspect_7z_contents, inspect_7z_cue_candidates_with_archive_identity,
    inspect_7z_cue_members, inspect_7z_ppf_candidates_with_archive_identity,
    load_7z_cue_with_control_and_archive_identity, load_7z_cue_with_control_and_archive_ppf,
    load_7z_cue_with_control_and_mods, load_7z_rom_entry_with_control,
    load_7z_selected_cue_with_control_and_archive_identity,
    load_7z_selected_cue_with_control_and_archive_ppf,
};
pub(crate) use ppf::{
    PceCdArchivePpfCandidate, PceCdArchivePpfLoad, PceCdArchivePpfPatch,
    PceCdArchivePpfPatchIdentity,
};

#[cfg(test)]
pub(crate) use loading::{
    inspect_7z_cue_path, load_7z_cue, load_7z_cue_with_cache_root,
    load_7z_cue_with_cache_root_and_archive_identity_for_test, load_7z_cue_with_control,
};

#[cfg(feature = "profile-cores")]
pub(crate) use loading::profile_cache_load;

const SEVEN_ZIP_ARCHIVE_BYTES_LIMIT: u64 = 8_u64 * 1024 * 1024 * 1024;
const SEVEN_ZIP_DECODED_BYTES_LIMIT: u64 = 8_u64 * 1024 * 1024 * 1024;
const PCE_CD_7Z_DECODED_BYTES_LIMIT: u64 = 1024 * 1024 * 1024;
const PCE_CD_7Z_ENTRY_LIMIT: usize = 256;
const PCE_CD_7Z_METADATA_CANCEL_BOUND_BYTES: usize = 64 * 1024 * 1024;
const STREAM_BUFFER_BYTES: usize = 64 * 1024;
const GENERIC_ROM_BYTES_LIMIT: u64 = 128 * 1024 * 1024;
const CACHE_FORMAT_VERSION: u32 = 2;
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PceCdArchiveCueIdentity {
    pub(crate) source_sha256: [u8; 32],
    pub(crate) source_len: usize,
    pub(crate) cue_member_path_sha256: [u8; 32],
    pub(crate) selection: PceCdArchiveCueSelection,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PceCdArchiveCueSelection {
    Unique,
    Explicit,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PceCdArchiveCueCandidate {
    pub(crate) cue_member: String,
    pub(crate) identity: PceCdArchiveCueIdentity,
}

fn archive_cue_identity(
    source: SourceFingerprint,
    cue_name: &str,
    selection: PceCdArchiveCueSelection,
) -> Result<PceCdArchiveCueIdentity, PceCdLoadError> {
    let mut hasher = Sha256::new();
    hasher.update(b"zeff-pce-cd-7z-cue-member:v1\0");
    hasher.update(cue_name.as_bytes());
    Ok(PceCdArchiveCueIdentity {
        source_sha256: source.sha256,
        source_len: usize::try_from(source.size)
            .map_err(|_| PceCdLoadError::ArchiveTooLarge(source.size))?,
        cue_member_path_sha256: hasher.finalize().into(),
        selection,
    })
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

fn open_validated(
    path: &Path,
    decoder_memory_limit_mib: usize,
) -> Result<(ArchiveReader<File>, ArchiveManifest), PceCdLoadError> {
    let source =
        File::open(path).map_err(|_| PceCdLoadError::ArchiveUnreadable(path.to_path_buf()))?;
    open_validated_source(source, path, decoder_memory_limit_mib)
}

fn open_validated_with_source_fingerprint(
    path: &Path,
    decoder_memory_limit_mib: usize,
    cancel: &AtomicBool,
    progress: &PceCdPackageProgress,
) -> Result<(ArchiveReader<File>, ArchiveManifest, SourceFingerprint), PceCdLoadError> {
    let mut source =
        File::open(path).map_err(|_| PceCdLoadError::ArchiveUnreadable(path.to_path_buf()))?;
    validate_source_metadata(&source, path)?;
    let source_fingerprint = source_fingerprint(&mut source, path, cancel, progress)?;
    let (reader, manifest) = open_validated_source(source, path, decoder_memory_limit_mib)?;
    Ok((reader, manifest, source_fingerprint))
}

fn open_validated_with_source_verifier(
    path: &Path,
    decoder_memory_limit_mib: usize,
    cancel: &AtomicBool,
    progress: &PceCdPackageProgress,
) -> Result<
    (
        ArchiveReader<File>,
        ArchiveManifest,
        SourceFingerprint,
        File,
    ),
    PceCdLoadError,
> {
    let mut source =
        File::open(path).map_err(|_| PceCdLoadError::ArchiveUnreadable(path.to_path_buf()))?;
    validate_source_metadata(&source, path)?;
    let verifier = source
        .try_clone()
        .map_err(|_| PceCdLoadError::ArchiveUnreadable(path.to_path_buf()))?;
    let fingerprint = source_fingerprint(&mut source, path, cancel, progress)?;
    let (reader, manifest) = open_validated_source(source, path, decoder_memory_limit_mib)?;
    Ok((reader, manifest, fingerprint, verifier))
}

fn reauthenticate_source(
    source: &mut File,
    expected: SourceFingerprint,
    path: &Path,
    cancel: &AtomicBool,
    progress: &PceCdPackageProgress,
) -> Result<(), PceCdLoadError> {
    source
        .seek(SeekFrom::Start(0))
        .map_err(|_| PceCdLoadError::ArchiveUnreadable(path.to_path_buf()))?;
    let actual = source_fingerprint(source, path, cancel, progress)?;
    if actual != expected {
        return Err(PceCdLoadError::ArchiveChanged);
    }
    Ok(())
}

fn open_validated_source(
    source: File,
    path: &Path,
    decoder_memory_limit_mib: usize,
) -> Result<(ArchiveReader<File>, ArchiveManifest), PceCdLoadError> {
    validate_source_metadata(&source, path)?;
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
    let mut reader = ArchiveReader::new_with_limits(source, Password::empty(), limits)
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

fn validate_source_metadata(source: &File, path: &Path) -> Result<(), PceCdLoadError> {
    let metadata = source
        .metadata()
        .map_err(|_| PceCdLoadError::ArchiveUnreadable(path.to_path_buf()))?;
    if !metadata.is_file() {
        return Err(PceCdLoadError::ArchiveUnreadable(path.to_path_buf()));
    }
    if metadata.len() > SEVEN_ZIP_ARCHIVE_BYTES_LIMIT {
        return Err(PceCdLoadError::ArchiveTooLarge(metadata.len()));
    }
    Ok(())
}

fn unique_cue_name(manifest: &ArchiveManifest) -> Result<&str, PceCdLoadError> {
    select_normalized_cue_name(&manifest.cue_names, None)
}

pub(super) fn select_normalized_cue_name<'a>(
    cue_names: &'a [String],
    selected: Option<&str>,
) -> Result<&'a str, PceCdLoadError> {
    let Some(selected) = selected else {
        return match cue_names {
            [] => Err(PceCdLoadError::NoArchiveCue),
            [name] => Ok(name),
            _ => Err(PceCdLoadError::MultipleArchiveCues),
        };
    };
    if cue_names.is_empty() {
        return Err(PceCdLoadError::NoArchiveCue);
    }
    let normalized = normalize_portable_path(selected)
        .map_err(|_| PceCdLoadError::UnsafeArchiveEntry(selected.to_owned()))?;
    if !extension_is(&normalized, "cue") {
        return Err(PceCdLoadError::ArchiveMemberMissing(normalized));
    }
    match cue_names
        .iter()
        .filter(|name| name.eq_ignore_ascii_case(&normalized))
        .collect::<Vec<_>>()
        .as_slice()
    {
        [] => Err(PceCdLoadError::ArchiveMemberMissing(normalized)),
        [name] => Ok(name.as_str()),
        _ => Err(PceCdLoadError::DuplicateArchiveEntry(normalized)),
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
