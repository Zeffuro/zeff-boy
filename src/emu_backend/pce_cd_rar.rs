#![cfg(not(target_arch = "wasm32"))]

use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use rars::{
    Archive, ArchiveFamily, ArchiveReadOptions, ArchiveReader, AttrSource, ExtractedEntryMeta,
};

use super::pce_cd::{
    LoadedPceCd, PCE_CD_CUE_BYTES_LIMIT, PCE_CD_DATA_BYTES_LIMIT, PceCdLoadError,
    build_disc_with_mods, normalize_portable_path, parse_cue_bytes,
};
use super::pce_cd_archive::{PceCdPackageLoadPhase, PceCdPackageProgress};

const RAR_ARCHIVE_BYTES_LIMIT: u64 = 8 * 1024 * 1024 * 1024;
const RAR_DECODED_BYTES_LIMIT: u64 = 1024 * 1024 * 1024;
const RAR_ENTRY_LIMIT: usize = 256;
const DOS_REPARSE_POINT: u64 = 0x400;
const UNIX_FILE_TYPE_MASK: u64 = 0o170000;
const UNIX_SYMBOLIC_LINK: u64 = 0o120000;

#[derive(Clone, Debug)]
struct RarMember {
    name: String,
    size: u64,
    is_directory: bool,
}

#[derive(Debug)]
struct RarManifest {
    members: Vec<RarMember>,
    cue_name: String,
    decoded_bytes: u64,
}

type SharedBytes = Arc<Mutex<Vec<u8>>>;

struct PassState {
    cancel: Arc<AtomicBool>,
    progress: Arc<PceCdPackageProgress>,
    completed: AtomicU64,
    maximum: u64,
    failure: Mutex<Option<PceCdLoadError>>,
}

impl PassState {
    fn fail(&self, error: PceCdLoadError) -> std::io::Error {
        let mut failure = self
            .failure
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if failure.is_none() {
            *failure = Some(error);
        }
        std::io::Error::new(std::io::ErrorKind::Interrupted, "RAR extraction stopped")
    }
}

struct PassWriter {
    state: Arc<PassState>,
    output: Option<SharedBytes>,
    expected: u64,
    written: u64,
}

impl Write for PassWriter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if self.state.cancel.load(Ordering::Acquire) {
            return Err(self.state.fail(PceCdLoadError::ArchiveCancelled));
        }
        let written = self
            .written
            .checked_add(bytes.len() as u64)
            .ok_or_else(|| self.state.fail(PceCdLoadError::ArchiveDecodedLimit))?;
        if written > self.expected {
            return Err(self.state.fail(PceCdLoadError::ArchiveChanged));
        }
        let completed = self
            .state
            .completed
            .fetch_add(bytes.len() as u64, Ordering::AcqRel)
            .saturating_add(bytes.len() as u64);
        if completed > self.state.maximum {
            return Err(self.state.fail(PceCdLoadError::ArchiveDecodedLimit));
        }
        if let Some(output) = &self.output {
            let mut output = output
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            output
                .try_reserve(bytes.len())
                .map_err(|_| self.state.fail(PceCdLoadError::ArchiveAllocationFailed))?;
            output.extend_from_slice(bytes);
        }
        self.written = written;
        self.state.progress.set_completed_bytes(completed);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

pub(crate) fn inspect_rar_cue_path(path: &Path) -> Result<PathBuf, PceCdLoadError> {
    let (_, manifest) = open_validated(path)?;
    Ok(virtual_member_path(path, &manifest.cue_name))
}

pub(crate) fn load_rar_cue_with_control_and_mods(
    path: &Path,
    cancel: Arc<AtomicBool>,
    progress: Arc<PceCdPackageProgress>,
    apply_mods: bool,
) -> Result<(PathBuf, LoadedPceCd), PceCdLoadError> {
    check_cancelled(&cancel)?;
    progress.set_phase(PceCdPackageLoadPhase::Inspecting);
    let (archive, manifest) = open_validated(path)?;
    let cue_target = BTreeSet::from([manifest.cue_name.clone()]);
    progress.set_phase(PceCdPackageLoadPhase::ReadingCue);
    progress.set_total_bytes(manifest.decoded_bytes.saturating_mul(2));
    progress.set_completed_bytes(0);
    let mut cue = extract_targets(
        &archive,
        &manifest,
        &cue_target,
        Arc::clone(&cancel),
        Arc::clone(&progress),
        0,
    )?;
    let cue_bytes = cue
        .remove(&manifest.cue_name)
        .ok_or_else(|| PceCdLoadError::ArchiveMemberMissing(manifest.cue_name.clone()))?;
    let sheet = parse_cue_bytes(&cue_bytes)?;

    let mut resolved = Vec::with_capacity(sheet.files.len());
    let mut targets = BTreeSet::new();
    let mut data_bytes = 0_u64;
    for file in &sheet.files {
        let name = resolve_reference(&manifest, &manifest.cue_name, &file.reference)?;
        let member = manifest
            .members
            .iter()
            .find(|member| member.name == name)
            .ok_or_else(|| PceCdLoadError::ArchiveMemberMissing(name.clone()))?;
        data_bytes = data_bytes
            .checked_add(member.size)
            .ok_or(PceCdLoadError::DataTooLarge(u64::MAX))?;
        if data_bytes > PCE_CD_DATA_BYTES_LIMIT as u64 {
            return Err(PceCdLoadError::DataTooLarge(data_bytes));
        }
        targets.insert(name.clone());
        resolved.push(name);
    }

    check_cancelled(&cancel)?;
    progress.set_phase(PceCdPackageLoadPhase::ReadingData);
    let mut extracted = extract_targets(
        &archive,
        &manifest,
        &targets,
        cancel,
        progress,
        manifest.decoded_bytes,
    )?;
    let files = resolved
        .into_iter()
        .map(|name| {
            extracted
                .remove(&name)
                .ok_or(PceCdLoadError::ArchiveMemberMissing(name))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let loaded = build_disc_with_mods(cue_bytes, &sheet, files, apply_mods)?;
    Ok((virtual_member_path(path, &manifest.cue_name), loaded))
}

fn open_validated(path: &Path) -> Result<(Archive, RarManifest), PceCdLoadError> {
    let metadata = std::fs::metadata(path)
        .map_err(|_| PceCdLoadError::ArchiveUnreadable(path.to_path_buf()))?;
    if !metadata.is_file() {
        return Err(PceCdLoadError::ArchiveUnreadable(path.to_path_buf()));
    }
    if metadata.len() > RAR_ARCHIVE_BYTES_LIMIT {
        return Err(PceCdLoadError::ArchiveTooLarge(metadata.len()));
    }
    let archive = ArchiveReader::read_path(path).map_err(map_rar_error)?;
    let mut members = Vec::new();
    let mut names = BTreeSet::new();
    let mut cue_names = Vec::new();
    let mut decoded_bytes = 0_u64;
    for member in archive.members() {
        if members.len() == RAR_ENTRY_LIMIT {
            return Err(PceCdLoadError::TooManyArchiveEntries(members.len() + 1));
        }
        let raw_name = std::str::from_utf8(member.meta.name_bytes())
            .map_err(|_| PceCdLoadError::UnsafeArchiveEntry(member.meta.name_lossy()))?;
        let name = normalize_portable_path(raw_name)
            .map_err(|_| PceCdLoadError::UnsafeArchiveEntry(raw_name.to_owned()))?;
        let key = name.to_ascii_lowercase();
        if !names.insert(key) {
            return Err(PceCdLoadError::DuplicateArchiveEntry(name));
        }
        if member.meta.is_encrypted {
            return Err(PceCdLoadError::ArchiveCodecUnsupported(
                "encrypted RAR member".to_owned(),
            ));
        }
        if member.meta.is_split_before || member.meta.is_split_after {
            return Err(PceCdLoadError::ArchiveCodecUnsupported(
                "multi-volume RAR member".to_owned(),
            ));
        }
        if is_link(
            member_attr_source(member.meta.family, member.meta.host_os),
            member.meta.file_attr,
        ) {
            return Err(PceCdLoadError::ArchiveLinkUnsupported(name));
        }
        if !member.meta.is_directory {
            decoded_bytes = decoded_bytes
                .checked_add(member.meta.unpacked_size)
                .ok_or(PceCdLoadError::ArchiveDecodedLimit)?;
            if decoded_bytes > RAR_DECODED_BYTES_LIMIT {
                return Err(PceCdLoadError::ArchiveDecodedLimit);
            }
            if Path::new(&name)
                .extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| extension.eq_ignore_ascii_case("cue"))
            {
                if member.meta.unpacked_size > PCE_CD_CUE_BYTES_LIMIT as u64 {
                    return Err(PceCdLoadError::CueTooLarge(member.meta.unpacked_size));
                }
                cue_names.push(name.clone());
            }
        }
        members.push(RarMember {
            name,
            size: member.meta.unpacked_size,
            is_directory: member.meta.is_directory,
        });
    }
    let cue_name = match cue_names.as_slice() {
        [] => return Err(PceCdLoadError::NoArchiveCue),
        [cue] => cue.clone(),
        _ => return Err(PceCdLoadError::MultipleArchiveCues),
    };
    Ok((
        archive,
        RarManifest {
            members,
            cue_name,
            decoded_bytes,
        },
    ))
}

fn extract_targets(
    archive: &Archive,
    manifest: &RarManifest,
    target_names: &BTreeSet<String>,
    cancel: Arc<AtomicBool>,
    progress: Arc<PceCdPackageProgress>,
    progress_base: u64,
) -> Result<BTreeMap<String, Vec<u8>>, PceCdLoadError> {
    let maximum = manifest.decoded_bytes.saturating_mul(2);
    let state = Arc::new(PassState {
        cancel,
        progress,
        completed: AtomicU64::new(progress_base),
        maximum,
        failure: Mutex::new(None),
    });
    let mut outputs = BTreeMap::new();
    for name in target_names {
        let member = manifest
            .members
            .iter()
            .find(|member| member.name == *name && !member.is_directory)
            .ok_or_else(|| PceCdLoadError::ArchiveMemberMissing(name.clone()))?;
        let capacity =
            usize::try_from(member.size).map_err(|_| PceCdLoadError::ArchiveAllocationFailed)?;
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(capacity)
            .map_err(|_| PceCdLoadError::ArchiveAllocationFailed)?;
        outputs.insert(name.clone(), Arc::new(Mutex::new(bytes)));
    }

    let mut open_failure = None;
    let result = archive.extract_to_with_options(ArchiveReadOptions::default(), |meta| {
        let name = extraction_name(meta, &mut open_failure)?;
        let member = manifest
            .members
            .iter()
            .find(|member| member.name == name)
            .ok_or(rars::Error::Cancelled)?;
        let output = outputs.get(&name).cloned();
        Ok(Box::new(PassWriter {
            state: Arc::clone(&state),
            output,
            expected: member.size,
            written: 0,
        }))
    });
    if let Some(error) = open_failure {
        return Err(error);
    }
    if let Some(error) = state
        .failure
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .take()
    {
        return Err(error);
    }
    result.map_err(map_rar_error)?;
    if state.completed.load(Ordering::Acquire)
        != progress_base.saturating_add(manifest.decoded_bytes)
    {
        return Err(PceCdLoadError::ArchiveChanged);
    }

    let mut result = BTreeMap::new();
    for (name, output) in outputs {
        let mut output = output
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let bytes = std::mem::take(&mut *output);
        let expected = manifest
            .members
            .iter()
            .find(|member| member.name == name)
            .map(|member| member.size)
            .ok_or(PceCdLoadError::ArchiveChanged)?;
        if bytes.len() as u64 != expected {
            return Err(PceCdLoadError::ArchiveMemberSizeMismatch(name));
        }
        result.insert(name, bytes);
    }
    Ok(result)
}

fn extraction_name(
    meta: &ExtractedEntryMeta,
    failure: &mut Option<PceCdLoadError>,
) -> rars::Result<String> {
    let raw = match std::str::from_utf8(meta.name_bytes()) {
        Ok(raw) => raw,
        Err(_) => {
            *failure = Some(PceCdLoadError::UnsafeArchiveEntry(meta.name_lossy()));
            return Err(rars::Error::Cancelled);
        }
    };
    match normalize_portable_path(raw) {
        Ok(name) => Ok(name),
        Err(()) => {
            *failure = Some(PceCdLoadError::UnsafeArchiveEntry(raw.to_owned()));
            Err(rars::Error::Cancelled)
        }
    }
}

fn resolve_reference(
    manifest: &RarManifest,
    cue_name: &str,
    reference: &str,
) -> Result<String, PceCdLoadError> {
    let cue_parent = Path::new(cue_name)
        .parent()
        .unwrap_or_else(|| Path::new(""));
    let joined = if cue_parent.as_os_str().is_empty() {
        reference.to_owned()
    } else {
        format!(
            "{}/{}",
            cue_parent.to_string_lossy().replace('\\', "/"),
            reference
        )
    };
    let normalized = normalize_portable_path(&joined)
        .map_err(|_| PceCdLoadError::UnsafeFileReference(reference.to_owned()))?;
    let matches = manifest
        .members
        .iter()
        .filter(|member| !member.is_directory && member.name.eq_ignore_ascii_case(&normalized))
        .collect::<Vec<_>>();
    match matches.as_slice() {
        [member] => Ok(member.name.clone()),
        [] => Err(PceCdLoadError::ArchiveMemberMissing(normalized)),
        _ => Err(PceCdLoadError::DuplicateArchiveEntry(normalized)),
    }
}

fn virtual_member_path(archive: &Path, member: &str) -> PathBuf {
    member
        .split('/')
        .fold(archive.to_path_buf(), |path, part| path.join(part))
}

fn is_link(source: AttrSource, attributes: u64) -> bool {
    match source {
        AttrSource::Dos => attributes & DOS_REPARSE_POINT != 0,
        AttrSource::Unix => attributes & UNIX_FILE_TYPE_MASK == UNIX_SYMBOLIC_LINK,
        _ => false,
    }
}

fn member_attr_source(family: ArchiveFamily, host_os: Option<u64>) -> AttrSource {
    match (family, host_os) {
        (ArchiveFamily::Rar13, _) => AttrSource::Dos,
        (ArchiveFamily::Rar15To40, Some(0 | 1 | 2 | 4)) | (ArchiveFamily::Rar50Plus, Some(0)) => {
            AttrSource::Dos
        }
        (ArchiveFamily::Rar15To40, Some(3 | 5)) | (ArchiveFamily::Rar50Plus, Some(1)) => {
            AttrSource::Unix
        }
        _ => AttrSource::Unknown,
    }
}

fn check_cancelled(cancel: &AtomicBool) -> Result<(), PceCdLoadError> {
    if cancel.load(Ordering::Acquire) {
        Err(PceCdLoadError::ArchiveCancelled)
    } else {
        Ok(())
    }
}

fn map_rar_error(error: rars::Error) -> PceCdLoadError {
    match error {
        rars::Error::NeedPassword
        | rars::Error::WrongPasswordOrCorruptData
        | rars::Error::UnsupportedEncryption { .. } => {
            PceCdLoadError::ArchiveCodecUnsupported("encrypted RAR archive".to_owned())
        }
        rars::Error::CrcMismatch { .. }
        | rars::Error::Crc32Mismatch { .. }
        | rars::Error::HashMismatch { .. } => PceCdLoadError::ArchiveChecksumMismatch,
        rars::Error::MemoryLimitExceeded {
            limit, required, ..
        }
        | rars::Error::Rar50BufferedDecodeLimitExceeded { limit, required } => {
            PceCdLoadError::ArchiveMemoryLimit {
                allowed_mib: usize::try_from(limit / (1024 * 1024)).unwrap_or(usize::MAX),
                required_mib: usize::try_from(required.div_ceil(1024 * 1024)).unwrap_or(usize::MAX),
            }
        }
        other => PceCdLoadError::Archive(other.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rars::rar50::{ArchiveEntry, Rar50Writer, WriterOptions};
    use rars::{ArchiveVersion, EntrySource, FeatureSet};

    fn archive_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("zeff-pce-rar-{}-{name}.rar", std::process::id()))
    }

    fn write_archive(path: &Path, entries: &[(&[u8], &[u8])]) {
        let entries = entries
            .iter()
            .map(|(name, data)| {
                ArchiveEntry::new(
                    name.to_vec(),
                    EntrySource::from_bytes(Arc::<[u8]>::from(data.to_vec())),
                )
            })
            .collect::<Vec<_>>();
        let bytes = Rar50Writer::new(
            WriterOptions::new(ArchiveVersion::Rar50, FeatureSet::store_only())
                .with_compression_level(0),
        )
        .entries(entries)
        .finish()
        .unwrap();
        std::fs::write(path, bytes).unwrap();
    }

    #[test]
    fn rar_cue_load_matches_direct_cue_identity() {
        let path = archive_path("equivalent");
        let cue = b"FILE \"disc.bin\" BINARY\n  TRACK 01 MODE1/2048\n    INDEX 01 00:00:00\n";
        let data = vec![0x5A; 2048 * 3];
        write_archive(&path, &[(b"disc.cue", cue), (b"disc.bin", &data)]);

        let cancel = Arc::new(AtomicBool::new(false));
        let progress = Arc::new(PceCdPackageProgress::default());
        let (virtual_path, loaded) =
            load_rar_cue_with_control_and_mods(&path, cancel, progress, false).unwrap();
        let expected = super::super::pce_cd::build_disc(
            cue.to_vec(),
            &parse_cue_bytes(cue).unwrap(),
            vec![data],
        )
        .unwrap();

        assert_eq!(virtual_path, path.join("disc.cue"));
        assert_eq!(loaded.disc, expected.disc);
        assert_eq!(loaded.content_sha256, expected.content_sha256);
        assert_eq!(loaded.source_disc_sha256, expected.source_disc_sha256);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn rar_rejects_ambiguous_cues_and_missing_members() {
        let ambiguous = archive_path("ambiguous");
        let cue = b"FILE \"disc.bin\" BINARY\n  TRACK 01 MODE1/2048\n    INDEX 01 00:00:00\n";
        write_archive(
            &ambiguous,
            &[
                (b"one.cue", cue),
                (b"two.cue", cue),
                (b"disc.bin", &[0; 2048]),
            ],
        );
        assert_eq!(
            inspect_rar_cue_path(&ambiguous),
            Err(PceCdLoadError::MultipleArchiveCues)
        );
        let _ = std::fs::remove_file(ambiguous);

        let missing = archive_path("missing");
        write_archive(&missing, &[(b"disc.cue", cue)]);
        let result = load_rar_cue_with_control_and_mods(
            &missing,
            Arc::new(AtomicBool::new(false)),
            Arc::new(PceCdPackageProgress::default()),
            false,
        );
        let Err(error) = result else {
            panic!("RAR with a missing data member unexpectedly loaded");
        };
        assert_eq!(
            error,
            PceCdLoadError::ArchiveMemberMissing("disc.bin".to_owned())
        );
        let _ = std::fs::remove_file(missing);
    }

    #[test]
    fn rar_load_honors_preexisting_cancellation() {
        let path = archive_path("cancel");
        let cue = b"FILE \"disc.bin\" BINARY\n  TRACK 01 MODE1/2048\n    INDEX 01 00:00:00\n";
        write_archive(&path, &[(b"disc.cue", cue), (b"disc.bin", &[0; 2048])]);
        let cancel = Arc::new(AtomicBool::new(true));
        let result = load_rar_cue_with_control_and_mods(
            &path,
            cancel,
            Arc::new(PceCdPackageProgress::default()),
            false,
        );
        assert!(matches!(result, Err(PceCdLoadError::ArchiveCancelled)));
        let _ = std::fs::remove_file(path);
    }
}
