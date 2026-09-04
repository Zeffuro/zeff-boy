use super::*;
use crate::emu_backend::pce_cd_archive::ppf::{
    ArchivePpfBuildInput, ArchivePpfMember, build_archive_ppf_load, discover_archive_ppf_members,
    patch_identities, patches_from_bytes,
};
use crate::emu_backend::pce_cd_archive::{PceCdArchivePpfCandidate, PceCdArchivePpfLoad};

pub(crate) fn inspect_rar_ppf_candidates_with_archive_identity(
    path: &Path,
    cancel: Arc<AtomicBool>,
) -> Result<Vec<PceCdArchivePpfCandidate>, PceCdLoadError> {
    let (archive, manifest, source_sha256, source_len) = open_validated_owned(path, &cancel)?;
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
    let mut extracted = extract_targets(
        &archive,
        &manifest,
        &targets,
        cancel,
        Arc::new(PceCdPackageProgress::default()),
        0,
    )?;
    candidates
        .into_iter()
        .map(|(cue_member, names)| {
            Ok(PceCdArchivePpfCandidate {
                identity: rar_cue_identity(
                    source_sha256,
                    source_len,
                    &cue_member,
                    PceCdArchiveCueSelection::Explicit,
                )?,
                patches: patch_identities(&names, &mut extracted)?,
                cue_member,
            })
        })
        .collect()
}

pub(crate) fn load_rar_cue_with_control_and_archive_ppf(
    path: &Path,
    cancel: Arc<AtomicBool>,
    progress: Arc<PceCdPackageProgress>,
) -> Result<PceCdArchivePpfLoad, PceCdLoadError> {
    load_rar_archive_ppf(path, None, cancel, progress)
}

pub(crate) fn load_rar_selected_cue_with_control_and_archive_ppf(
    path: &Path,
    selected_cue_name: &str,
    cancel: Arc<AtomicBool>,
    progress: Arc<PceCdPackageProgress>,
) -> Result<PceCdArchivePpfLoad, PceCdLoadError> {
    load_rar_archive_ppf(path, Some(selected_cue_name), cancel, progress)
}

fn load_rar_archive_ppf(
    path: &Path,
    selected_cue_name: Option<&str>,
    cancel: Arc<AtomicBool>,
    progress: Arc<PceCdPackageProgress>,
) -> Result<PceCdArchivePpfLoad, PceCdLoadError> {
    check_cancelled(&cancel)?;
    progress.set_phase(PceCdPackageLoadPhase::Inspecting);
    let (archive, manifest, source_sha256, source_len) = open_validated_owned(path, &cancel)?;
    let cue_name = select_normalized_cue_name(&manifest.cue_names, selected_cue_name)?.to_owned();
    let selection = if selected_cue_name.is_some() {
        PceCdArchiveCueSelection::Explicit
    } else {
        PceCdArchiveCueSelection::Unique
    };
    let identity = rar_cue_identity(source_sha256, source_len, &cue_name, selection)?;
    let patch_names = discover_archive_ppf_members(&cue_name, &ppf_members(&manifest))?
        .ok_or(PceCdLoadError::NoArchivePpfStack)?;
    let cue_target = BTreeSet::from([cue_name.clone()]);
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
        .remove(&cue_name)
        .ok_or_else(|| PceCdLoadError::ArchiveMemberMissing(cue_name.clone()))?;
    let sheet = parse_cue_bytes(&cue_bytes)?;
    let mut resolved = Vec::with_capacity(sheet.files.len());
    let mut targets = BTreeSet::new();
    let mut data_bytes = 0_u64;
    for file in &sheet.files {
        let name = resolve_reference(&manifest, &cue_name, &file.reference)?;
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
    targets.extend(patch_names.iter().cloned());
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
    let patches = patches_from_bytes(&patch_names, &mut extracted)?;
    build_archive_ppf_load(ArchivePpfBuildInput {
        archive_path: path,
        cue_name: &cue_name,
        cue_bytes: &cue_bytes,
        sheet: &sheet,
        files,
        archive_identity: identity,
        patches,
    })
}

fn ppf_members(manifest: &RarManifest) -> Vec<ArchivePpfMember> {
    manifest
        .members
        .iter()
        .map(|member| ArchivePpfMember {
            name: member.name.clone(),
            size: member.size,
            is_regular: !member.is_directory,
        })
        .collect()
}
