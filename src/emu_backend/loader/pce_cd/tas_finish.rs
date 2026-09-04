use super::*;
use crate::emu_backend::pce_cd_archive::{
    PceCdArchiveCueIdentity, PceCdArchivePpfLoad, PceCdArchivePpfPatchIdentity,
};

pub(crate) fn finish_preloaded_archive_ppf_backend(
    source_path: &Path,
    load: PceCdArchivePpfLoad,
    config: &BackendLoadConfig,
) -> anyhow::Result<LoadedBackend> {
    let PceCdArchivePpfLoad {
        cue_path,
        loaded,
        archive_identity,
        patches,
        unpatched_disc_sha256,
    } = load;
    let patch_identities = patches
        .into_iter()
        .map(|patch| patch.identity)
        .collect::<Vec<_>>();
    if patch_identities.is_empty() {
        return Err(super::super::super::pce_cd::PceCdLoadError::NoArchivePpfStack.into());
    }
    if loaded.source_disc_sha256 != unpatched_disc_sha256 {
        return Err(super::super::super::pce_cd::PceCdLoadError::ArchiveChanged.into());
    }
    let (archive, rar, zip) = exact_archive_identity(source_path, archive_identity, config)?;
    let console_wiring = pce_cd_console_wiring(config, loaded.content_sha256);
    let system_card = resolve_pce_cd_system_card(
        config,
        source_path,
        console_wiring,
        loaded.disc.content_hash() == super::super::super::pce_cd::ADPCM_FIXTURE_DISC_SHA256,
    )?;
    let system_card_profile = pce_system_card_profile(&system_card, console_wiring)?;
    check_minimum_system_card(unpatched_disc_sha256, system_card_profile)?;
    let system_card_board = pce_system_card_board(system_card_profile);
    let effective_disc_sha256 = loaded.disc.content_hash();
    let provenance = super::super::super::pce::PceTasLoadProvenanceSeed::new_cd(
        super::super::super::pce::PceTasCdLoadMedia {
            raw_source_media_sha256: archive_identity.source_sha256,
            raw_source_media_len: archive_identity.source_len,
            source_disc_sha256: unpatched_disc_sha256,
            effective_disc_sha256,
            direct: true,
            chd: false,
            iso: false,
            ppf: false,
            archive,
            archive_ppf: true,
            rar,
            zip,
            archive_cue_member_path_sha256: archive
                .then_some(archive_identity.cue_member_path_sha256),
            rar_cue_member_path_sha256: rar.then_some(archive_identity.cue_member_path_sha256),
            zip_cue_member_path_sha256: zip.then_some(archive_identity.cue_member_path_sha256),
            archive_cue_explicitly_selected: archive && explicit(archive_identity),
            rar_cue_explicitly_selected: rar && explicit(archive_identity),
            zip_cue_explicitly_selected: zip && explicit(archive_identity),
            archive_ppf_patches: patch_identities.into_iter().map(provenance_patch).collect(),
        },
        super::super::super::pce::PceTasLoadSetup {
            loaded_from_source_path: false,
            any_mod_enabled: true,
            any_mod_applied: effective_disc_sha256 != unpatched_disc_sha256,
            initial_input: config.initial_input,
            configured_sample_rate: config.sample_rate,
            selected_wiring: config.pce_console_wiring,
            selected_board: Some(system_card_board),
            selected_hardware: Some(zeff_pce_core::hardware::PceCartridgeHardware::Base),
            selected_controller_mode: config.pce_controller_mode,
            selected_memory_base_mode: config.pce_memory_base_mode,
            selected_arcade_card_mode: config.pce_arcade_card_mode,
            tas_source_media: config.pce_cd_tas_source_media,
        },
    );
    let mut backend = super::super::super::PceBackend::new_cdrom2_without_host_persistence(
        system_card.bytes,
        loaded.disc,
        super::super::super::pce::PceCdBackendConfig {
            system_card_board,
            cue_path,
            source_path: source_path.to_path_buf(),
            content_hash: loaded.content_sha256,
            content_crc32: loaded.content_crc32,
            source_disc_hash: loaded.source_disc_sha256,
            console_wiring,
            arcade_card_mode: config.pce_arcade_card_mode,
        },
    )?;
    if let Some(sample_rate) = config.sample_rate {
        backend.set_sample_rate(sample_rate);
    }
    backend.update_controller_mode(config.pce_controller_mode);
    backend.update_memory_base_mode(config.pce_memory_base_mode);
    backend.set_firmware_manifests(vec![system_card.manifest]);
    let provenance = provenance.finish(
        &backend,
        super::super::super::pce::PceTasPersistentLoadOutcome::Skipped,
    );
    backend = backend.with_tas_load_provenance(provenance);
    if let Some((buttons, dpad)) = config.initial_input {
        backend.set_input(buttons, dpad);
    }
    Ok(LoadedBackend {
        backend: EmuBackend::from_pce(backend),
        original_crc32: loaded.mod_crc32,
    })
}

fn exact_archive_identity(
    source_path: &Path,
    actual: PceCdArchiveCueIdentity,
    config: &BackendLoadConfig,
) -> anyhow::Result<(bool, bool, bool)> {
    let formats = (
        path_extension_is(source_path, "7z"),
        path_extension_is(source_path, "rar"),
        path_extension_is(source_path, "zip"),
    );
    let expected = match formats {
        (true, false, false) => config.pce_cd_tas_archive_cue,
        (false, true, false) => config.pce_cd_tas_rar_cue,
        (false, false, true) => config.pce_cd_tas_zip_cue,
        _ => None,
    };
    if expected != Some(actual)
        || config.pce_cd_tas_archive_cue.is_some() != formats.0
        || config.pce_cd_tas_rar_cue.is_some() != formats.1
        || config.pce_cd_tas_zip_cue.is_some() != formats.2
    {
        return Err(super::super::super::pce_cd::PceCdLoadError::ArchiveChanged.into());
    }
    Ok(formats)
}

fn explicit(identity: PceCdArchiveCueIdentity) -> bool {
    identity.selection == super::super::super::pce_cd_archive::PceCdArchiveCueSelection::Explicit
}

fn provenance_patch(
    patch: PceCdArchivePpfPatchIdentity,
) -> super::super::super::pce::PceTasArchivePpfPatchIdentity {
    super::super::super::pce::PceTasArchivePpfPatchIdentity {
        member_path: patch.member_path,
        len: patch.len,
        sha256: patch.sha256,
    }
}
