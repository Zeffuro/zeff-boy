use super::*;

pub(crate) fn validate_direct_pce_cd_tas_execution_runtime(
    backend: &EmuBackend,
    cheats_present: bool,
) -> Result<zeff_pce_core::hardware::save_state::tas::CurrentNativePceCdTasStateInspection> {
    validate_direct_pce_cd_tas_execution_runtime_for_controller(
        backend,
        cheats_present,
        PceControllerMode::TwoButton,
    )
}

pub(crate) fn validate_direct_pce_multitap_cd_tas_execution_runtime(
    backend: &EmuBackend,
    cheats_present: bool,
) -> Result<zeff_pce_core::hardware::save_state::tas::CurrentNativePceCdTasStateInspection> {
    validate_direct_pce_cd_tas_execution_runtime_for_controller(
        backend,
        cheats_present,
        PceControllerMode::Multitap,
    )
}

pub(super) fn validate_direct_pce_cd_tas_execution_runtime_for_controller(
    backend: &EmuBackend,
    cheats_present: bool,
    controller_mode: PceControllerMode,
) -> Result<zeff_pce_core::hardware::save_state::tas::CurrentNativePceCdTasStateInspection> {
    ensure!(
        backend.system() == ActiveSystem::Pce,
        "TAS execution profile requires a PC Engine backend"
    );
    let metadata = backend.replay_metadata();
    let pce = backend
        .pce()
        .context("PC Engine backend became unavailable")?;
    let provenance = pce
        .tas_load_provenance()
        .context("PC Engine backend omitted CD load provenance")?;
    let arcade_card = provenance.load.selected_arcade_card_mode == PceArcadeCardMode::Enabled;
    let memory_base = provenance.load.selected_memory_base_mode == PceMemoryBaseMode::Enabled;
    let multitap = controller_mode == PceControllerMode::Multitap;
    ensure!(
        provenance.load.direct_pce_cd && provenance.load.raw_source_media_len != 0,
        "PC Engine CD TAS requires bounded direct media"
    );
    ensure!(
        [
            provenance.load.direct_pce_cd_chd,
            provenance.load.direct_pce_cd_iso,
            provenance.load.direct_pce_cd_ppf,
            provenance.load.direct_pce_cd_archive,
            provenance.load.direct_pce_cd_rar,
            provenance.load.direct_pce_cd_zip,
        ]
        .into_iter()
        .filter(|kind| *kind)
        .count()
            <= 1,
        "PC Engine CD TAS requires one direct media source type"
    );
    ensure!(
        !(arcade_card && memory_base),
        "PC Engine CD TAS does not support combined Arcade Card and Memory Base 128 profiles"
    );
    ensure!(
        provenance.load.archive_cue_member_path_sha256.is_some()
            == provenance.load.direct_pce_cd_archive
            && (!provenance.load.archive_cue_explicitly_selected
                || provenance.load.direct_pce_cd_archive),
        "PC Engine CD TAS archive provenance is incomplete"
    );
    ensure!(
        provenance.load.rar_cue_member_path_sha256.is_some() == provenance.load.direct_pce_cd_rar
            && (!provenance.load.rar_cue_explicitly_selected || provenance.load.direct_pce_cd_rar),
        "PC Engine CD TAS RAR provenance is incomplete"
    );
    ensure!(
        provenance.load.zip_cue_member_path_sha256.is_some() == provenance.load.direct_pce_cd_zip
            && (!provenance.load.zip_cue_explicitly_selected || provenance.load.direct_pce_cd_zip),
        "PC Engine CD ZIP provenance is incomplete"
    );
    let profile = PceCdTasProfile::from_runtime_flags(
        (
            provenance.load.direct_pce_cd_chd,
            provenance.load.direct_pce_cd_iso,
            provenance.load.direct_pce_cd_ppf,
            provenance.load.direct_pce_cd_archive,
            provenance.load.direct_pce_cd_rar,
            provenance.load.direct_pce_cd_zip,
        ),
        provenance.load.direct_pce_cd_archive_ppf,
        (
            provenance.load.archive_cue_explicitly_selected,
            provenance.load.rar_cue_explicitly_selected,
            provenance.load.zip_cue_explicitly_selected,
        ),
        (arcade_card, memory_base),
        controller_mode,
    )
    .context("PC Engine CD TAS provenance describes an invalid execution profile")?;
    validate_archive_ppf_provenance(provenance.load)?;
    ensure!(
        !multitap
            || !arcade_card
            || (profile.media() == PceCdTasMediaRoute::Cue
                && crate::emu_backend::pce_profiles::automatic_arcade_card_enabled(Some(
                    provenance.load.source_disc_sha256.context(
                        "PC Engine CD Arcade Card Multitap omitted its catalog disc witness",
                    )?,
                ))),
        "PC Engine CD Arcade Card Multitap requires a direct CUE and independent Arcade Card catalog witness"
    );
    ensure!(
        !multitap
            || !memory_base
            || (profile.media() == PceCdTasMediaRoute::Cue
                && provenance
                    .load
                    .source_disc_sha256
                    .is_some_and(|hash| { direct_pce_cd_memory_base_multitap_eligible(hash) })),
        "PC Engine CD Memory Base Multitap requires a direct CUE and independent Memory Base and Multitap catalog witnesses"
    );
    ensure!(
        (provenance.load.direct_pce_cd_ppf
            || provenance.load.direct_pce_cd_archive_ppf
            || provenance.load.source_disc_sha256 == provenance.load.effective_disc_sha256)
            && provenance.load.effective_disc_sha256 == pce.normalized_disc_hash(),
        "PC Engine CD TAS normalized disc identity is incompatible"
    );
    let archive_member_sha256 = provenance
        .load
        .archive_cue_member_path_sha256
        .or(provenance.load.rar_cue_member_path_sha256)
        .or(provenance.load.zip_cue_member_path_sha256);
    let expected_source_sha256 = if provenance.load.direct_pce_cd_chd {
        direct_pce_cd_chd_source_identity(
            provenance.load.raw_source_media_sha256,
            provenance.load.raw_source_media_len,
        )
        .0
    } else if provenance.load.direct_pce_cd_iso {
        direct_pce_cd_iso_source_identity(
            provenance.load.raw_source_media_sha256,
            provenance.load.raw_source_media_len,
        )
        .0
    } else if let Some(member_sha256) = archive_member_sha256 {
        if provenance.load.direct_pce_cd_archive_ppf {
            let patches = provenance
                .load
                .archive_ppf_patches
                .iter()
                .map(|patch| (patch.member_path.as_str(), patch.len, patch.sha256))
                .collect::<Vec<_>>();
            profile
                .archive_ppf_source_identity(
                    provenance.load.raw_source_media_sha256,
                    provenance.load.raw_source_media_len,
                    member_sha256,
                    &patches,
                )
                .context("PC Engine CD package omitted its archive PPF profile")?
                .0
        } else {
            profile
                .archive_source_identity(
                    provenance.load.raw_source_media_sha256,
                    provenance.load.raw_source_media_len,
                    member_sha256,
                )
                .context("PC Engine CD package omitted its archive profile")?
                .0
        }
    } else {
        provenance.load.raw_source_media_sha256
    };
    ensure!(
        provenance.load.tas_source_media_sha256 == expected_source_sha256
            && provenance.load.tas_source_media_len == provenance.load.raw_source_media_len
            && provenance.load.tas_sync_config_sha256 == profile.sync_config().0,
        "PC Engine CD TAS source identity is incompatible"
    );
    ensure!(
        ((!(provenance.load.direct_pce_cd_ppf || provenance.load.direct_pce_cd_archive_ppf)
            && !provenance.load.any_mod_enabled
            && !provenance.load.any_mod_applied)
            || ((provenance.load.direct_pce_cd_ppf || provenance.load.direct_pce_cd_archive_ppf)
                && provenance.load.any_mod_enabled))
            && provenance.load.persistent_load
                == crate::emu_backend::pce::PceTasPersistentLoadOutcome::Skipped
            && provenance.load.initial_input.is_none(),
        "PC Engine CD TAS requires unmodified media with host persistence disabled"
    );
    ensure!(
        provenance.load.direct_pce_cd_chd
            || provenance.load.direct_pce_cd_iso
            || provenance.load.direct_pce_cd_ppf
            || provenance.load.direct_pce_cd_archive
            || provenance.load.direct_pce_cd_rar
            || provenance.load.direct_pce_cd_zip
            || provenance.load.raw_source_media_sha256
                == provenance
                    .load
                    .source_disc_sha256
                    .context("PC Engine backend omitted source disc identity")?,
        "PC Engine CD TAS direct CUE identity is incompatible"
    );
    ensure!(
        !arcade_card
            || provenance.load.source_disc_sha256.is_some_and(|hash| {
                direct_pce_cd_arcade_eligible(provenance.load.direct_pce_cd_ppf, hash)
            }),
        "PC Engine CD TAS Arcade Card requires an exact catalog-recognized media profile"
    );
    ensure!(
        !memory_base
            || provenance.load.source_disc_sha256.is_some_and(|hash| {
                if multitap {
                    profile.media() == PceCdTasMediaRoute::Cue
                        && direct_pce_cd_memory_base_multitap_eligible(hash)
                } else {
                    direct_pce_cd_memory_base_eligible(
                        provenance.load.direct_pce_cd_chd,
                        provenance.load.direct_pce_cd_iso,
                        provenance.load.direct_pce_cd_ppf,
                        hash,
                    )
                }
            }),
        "PC Engine CD TAS Memory Base 128 requires an exact CUE, CHD, ISO, archive, or ordered PPF profile"
    );
    ensure!(
        !multitap
            || provenance.load.source_disc_sha256.is_some_and(|hash| {
                crate::emu_backend::pce_profiles::automatic_controller_mode(hash)
                    == PceControllerMode::Multitap
            }),
        "PC Engine CD Multitap TAS requires an exact catalog-recognized disc"
    );
    ensure!(
        provenance.load.configured_sample_rate == Some(48_000)
            && provenance.load.initial_sample_rate == 48_000
            && provenance.current_sample_rate == 48_000,
        "PC Engine CD TAS requires an exact 48000 Hz sample rate"
    );
    ensure!(
        provenance.load.selected_wiring == Some(PceConsoleWiring::PcEngine)
            && provenance.load.effective_wiring == PceConsoleWiring::PcEngine
            && provenance.load.selected_board == Some(PceHuCardBoard::SystemCardV3)
            && provenance.load.effective_board == PceHuCardBoard::SystemCardV3
            && provenance.load.selected_hardware == Some(PceCartridgeHardware::Base)
            && provenance.load.effective_topology == PceHardwareTopology::Base
            && provenance.load.selected_controller_mode == controller_mode
            && provenance.load.effective_controller_mode == controller_mode
            && provenance.load.selected_memory_base_mode
                == provenance.load.effective_memory_base_mode
            && matches!(
                provenance.load.selected_memory_base_mode,
                PceMemoryBaseMode::Disabled | PceMemoryBaseMode::Enabled
            )
            && (provenance.load.selected_arcade_card_mode
                == provenance.load.effective_arcade_card_mode)
            && matches!(
                provenance.load.selected_arcade_card_mode,
                PceArcadeCardMode::Disabled | PceArcadeCardMode::Enabled
            ),
        "PC Engine CD TAS requires its exact Base CD controller topology"
    );
    ensure!(
        !cheats_present && metadata.cheat_sha256.is_none(),
        "PC Engine CD TAS execution enabled cheats"
    );
    ensure!(
        backend.save_ram_kind() == SaveRamKind::known_battery_backed(CDROM2_BRAM_LEN)
            && pce.tas_frame_counters_match()
            && pce.tas_output_policy_is_exact()
            && pce.tas_presented_frame_is_current(),
        "PC Engine CD TAS runtime state is incompatible"
    );
    let state = backend.encode_state_bytes()?;
    let inspection = if controller_mode == PceControllerMode::TwoButton {
        pce.inspect_current_native_cd_tas_state_for_profile(&state, arcade_card, memory_base)?
    } else {
        pce.inspect_current_native_cd_tas_state_for_profile_and_controller(
            &state,
            arcade_card,
            memory_base,
            controller_mode,
        )?
    };
    ensure!(
        inspection.board == PceHuCardBoard::SystemCardV3
            && inspection.wiring == PceConsoleWiring::PcEngine
            && inspection.psg_revision == PsgRevision::HuC6280
            && inspection.arcade_card_enabled == arcade_card
            && inspection.memory_base_enabled == memory_base
            && Some(inspection.disc_sha256) == pce.normalized_disc_hash()
            && firmware(backend, inspection.system_card_sha256)?.len() == 1,
        "PC Engine CD core media, firmware, or hardware identity is incompatible"
    );
    Ok(inspection)
}

fn validate_archive_ppf_provenance(
    load: &crate::emu_backend::pce::PceTasLoadProvenance,
) -> Result<()> {
    if !load.direct_pce_cd_archive_ppf {
        ensure!(
            load.archive_ppf_patches.is_empty(),
            "non-PPF archive provenance contains a PPF stack"
        );
        return Ok(());
    }
    ensure!(
        (1..=8).contains(&load.archive_ppf_patches.len()),
        "archive PPF provenance requires one bounded ordered stack"
    );
    let mut total = 0_usize;
    let mut parent = None;
    for (index, patch) in load.archive_ppf_patches.iter().enumerate() {
        ensure!(
            patch.len <= 16 * 1024 * 1024
                && patch.member_path.rsplit('/').next() == Some(&format!("{:04}.ppf", index + 1)),
            "archive PPF provenance is not canonical"
        );
        let normalized = crate::emu_backend::pce_cd::normalize_portable_path(&patch.member_path)
            .map_err(|_| anyhow::anyhow!("archive PPF provenance path is unsafe"))?;
        ensure!(
            normalized == patch.member_path,
            "archive PPF provenance path changed"
        );
        let patch_parent = patch
            .member_path
            .rsplit_once('/')
            .map(|(parent, _)| parent)
            .context("archive PPF provenance path has no container")?;
        ensure!(
            patch_parent.ends_with(".ppf") && parent.is_none_or(|value| value == patch_parent),
            "archive PPF provenance spans multiple containers"
        );
        parent = Some(patch_parent);
        total = total
            .checked_add(patch.len)
            .context("archive PPF provenance size overflowed")?;
    }
    ensure!(
        total <= 128 * 1024 * 1024,
        "archive PPF provenance exceeds its aggregate bound"
    );
    Ok(())
}
