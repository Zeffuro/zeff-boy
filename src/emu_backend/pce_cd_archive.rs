#![cfg(not(target_arch = "wasm32"))]

use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering};

use sevenz_rust2::{
    ArchiveEntry, ArchiveLimits, ArchiveReader, EncoderMethod, Error as SevenzError, Password,
};

use super::ActiveSystem;
use super::pce_cd::{
    LoadedPceCd, PCE_CD_CUE_BYTES_LIMIT, PCE_CD_DATA_BYTES_LIMIT, PceCdLoadError,
    build_disc_with_mods, normalize_portable_path, parse_cue_bytes,
};
use crate::rom_archive::ArchiveRomEntry;

const SEVEN_ZIP_ARCHIVE_BYTES_LIMIT: u64 = 8_u64 * 1024 * 1024 * 1024;
const SEVEN_ZIP_DECODED_BYTES_LIMIT: u64 = 8_u64 * 1024 * 1024 * 1024;
const PCE_CD_7Z_DECODED_BYTES_LIMIT: u64 = 1024 * 1024 * 1024;
const PCE_CD_7Z_ENTRY_LIMIT: usize = 256;
const PCE_CD_7Z_METADATA_CANCEL_BOUND_BYTES: usize = 64 * 1024 * 1024;
const STREAM_BUFFER_BYTES: usize = 64 * 1024;
const GENERIC_ROM_BYTES_LIMIT: u64 = 128 * 1024 * 1024;
const WINDOWS_REPARSE_POINT: u32 = 0x400;
const UNIX_FILE_TYPE_MASK: u32 = 0o170000;
const UNIX_SYMBOLIC_LINK: u32 = 0o120000;
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

    fn set_total_bytes(&self, total: u64) {
        self.total_bytes.store(total, Ordering::Release);
    }

    fn set_completed_bytes(&self, completed: u64) {
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
    check_cancelled(cancel)?;
    progress.set_phase(PceCdPackageLoadPhase::Inspecting);
    let (mut reader, manifest) = open_validated(path, decoder_memory_limit_mib)?;
    let cue_name = unique_cue_name(&manifest)?.to_owned();
    check_cancelled(cancel)?;
    let decoded_per_pass = manifest
        .entries
        .iter()
        .try_fold(0_u64, |total, member| total.checked_add(member.size))
        .ok_or(PceCdLoadError::ArchiveDecodedLimit)?;
    if decoded_per_pass > PCE_CD_7Z_DECODED_BYTES_LIMIT {
        return Err(PceCdLoadError::ArchiveDecodedLimit);
    }
    progress.set_total_bytes(decoded_per_pass.saturating_mul(2));
    progress.set_completed_bytes(0);
    progress.set_phase(PceCdPackageLoadPhase::ReadingCue);
    let cue_target = BTreeSet::from([cue_name.clone()]);
    let mut cue_members = decode_pass(
        &mut reader,
        &manifest,
        &cue_target,
        cancel,
        progress,
        DecodePassPolicy {
            progress_base: 0,
            decoded_bytes_limit: PCE_CD_7Z_DECODED_BYTES_LIMIT,
        },
    )?;
    check_cancelled(cancel)?;
    let cue_bytes = cue_members
        .remove(&cue_name)
        .ok_or_else(|| PceCdLoadError::ArchiveMemberMissing(cue_name.clone()))?;
    let sheet = parse_cue_bytes(&cue_bytes)?;

    let mut resolved = Vec::with_capacity(sheet.files.len());
    let mut targets = BTreeSet::from([cue_name.clone()]);
    let mut total = 0_u64;
    for file in &sheet.files {
        let name = resolve_reference(&manifest, &cue_name, &file.reference)?;
        let member = manifest
            .entries
            .iter()
            .find(|member| member.name == name)
            .ok_or_else(|| PceCdLoadError::ArchiveMemberMissing(name.clone()))?;
        validate_data_member(member)?;
        total = total
            .checked_add(member.size)
            .ok_or(PceCdLoadError::DataTooLarge(u64::MAX))?;
        if total > PCE_CD_DATA_BYTES_LIMIT as u64 {
            return Err(PceCdLoadError::DataTooLarge(total));
        }
        targets.insert(name.clone());
        resolved.push(name);
    }

    progress.set_phase(PceCdPackageLoadPhase::ReadingData);
    let mut members = decode_pass(
        &mut reader,
        &manifest,
        &targets,
        cancel,
        progress,
        DecodePassPolicy {
            progress_base: decoded_per_pass,
            decoded_bytes_limit: PCE_CD_7Z_DECODED_BYTES_LIMIT,
        },
    )?;
    check_cancelled(cancel)?;
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
    let virtual_path = virtual_member_path(path, &cue_name);
    let loaded = build_disc_with_mods(cue_bytes, &sheet, files, apply_mods)?;
    check_cancelled(cancel)?;
    Ok((virtual_path, loaded))
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
    fn controlled_load_reports_two_complete_decode_passes() {
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
