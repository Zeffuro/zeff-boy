use super::super::*;
use super::DecodePassPolicy;

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
