#![cfg(not(target_arch = "wasm32"))]

use std::collections::{BTreeMap, BTreeSet};
use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use sha2::{Digest, Sha256};

use super::pce_cd::{
    LoadedPceCd, PCE_CD_CUE_BYTES_LIMIT, PCE_CD_DATA_BYTES_LIMIT, PceCdLoadError,
    build_disc_with_mods, normalize_portable_path, parse_cue_bytes,
};
use super::pce_cd_archive::ppf::{
    ArchivePpfBuildInput, ArchivePpfMember, build_archive_ppf_load, discover_archive_ppf_members,
    patch_identities, patches_from_bytes,
};
use super::pce_cd_archive::{
    PceCdArchiveCueCandidate, PceCdArchiveCueIdentity, PceCdArchiveCueSelection,
    PceCdArchivePpfCandidate, PceCdArchivePpfLoad, PceCdPackageLoadPhase, PceCdPackageProgress,
    select_normalized_cue_name,
};

#[cfg(test)]
#[path = "pce_cd_zip/tests.rs"]
mod tests;

const ZIP_ARCHIVE_BYTES_LIMIT: u64 = 1024 * 1024 * 1024;
const ZIP_DECODED_BYTES_LIMIT: u64 = 1024 * 1024 * 1024;
const ZIP_ENTRY_LIMIT: usize = 256;
const UNIX_FILE_TYPE_MASK: u32 = 0o170000;
const UNIX_SYMBOLIC_LINK: u32 = 0o120000;

#[derive(Clone, Debug)]
struct ZipMember {
    index: usize,
    name: String,
    size: u64,
}

#[derive(Debug)]
struct ZipManifest {
    members: Vec<ZipMember>,
    cue_names: Vec<String>,
    decoded_bytes: u64,
}

type OpenZip = zip::ZipArchive<Cursor<Vec<u8>>>;

pub(crate) fn inspect_zip_cue_members(path: &Path) -> Result<Vec<String>, PceCdLoadError> {
    let bytes = read_archive(path, &AtomicBool::new(false))?;
    let (_, manifest) = open_validated(bytes)?;
    Ok(manifest.cue_names)
}

pub(crate) fn zip_contains_cue(path: &Path) -> Result<bool, PceCdLoadError> {
    let file = std::fs::File::open(path)
        .map_err(|_| PceCdLoadError::ArchiveUnreadable(path.to_path_buf()))?;
    let size = file
        .metadata()
        .map_err(|_| PceCdLoadError::ArchiveUnreadable(path.to_path_buf()))?
        .len();
    if size > ZIP_ARCHIVE_BYTES_LIMIT {
        return Err(PceCdLoadError::ArchiveTooLarge(size));
    }
    let mut archive = zip::ZipArchive::new(file).map_err(map_zip_error)?;
    Ok(!validated_manifest(&mut archive)?.cue_names.is_empty())
}

pub(crate) fn inspect_zip_cue_candidates_with_archive_identity(
    path: &Path,
    cancel: &AtomicBool,
) -> Result<Vec<PceCdArchiveCueCandidate>, PceCdLoadError> {
    let bytes = read_archive(path, cancel)?;
    let source_sha256 = Sha256::digest(&bytes).into();
    let source_len = bytes.len();
    let (_, manifest) = open_validated(bytes)?;
    manifest
        .cue_names
        .into_iter()
        .map(|cue_member| {
            Ok(PceCdArchiveCueCandidate {
                identity: zip_cue_identity(
                    source_sha256,
                    source_len,
                    &cue_member,
                    PceCdArchiveCueSelection::Explicit,
                ),
                cue_member,
            })
        })
        .collect()
}

pub(crate) fn inspect_zip_ppf_candidates_with_archive_identity(
    path: &Path,
    cancel: &AtomicBool,
) -> Result<Vec<PceCdArchivePpfCandidate>, PceCdLoadError> {
    let bytes = read_archive(path, cancel)?;
    let source_sha256 = Sha256::digest(&bytes).into();
    let source_len = bytes.len();
    let (mut archive, manifest) = open_validated(bytes)?;
    let descriptors = ppf_members(&manifest);
    let mut candidates = Vec::with_capacity(manifest.cue_names.len());
    let mut targets = BTreeSet::new();
    for cue_name in &manifest.cue_names {
        if let Some(names) = discover_archive_ppf_members(cue_name, &descriptors)? {
            targets.extend(names.iter().cloned());
            candidates.push((cue_name.clone(), names));
        }
    }
    if candidates.is_empty() {
        return Err(PceCdLoadError::NoArchivePpfStack);
    }
    let progress = PceCdPackageProgress::default();
    let mut extracted = BTreeMap::new();
    let mut completed = 0_u64;
    for name in targets {
        let descriptor = member(&manifest, &name)?;
        let bytes = extract_member(&mut archive, descriptor, cancel, &progress, completed)?;
        completed = completed.saturating_add(bytes.len() as u64);
        extracted.insert(name, bytes);
    }
    candidates
        .into_iter()
        .map(|(cue_member, names)| {
            Ok(PceCdArchivePpfCandidate {
                identity: zip_cue_identity(
                    source_sha256,
                    source_len,
                    &cue_member,
                    PceCdArchiveCueSelection::Explicit,
                ),
                patches: patch_identities(&names, &mut extracted)?,
                cue_member,
            })
        })
        .collect()
}

pub(crate) fn load_zip_cue_with_control_and_mods(
    path: &Path,
    cancel: Arc<AtomicBool>,
    progress: Arc<PceCdPackageProgress>,
    apply_mods: bool,
) -> Result<(PathBuf, LoadedPceCd), PceCdLoadError> {
    load(path, None, cancel, progress, apply_mods).map(|(path, loaded, _)| (path, loaded))
}

pub(crate) fn load_zip_cue_with_control_and_archive_identity(
    path: &Path,
    cancel: Arc<AtomicBool>,
    progress: Arc<PceCdPackageProgress>,
    apply_mods: bool,
) -> Result<(PathBuf, LoadedPceCd, PceCdArchiveCueIdentity), PceCdLoadError> {
    load(path, None, cancel, progress, apply_mods)
}

pub(crate) fn load_zip_selected_cue_with_control_and_archive_identity(
    path: &Path,
    selected_cue_name: &str,
    cancel: Arc<AtomicBool>,
    progress: Arc<PceCdPackageProgress>,
    apply_mods: bool,
) -> Result<(PathBuf, LoadedPceCd, PceCdArchiveCueIdentity), PceCdLoadError> {
    load(path, Some(selected_cue_name), cancel, progress, apply_mods)
}

pub(crate) fn load_zip_cue_with_control_and_archive_ppf(
    path: &Path,
    cancel: Arc<AtomicBool>,
    progress: Arc<PceCdPackageProgress>,
) -> Result<PceCdArchivePpfLoad, PceCdLoadError> {
    load_archive_ppf(path, None, cancel, progress)
}

pub(crate) fn load_zip_selected_cue_with_control_and_archive_ppf(
    path: &Path,
    selected_cue_name: &str,
    cancel: Arc<AtomicBool>,
    progress: Arc<PceCdPackageProgress>,
) -> Result<PceCdArchivePpfLoad, PceCdLoadError> {
    load_archive_ppf(path, Some(selected_cue_name), cancel, progress)
}

fn load_archive_ppf(
    path: &Path,
    selected_cue_name: Option<&str>,
    cancel: Arc<AtomicBool>,
    progress: Arc<PceCdPackageProgress>,
) -> Result<PceCdArchivePpfLoad, PceCdLoadError> {
    check_cancelled(&cancel)?;
    progress.set_phase(PceCdPackageLoadPhase::Inspecting);
    let bytes = read_archive(path, &cancel)?;
    let source_sha256 = Sha256::digest(&bytes).into();
    let source_len = bytes.len();
    let (mut archive, manifest) = open_validated(bytes)?;
    let cue_name = select_normalized_cue_name(&manifest.cue_names, selected_cue_name)?.to_owned();
    let selection = if selected_cue_name.is_some() {
        PceCdArchiveCueSelection::Explicit
    } else {
        PceCdArchiveCueSelection::Unique
    };
    let identity = zip_cue_identity(source_sha256, source_len, &cue_name, selection);
    let patch_names = discover_archive_ppf_members(&cue_name, &ppf_members(&manifest))?
        .ok_or(PceCdLoadError::NoArchivePpfStack)?;

    progress.set_total_bytes(manifest.decoded_bytes);
    progress.set_completed_bytes(0);
    progress.set_phase(PceCdPackageLoadPhase::ReadingCue);
    let cue_bytes = extract_member(
        &mut archive,
        member(&manifest, &cue_name)?,
        &cancel,
        &progress,
        0,
    )?;
    let sheet = parse_cue_bytes(&cue_bytes)?;
    let mut resolved = Vec::with_capacity(sheet.files.len());
    let mut targets = BTreeSet::new();
    let mut data_bytes = 0_u64;
    for file in &sheet.files {
        let name = resolve_reference(&manifest, &cue_name, &file.reference)?;
        let referenced = member(&manifest, &name)?;
        data_bytes = data_bytes
            .checked_add(referenced.size)
            .ok_or(PceCdLoadError::DataTooLarge(u64::MAX))?;
        if data_bytes > PCE_CD_DATA_BYTES_LIMIT as u64 {
            return Err(PceCdLoadError::DataTooLarge(data_bytes));
        }
        targets.insert(name.clone());
        resolved.push(name);
    }
    targets.extend(patch_names.iter().cloned());
    progress.set_phase(PceCdPackageLoadPhase::ReadingData);
    let mut completed = cue_bytes.len() as u64;
    let mut extracted = BTreeMap::new();
    for name in targets {
        let bytes = extract_member(
            &mut archive,
            member(&manifest, &name)?,
            &cancel,
            &progress,
            completed,
        )?;
        completed = completed.saturating_add(bytes.len() as u64);
        extracted.insert(name, bytes);
    }
    let files = resolved
        .into_iter()
        .map(|name| {
            extracted
                .remove(&name)
                .ok_or(PceCdLoadError::ArchiveMemberMissing(name))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let patches = patches_from_bytes(&patch_names, &mut extracted)?;
    check_cancelled(&cancel)?;
    progress.set_phase(PceCdPackageLoadPhase::Building);
    let loaded = build_archive_ppf_load(ArchivePpfBuildInput {
        archive_path: path,
        cue_name: &cue_name,
        cue_bytes: &cue_bytes,
        sheet: &sheet,
        files,
        archive_identity: identity,
        patches,
    })?;
    progress.set_phase(PceCdPackageLoadPhase::Complete);
    Ok(loaded)
}

fn load(
    path: &Path,
    selected_cue_name: Option<&str>,
    cancel: Arc<AtomicBool>,
    progress: Arc<PceCdPackageProgress>,
    apply_mods: bool,
) -> Result<(PathBuf, LoadedPceCd, PceCdArchiveCueIdentity), PceCdLoadError> {
    check_cancelled(&cancel)?;
    progress.set_phase(PceCdPackageLoadPhase::Inspecting);
    let bytes = read_archive(path, &cancel)?;
    let source_sha256 = Sha256::digest(&bytes).into();
    let source_len = bytes.len();
    let (mut archive, manifest) = open_validated(bytes)?;
    let cue_name = select_normalized_cue_name(&manifest.cue_names, selected_cue_name)?.to_owned();
    let selection = if selected_cue_name.is_some() {
        PceCdArchiveCueSelection::Explicit
    } else {
        PceCdArchiveCueSelection::Unique
    };
    let identity = zip_cue_identity(source_sha256, source_len, &cue_name, selection);

    progress.set_total_bytes(manifest.decoded_bytes);
    progress.set_completed_bytes(0);
    progress.set_phase(PceCdPackageLoadPhase::ReadingCue);
    let cue_member = member(&manifest, &cue_name)?;
    let cue_bytes = extract_member(&mut archive, cue_member, &cancel, &progress, 0)?;
    let sheet = parse_cue_bytes(&cue_bytes)?;

    let mut resolved = Vec::with_capacity(sheet.files.len());
    let mut targets = BTreeSet::new();
    let mut data_bytes = 0_u64;
    for file in &sheet.files {
        let name = resolve_reference(&manifest, &cue_name, &file.reference)?;
        let referenced = member(&manifest, &name)?;
        data_bytes = data_bytes
            .checked_add(referenced.size)
            .ok_or(PceCdLoadError::DataTooLarge(u64::MAX))?;
        if data_bytes > PCE_CD_DATA_BYTES_LIMIT as u64 {
            return Err(PceCdLoadError::DataTooLarge(data_bytes));
        }
        targets.insert(name.clone());
        resolved.push(name);
    }

    check_cancelled(&cancel)?;
    progress.set_phase(PceCdPackageLoadPhase::ReadingData);
    let mut completed = cue_bytes.len() as u64;
    let mut extracted = BTreeMap::new();
    for name in targets {
        let referenced = member(&manifest, &name)?;
        let bytes = extract_member(&mut archive, referenced, &cancel, &progress, completed)?;
        completed = completed
            .checked_add(bytes.len() as u64)
            .ok_or(PceCdLoadError::ArchiveDecodedLimit)?;
        extracted.insert(name, bytes);
    }
    let files = resolved
        .into_iter()
        .map(|name| {
            extracted
                .remove(&name)
                .ok_or(PceCdLoadError::ArchiveMemberMissing(name))
        })
        .collect::<Result<Vec<_>, _>>()?;
    check_cancelled(&cancel)?;
    progress.set_phase(PceCdPackageLoadPhase::Building);
    let loaded = build_disc_with_mods(cue_bytes, &sheet, files, apply_mods)?;
    progress.set_phase(PceCdPackageLoadPhase::Complete);
    Ok((virtual_member_path(path, &cue_name), loaded, identity))
}

fn read_archive(path: &Path, cancel: &AtomicBool) -> Result<Vec<u8>, PceCdLoadError> {
    let mut file = std::fs::File::open(path)
        .map_err(|_| PceCdLoadError::ArchiveUnreadable(path.to_path_buf()))?;
    let size = file
        .metadata()
        .map_err(|_| PceCdLoadError::ArchiveUnreadable(path.to_path_buf()))?
        .len();
    if size > ZIP_ARCHIVE_BYTES_LIMIT {
        return Err(PceCdLoadError::ArchiveTooLarge(size));
    }
    let capacity = usize::try_from(size).map_err(|_| PceCdLoadError::ArchiveTooLarge(size))?;
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(capacity)
        .map_err(|_| PceCdLoadError::ArchiveAllocationFailed)?;
    let mut chunk = [0; 64 * 1024];
    loop {
        check_cancelled(cancel)?;
        let read = file
            .read(&mut chunk)
            .map_err(|_| PceCdLoadError::ArchiveUnreadable(path.to_path_buf()))?;
        if read == 0 {
            break;
        }
        bytes.extend_from_slice(&chunk[..read]);
        if bytes.len() as u64 > ZIP_ARCHIVE_BYTES_LIMIT {
            return Err(PceCdLoadError::ArchiveTooLarge(bytes.len() as u64));
        }
    }
    if bytes.len() != capacity {
        return Err(PceCdLoadError::ArchiveChanged);
    }
    Ok(bytes)
}

fn open_validated(bytes: Vec<u8>) -> Result<(OpenZip, ZipManifest), PceCdLoadError> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).map_err(map_zip_error)?;
    let manifest = validated_manifest(&mut archive)?;
    Ok((archive, manifest))
}

fn validated_manifest<R: Read + std::io::Seek>(
    archive: &mut zip::ZipArchive<R>,
) -> Result<ZipManifest, PceCdLoadError> {
    if archive.len() > ZIP_ENTRY_LIMIT {
        return Err(PceCdLoadError::TooManyArchiveEntries(archive.len()));
    }
    let mut members = Vec::new();
    let mut cue_names = Vec::new();
    let mut names = BTreeSet::new();
    let mut decoded_bytes = 0_u64;
    for index in 0..archive.len() {
        let entry = archive.by_index(index).map_err(map_zip_error)?;
        if entry.is_dir() {
            continue;
        }
        let raw_name = entry.name();
        if entry.enclosed_name().is_none() {
            return Err(PceCdLoadError::UnsafeArchiveEntry(raw_name.to_owned()));
        }
        let name = normalize_portable_path(raw_name)
            .map_err(|_| PceCdLoadError::UnsafeArchiveEntry(raw_name.to_owned()))?;
        if !names.insert(name.to_ascii_lowercase()) {
            return Err(PceCdLoadError::DuplicateArchiveEntry(name));
        }
        if entry.encrypted() {
            return Err(PceCdLoadError::ArchiveCodecUnsupported(
                "encrypted ZIP member".to_owned(),
            ));
        }
        if entry
            .unix_mode()
            .is_some_and(|mode| mode & UNIX_FILE_TYPE_MASK == UNIX_SYMBOLIC_LINK)
        {
            return Err(PceCdLoadError::ArchiveLinkUnsupported(name));
        }
        decoded_bytes = decoded_bytes
            .checked_add(entry.size())
            .ok_or(PceCdLoadError::ArchiveDecodedLimit)?;
        if decoded_bytes > ZIP_DECODED_BYTES_LIMIT {
            return Err(PceCdLoadError::ArchiveDecodedLimit);
        }
        if Path::new(&name)
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| extension.eq_ignore_ascii_case("cue"))
        {
            if entry.size() > PCE_CD_CUE_BYTES_LIMIT as u64 {
                return Err(PceCdLoadError::CueTooLarge(entry.size()));
            }
            cue_names.push(name.clone());
        }
        members.push(ZipMember {
            index,
            name,
            size: entry.size(),
        });
    }
    Ok(ZipManifest {
        members,
        cue_names,
        decoded_bytes,
    })
}

fn member<'a>(manifest: &'a ZipManifest, name: &str) -> Result<&'a ZipMember, PceCdLoadError> {
    manifest
        .members
        .iter()
        .find(|member| member.name == name)
        .ok_or_else(|| PceCdLoadError::ArchiveMemberMissing(name.to_owned()))
}

fn ppf_members(manifest: &ZipManifest) -> Vec<ArchivePpfMember> {
    manifest
        .members
        .iter()
        .map(|member| ArchivePpfMember {
            name: member.name.clone(),
            size: member.size,
            is_regular: true,
        })
        .collect()
}

fn extract_member(
    archive: &mut zip::ZipArchive<Cursor<Vec<u8>>>,
    member: &ZipMember,
    cancel: &AtomicBool,
    progress: &PceCdPackageProgress,
    progress_base: u64,
) -> Result<Vec<u8>, PceCdLoadError> {
    check_cancelled(cancel)?;
    let capacity =
        usize::try_from(member.size).map_err(|_| PceCdLoadError::ArchiveAllocationFailed)?;
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(capacity)
        .map_err(|_| PceCdLoadError::ArchiveAllocationFailed)?;
    let mut entry = archive.by_index(member.index).map_err(map_zip_error)?;
    if entry.name() != member.name || entry.size() != member.size {
        return Err(PceCdLoadError::ArchiveChanged);
    }
    let mut chunk = [0; 64 * 1024];
    loop {
        check_cancelled(cancel)?;
        let read = entry.read(&mut chunk).map_err(map_io_error)?;
        if read == 0 {
            break;
        }
        bytes.extend_from_slice(&chunk[..read]);
        if bytes.len() as u64 > member.size {
            return Err(PceCdLoadError::ArchiveMemberSizeMismatch(
                member.name.clone(),
            ));
        }
        progress.set_completed_bytes(progress_base.saturating_add(bytes.len() as u64));
    }
    if bytes.len() as u64 != member.size {
        return Err(PceCdLoadError::ArchiveMemberSizeMismatch(
            member.name.clone(),
        ));
    }
    Ok(bytes)
}

fn resolve_reference(
    manifest: &ZipManifest,
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
    manifest
        .members
        .iter()
        .find(|member| member.name.eq_ignore_ascii_case(&normalized))
        .map(|member| member.name.clone())
        .ok_or(PceCdLoadError::ArchiveMemberMissing(normalized))
}

fn zip_cue_identity(
    source_sha256: [u8; 32],
    source_len: usize,
    cue_name: &str,
    selection: PceCdArchiveCueSelection,
) -> PceCdArchiveCueIdentity {
    let mut hasher = Sha256::new();
    hasher.update(b"zeff-pce-cd-zip-cue-member:v1\0");
    hasher.update(cue_name.as_bytes());
    PceCdArchiveCueIdentity {
        source_sha256,
        source_len,
        cue_member_path_sha256: hasher.finalize().into(),
        selection,
    }
}

fn virtual_member_path(archive: &Path, member: &str) -> PathBuf {
    member
        .split('/')
        .fold(archive.to_path_buf(), |path, part| path.join(part))
}

fn check_cancelled(cancel: &AtomicBool) -> Result<(), PceCdLoadError> {
    if cancel.load(Ordering::Acquire) {
        Err(PceCdLoadError::ArchiveCancelled)
    } else {
        Ok(())
    }
}

fn map_zip_error(error: zip::result::ZipError) -> PceCdLoadError {
    PceCdLoadError::Archive(error.to_string())
}

fn map_io_error(error: std::io::Error) -> PceCdLoadError {
    if error.kind() == std::io::ErrorKind::InvalidData {
        PceCdLoadError::ArchiveChecksumMismatch
    } else {
        PceCdLoadError::Archive(error.to_string())
    }
}
