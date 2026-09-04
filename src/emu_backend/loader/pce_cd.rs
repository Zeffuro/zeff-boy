use std::path::{Path, PathBuf};
use std::sync::Arc;
#[cfg(not(target_arch = "wasm32"))]
use std::sync::atomic::{AtomicBool, Ordering};

use zeff_pce_core::hardware::{PceConsoleWiring, PceHuCardBoard};

use super::{BackendLoadConfig, EmuBackend, LoadedBackend};
use crate::emu_core_trait::EmulatorCore;

#[cfg(not(target_arch = "wasm32"))]
mod native_archive;
#[cfg(not(target_arch = "wasm32"))]
mod tas_finish;
#[cfg(not(target_arch = "wasm32"))]
pub(crate) use native_archive::prepare_native_archive_backend;
#[cfg(all(not(target_arch = "wasm32"), test))]
pub(crate) use native_archive::prepare_seven_zip_backend;
#[cfg(not(target_arch = "wasm32"))]
pub(crate) use tas_finish::finish_preloaded_archive_ppf_backend;

pub(super) fn is_pce_cd_path(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            extension.eq_ignore_ascii_case("cue")
                || extension.eq_ignore_ascii_case("chd")
                || extension.eq_ignore_ascii_case("iso")
        })
}

#[cfg(not(target_arch = "wasm32"))]
pub(super) fn load_pce_cd_backend(
    source_path: &Path,
    cue_path: &Path,
    preloaded_data: Option<Vec<u8>>,
    config: &BackendLoadConfig,
) -> anyhow::Result<LoadedBackend> {
    if preloaded_data.is_some() {
        return Err(super::super::pce_cd::PceCdLoadError::PackagedCdSetUnsupported.into());
    }
    let direct_iso = source_path == cue_path && path_extension_is(cue_path, "iso");
    let direct_ppf = config.pce_cd_tas_ppf_stack.as_ref();
    let expected_archive_cue = config.pce_cd_tas_archive_cue;
    let expected_rar_cue = config.pce_cd_tas_rar_cue;
    let expected_zip_cue = config.pce_cd_tas_zip_cue;
    if direct_ppf.is_some() && !(source_path == cue_path && path_extension_is(cue_path, "cue")) {
        return Err(super::super::pce_cd::PceCdLoadError::Disc(
            "PC Engine CD TAS PPF overlays require a direct CUE".to_owned(),
        )
        .into());
    }
    if expected_archive_cue.is_some() && !path_extension_is(source_path, "7z") {
        return Err(super::super::pce_cd::PceCdLoadError::Disc(
            "PC Engine CD TAS archive provenance requires one direct 7z source".to_owned(),
        )
        .into());
    }
    if expected_rar_cue.is_some() && !path_extension_is(source_path, "rar") {
        return Err(super::super::pce_cd::PceCdLoadError::Disc(
            "PC Engine CD TAS RAR provenance requires one direct RAR source".to_owned(),
        )
        .into());
    }
    if expected_zip_cue.is_some() && !path_extension_is(source_path, "zip") {
        return Err(super::super::pce_cd::PceCdLoadError::Disc(
            "PC Engine CD TAS ZIP provenance requires one direct ZIP source".to_owned(),
        )
        .into());
    }
    let (cue_path, loaded_disc, archive_cue, rar_cue, zip_cue) = if source_path == cue_path
        && path_extension_is(cue_path, "cue")
    {
        (
            cue_path.to_path_buf(),
            if let Some(stack) = direct_ppf {
                stack.load(cue_path)?
            } else {
                super::super::pce_cd::load_direct_cue_with_mods(cue_path, config.apply_mods)?
            },
            None,
            None,
            None,
        )
    } else if source_path == cue_path && path_extension_is(cue_path, "chd") {
        (
            cue_path.to_path_buf(),
            super::super::pce_cd::load_direct_chd_with_mods(cue_path, config.apply_mods)?,
            None,
            None,
            None,
        )
    } else if source_path == cue_path && path_extension_is(cue_path, "iso") {
        let actual_cue = super::super::pce_cd::cue_path_for_iso(cue_path)?;
        let loaded =
            super::super::pce_cd::load_direct_cue_with_mods(&actual_cue, config.apply_mods)?;
        (actual_cue, loaded, None, None, None)
    } else if path_extension_is(source_path, "7z") {
        let cancel = AtomicBool::new(false);
        let progress = super::super::pce_cd_archive::PceCdPackageProgress::default();
        let (actual, loaded, archive_cue) = if let Some(expected) = expected_archive_cue {
            let (actual, loaded, identity) = if expected.selection
                == super::super::pce_cd_archive::PceCdArchiveCueSelection::Explicit
            {
                let selected = selected_archive_member_name(source_path, cue_path)?;
                super::super::pce_cd_archive::load_7z_selected_cue_with_control_and_archive_identity(
                    source_path,
                    &selected,
                    &cancel,
                    &progress,
                    config.pce_cd_archive_memory_limit_mib,
                    config.apply_mods,
                )?
            } else {
                super::super::pce_cd_archive::load_7z_cue_with_control_and_archive_identity(
                    source_path,
                    &cancel,
                    &progress,
                    config.pce_cd_archive_memory_limit_mib,
                    config.apply_mods,
                )?
            };
            (actual, loaded, Some(identity))
        } else {
            let (actual, loaded) = super::super::pce_cd_archive::load_7z_cue_with_control_and_mods(
                source_path,
                &cancel,
                &progress,
                config.pce_cd_archive_memory_limit_mib,
                config.apply_mods,
            )?;
            (actual, loaded, None)
        };
        if actual != cue_path
            || expected_archive_cue
                .zip(archive_cue)
                .is_some_and(|(expected, actual)| expected != actual)
        {
            return Err(super::super::pce_cd::PceCdLoadError::ArchiveChanged.into());
        }
        (actual, loaded, archive_cue, None, None)
    } else if path_extension_is(source_path, "rar") {
        let cancel = Arc::new(AtomicBool::new(false));
        let progress = Arc::new(super::super::pce_cd_archive::PceCdPackageProgress::default());
        let (actual, loaded, rar_cue) = if let Some(expected) = expected_rar_cue {
            let (actual, loaded, identity) = if expected.selection
                == super::super::pce_cd_archive::PceCdArchiveCueSelection::Explicit
            {
                let selected = selected_archive_member_name(source_path, cue_path)?;
                super::super::pce_cd_rar::load_rar_selected_cue_with_control_and_archive_identity(
                    source_path,
                    &selected,
                    cancel,
                    progress,
                    config.apply_mods,
                )?
            } else {
                super::super::pce_cd_rar::load_rar_cue_with_control_and_archive_identity(
                    source_path,
                    cancel,
                    progress,
                    config.apply_mods,
                )?
            };
            (actual, loaded, Some(identity))
        } else {
            let (actual, loaded) = super::super::pce_cd_rar::load_rar_cue_with_control_and_mods(
                source_path,
                cancel,
                progress,
                config.apply_mods,
            )?;
            (actual, loaded, None)
        };
        if actual != cue_path
            || expected_rar_cue
                .zip(rar_cue)
                .is_some_and(|(expected, actual)| expected != actual)
        {
            return Err(super::super::pce_cd::PceCdLoadError::ArchiveChanged.into());
        }
        (actual, loaded, None, rar_cue, None)
    } else if path_extension_is(source_path, "zip") {
        let cancel = Arc::new(AtomicBool::new(false));
        let progress = Arc::new(super::super::pce_cd_archive::PceCdPackageProgress::default());
        let (actual, loaded, zip_cue) = if let Some(expected) = expected_zip_cue {
            let (actual, loaded, identity) = if expected.selection
                == super::super::pce_cd_archive::PceCdArchiveCueSelection::Explicit
            {
                let selected = selected_archive_member_name(source_path, cue_path)?;
                super::super::pce_cd_zip::load_zip_selected_cue_with_control_and_archive_identity(
                    source_path,
                    &selected,
                    cancel,
                    progress,
                    config.apply_mods,
                )?
            } else {
                super::super::pce_cd_zip::load_zip_cue_with_control_and_archive_identity(
                    source_path,
                    cancel,
                    progress,
                    config.apply_mods,
                )?
            };
            (actual, loaded, Some(identity))
        } else {
            let (actual, loaded) = super::super::pce_cd_zip::load_zip_cue_with_control_and_mods(
                source_path,
                cancel,
                progress,
                config.apply_mods,
            )?;
            (actual, loaded, None)
        };
        if actual != cue_path
            || expected_zip_cue
                .zip(zip_cue)
                .is_some_and(|(expected, actual)| expected != actual)
        {
            return Err(super::super::pce_cd::PceCdLoadError::ArchiveChanged.into());
        }
        (actual, loaded, None, None, zip_cue)
    } else {
        return Err(super::super::pce_cd::PceCdLoadError::PackagedCdSetUnsupported.into());
    };
    let console_wiring = pce_cd_console_wiring(config, loaded_disc.content_sha256);
    let system_card = resolve_pce_cd_system_card(
        config,
        source_path,
        console_wiring,
        loaded_disc.disc.content_hash() == super::super::pce_cd::ADPCM_FIXTURE_DISC_SHA256,
    )?;
    let system_card_profile = pce_system_card_profile(&system_card, console_wiring)?;
    check_minimum_system_card(loaded_disc.source_disc_sha256, system_card_profile)?;
    let system_card_board = pce_system_card_board(system_card_profile);
    let source_disc_sha256 = loaded_disc.source_disc_sha256;
    let effective_disc_sha256 = loaded_disc.disc.content_hash();
    let tas_archive_cue = expected_archive_cue.and(archive_cue);
    let tas_rar_cue = expected_rar_cue.and(rar_cue);
    let tas_zip_cue = expected_zip_cue.and(zip_cue);
    let (raw_source_media_sha256, raw_source_media_len) = if let Some(stack) = direct_ppf {
        stack.source_media_identity()
    } else if direct_iso {
        super::super::pce_cd_file::direct_file_sha256(source_path)?
    } else if let Some(archive_cue) = tas_archive_cue {
        (archive_cue.source_sha256, archive_cue.source_len)
    } else if let Some(rar_cue) = tas_rar_cue {
        (rar_cue.source_sha256, rar_cue.source_len)
    } else if let Some(zip_cue) = tas_zip_cue {
        (zip_cue.source_sha256, zip_cue.source_len)
    } else {
        (
            loaded_disc.raw_source_media_sha256,
            loaded_disc.raw_source_media_len,
        )
    };
    let provenance = super::super::pce::PceTasLoadProvenanceSeed::new_cd(
        super::super::pce::PceTasCdLoadMedia {
            raw_source_media_sha256,
            raw_source_media_len,
            source_disc_sha256,
            effective_disc_sha256,
            direct: source_path == cue_path
                && (path_extension_is(&cue_path, "cue") || path_extension_is(&cue_path, "chd"))
                || direct_iso
                || direct_ppf.is_some()
                || tas_archive_cue.is_some()
                || tas_rar_cue.is_some()
                || tas_zip_cue.is_some(),
            chd: source_path == cue_path && path_extension_is(&cue_path, "chd"),
            iso: direct_iso,
            ppf: direct_ppf.is_some(),
            archive: tas_archive_cue.is_some(),
            archive_ppf: false,
            rar: tas_rar_cue.is_some(),
            zip: tas_zip_cue.is_some(),
            archive_cue_member_path_sha256: tas_archive_cue
                .map(|identity| identity.cue_member_path_sha256),
            rar_cue_member_path_sha256: tas_rar_cue.map(|identity| identity.cue_member_path_sha256),
            zip_cue_member_path_sha256: tas_zip_cue.map(|identity| identity.cue_member_path_sha256),
            archive_cue_explicitly_selected: tas_archive_cue.is_some_and(|identity| {
                identity.selection
                    == super::super::pce_cd_archive::PceCdArchiveCueSelection::Explicit
            }),
            rar_cue_explicitly_selected: tas_rar_cue.is_some_and(|identity| {
                identity.selection
                    == super::super::pce_cd_archive::PceCdArchiveCueSelection::Explicit
            }),
            zip_cue_explicitly_selected: tas_zip_cue.is_some_and(|identity| {
                identity.selection
                    == super::super::pce_cd_archive::PceCdArchiveCueSelection::Explicit
            }),
            archive_ppf_patches: Vec::new(),
        },
        super::super::pce::PceTasLoadSetup {
            loaded_from_source_path: source_path == cue_path,
            any_mod_enabled: config.apply_mods,
            any_mod_applied: effective_disc_sha256 != source_disc_sha256,
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
    let constructor = if config.pce_load_battery_bram {
        super::super::PceBackend::new_cdrom2
    } else {
        super::super::PceBackend::new_cdrom2_without_host_persistence
    };
    let mut backend = constructor(
        system_card.bytes,
        loaded_disc.disc,
        super::super::pce::PceCdBackendConfig {
            system_card_board,
            cue_path,
            source_path: source_path.to_path_buf(),
            content_hash: loaded_disc.content_sha256,
            content_crc32: loaded_disc.content_crc32,
            source_disc_hash: loaded_disc.source_disc_sha256,
            console_wiring,
            arcade_card_mode: config.pce_arcade_card_mode,
        },
    )?;
    if let Some(sample_rate) = config.sample_rate {
        backend.set_sample_rate(sample_rate);
    }
    backend.update_controller_mode(config.pce_controller_mode);
    backend.update_memory_base_mode(config.pce_memory_base_mode);
    if config.pce_load_battery_bram {
        load_pce_cd_bram(&mut backend);
        super::systems::log_sram_result(backend.try_load_memory_base128());
    }
    backend.set_firmware_manifests(vec![system_card.manifest]);
    let persistent_load = if config.pce_load_battery_bram {
        super::super::pce::PceTasPersistentLoadOutcome::Unknown
    } else {
        super::super::pce::PceTasPersistentLoadOutcome::Skipped
    };
    let provenance = provenance.finish(&backend, persistent_load);
    backend = backend.with_tas_load_provenance(provenance);
    if let Some((buttons, dpad)) = config.initial_input {
        backend.set_input(buttons, dpad);
    }
    Ok(LoadedBackend {
        backend: EmuBackend::from_pce(backend),
        original_crc32: loaded_disc.mod_crc32,
    })
}

#[cfg(not(target_arch = "wasm32"))]
fn selected_archive_member_name(source_path: &Path, cue_path: &Path) -> anyhow::Result<String> {
    let relative = cue_path
        .strip_prefix(source_path)
        .map_err(|_| super::super::pce_cd::PceCdLoadError::ArchiveChanged)?;
    let raw = relative
        .to_str()
        .ok_or(super::super::pce_cd::PceCdLoadError::ArchiveChanged)?;
    super::super::pce_cd::normalize_portable_path(raw)
        .map_err(|_| super::super::pce_cd::PceCdLoadError::ArchiveChanged.into())
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn prepare_pce_cd_7z_backend(
    source_path: &Path,
    expected_cue_path: Option<&Path>,
    config: &BackendLoadConfig,
    cancel: &AtomicBool,
    progress: &super::super::pce_cd_archive::PceCdPackageProgress,
) -> anyhow::Result<(PathBuf, LoadedBackend)> {
    check_package_cancel(cancel)?;
    let (cue_path, loaded_disc) = super::super::pce_cd_archive::load_7z_cue_with_control_and_mods(
        source_path,
        cancel,
        progress,
        config.pce_cd_archive_memory_limit_mib,
        config.apply_mods,
    )?;
    if expected_cue_path.is_some_and(|expected| expected != cue_path) {
        return Err(super::super::pce_cd::PceCdLoadError::ArchiveChanged.into());
    }
    let loaded = finish_prepared_pce_cd_backend(
        source_path,
        &cue_path,
        loaded_disc,
        config,
        cancel,
        progress,
    )?;
    Ok((cue_path, loaded))
}

#[cfg(not(target_arch = "wasm32"))]
fn finish_prepared_pce_cd_backend(
    source_path: &Path,
    cue_path: &Path,
    loaded_disc: super::super::pce_cd::LoadedPceCd,
    config: &BackendLoadConfig,
    cancel: &AtomicBool,
    progress: &super::super::pce_cd_archive::PceCdPackageProgress,
) -> anyhow::Result<LoadedBackend> {
    check_package_cancel(cancel)?;
    progress.set_phase(super::super::pce_cd_archive::PceCdPackageLoadPhase::Firmware);
    let console_wiring = pce_cd_console_wiring(config, loaded_disc.content_sha256);
    let system_card = resolve_pce_cd_system_card(
        config,
        source_path,
        console_wiring,
        loaded_disc.disc.content_hash() == super::super::pce_cd::ADPCM_FIXTURE_DISC_SHA256,
    )?;
    let system_card_profile = pce_system_card_profile(&system_card, console_wiring)?;
    check_minimum_system_card(loaded_disc.source_disc_sha256, system_card_profile)?;
    let system_card_board = pce_system_card_board(system_card_profile);
    check_package_cancel(cancel)?;
    progress.set_phase(super::super::pce_cd_archive::PceCdPackageLoadPhase::Building);
    let constructor = if config.pce_load_battery_bram {
        super::super::PceBackend::new_cdrom2
    } else {
        super::super::PceBackend::new_cdrom2_without_host_persistence
    };
    let mut backend = constructor(
        system_card.bytes,
        loaded_disc.disc,
        super::super::pce::PceCdBackendConfig {
            system_card_board,
            cue_path: cue_path.to_path_buf(),
            source_path: source_path.to_path_buf(),
            content_hash: loaded_disc.content_sha256,
            content_crc32: loaded_disc.content_crc32,
            source_disc_hash: loaded_disc.source_disc_sha256,
            console_wiring,
            arcade_card_mode: config.pce_arcade_card_mode,
        },
    )?;
    if let Some(sample_rate) = config.sample_rate {
        backend.set_sample_rate(sample_rate);
    }
    if config.pce_load_battery_bram {
        load_pce_cd_bram(&mut backend);
        super::systems::log_sram_result(backend.try_load_memory_base128());
    }
    backend.set_firmware_manifests(vec![system_card.manifest]);
    if let Some((buttons, dpad)) = config.initial_input {
        backend.set_input(buttons, dpad);
    }
    check_package_cancel(cancel)?;
    progress.set_phase(super::super::pce_cd_archive::PceCdPackageLoadPhase::Complete);
    Ok(LoadedBackend {
        backend: EmuBackend::from_pce(backend),
        original_crc32: loaded_disc.mod_crc32,
    })
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) enum PreparedSevenZipBackend {
    Ready {
        rom_path: PathBuf,
        system: super::super::ActiveSystem,
        loaded: LoadedBackend,
    },
    Selection(Vec<crate::rom_archive::ArchiveRomEntry>),
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) enum PreparedNativeArchiveBackend {
    Ready {
        rom_path: PathBuf,
        system: super::super::ActiveSystem,
        loaded: LoadedBackend,
    },
    Selection(Vec<crate::rom_archive::ArchiveRomEntry>),
}

#[cfg(not(target_arch = "wasm32"))]
fn check_package_cancel(cancel: &AtomicBool) -> Result<(), super::super::pce_cd::PceCdLoadError> {
    if cancel.load(Ordering::Acquire) {
        Err(super::super::pce_cd::PceCdLoadError::ArchiveCancelled)
    } else {
        Ok(())
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn load_pce_cd_bram(backend: &mut super::super::PceBackend) {
    let rom_path = backend.rom_path().to_path_buf();
    super::systems::log_sram_result(crate::save_paths::try_load_battery_sram(
        &rom_path,
        "PC Engine CD",
        true,
        |bytes| backend.load_cd_bram(bytes),
    ));
}

#[cfg(not(target_arch = "wasm32"))]
fn pce_system_card_profile(
    firmware: &super::super::firmware::ResolvedFirmwareBytes,
    console_wiring: PceConsoleWiring,
) -> Result<zeff_firmware::PceSystemCardFirmware, super::super::pce_cd::PceCdLoadError> {
    #[cfg(test)]
    if firmware.sha256
        == [
            0x8A, 0x39, 0xD2, 0xAB, 0xD3, 0x99, 0x9A, 0xB7, 0x3C, 0x34, 0xDB, 0x24, 0x76, 0x84,
            0x9C, 0xDD, 0xF3, 0x03, 0xCE, 0x38, 0x9B, 0x35, 0x82, 0x68, 0x50, 0xF9, 0xA7, 0x00,
            0x58, 0x9B, 0x4A, 0x90,
        ]
    {
        return Ok(zeff_firmware::classify_pce_system_card_sha256(
            zeff_firmware::PCE_SYSTEM_CARD_ADPCM_FIXTURE_SHA256,
        )
        .unwrap());
    }
    let profile = zeff_firmware::classify_pce_system_card_sha256(firmware.sha256).ok_or(
        super::super::pce_cd::PceCdLoadError::UnrecognizedSystemCardFirmware(firmware.sha256),
    )?;
    let expected = match console_wiring {
        PceConsoleWiring::PcEngine => zeff_firmware::PceSystemCardRegion::Japan,
        PceConsoleWiring::TurboGrafx16 => zeff_firmware::PceSystemCardRegion::Usa,
    };
    if profile.region() != expected {
        return Err(
            super::super::pce_cd::PceCdLoadError::SystemCardRegionMismatch {
                expected,
                actual: profile.region(),
            },
        );
    }
    Ok(profile)
}

fn pce_system_card_board(profile: zeff_firmware::PceSystemCardFirmware) -> PceHuCardBoard {
    match profile.board() {
        zeff_firmware::PceSystemCardBoard::OriginalCdRom2 => PceHuCardBoard::SystemCardV1V2,
        zeff_firmware::PceSystemCardBoard::SuperCdRom2 => PceHuCardBoard::SystemCardV3,
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn check_minimum_system_card(
    source_disc_sha256: [u8; 32],
    selected: zeff_firmware::PceSystemCardFirmware,
) -> Result<(), super::super::pce_cd::PceCdLoadError> {
    let Some(title) = super::super::pce_profiles::canonical_title_metadata(source_disc_sha256)
    else {
        return Ok(());
    };
    let Some(required) = title.minimum_system_card else {
        return Ok(());
    };
    if selected.tier() < required {
        return Err(super::super::pce_cd::PceCdLoadError::SystemCardTierTooLow {
            title: title.title,
            required,
            selected: selected.tier(),
        });
    }
    Ok(())
}

#[cfg(not(target_arch = "wasm32"))]
fn pce_cd_console_wiring(config: &BackendLoadConfig, content_sha256: [u8; 32]) -> PceConsoleWiring {
    const KNOWN_TURBOGRAFX_CD_SHA256: [[u8; 32]; 2] = [
        [
            0x08, 0x02, 0xF4, 0xFA, 0x00, 0x2E, 0x0E, 0x53, 0x2A, 0x30, 0xA7, 0x8B, 0xE5, 0x38,
            0x81, 0x40, 0x7B, 0xE9, 0x40, 0x33, 0x47, 0x2D, 0x95, 0x25, 0xC0, 0x0F, 0xBC, 0x5D,
            0xDE, 0xA8, 0xA1, 0x7F,
        ],
        [
            0x6B, 0xD7, 0x28, 0xCD, 0xF8, 0x7C, 0x6C, 0xEE, 0x9E, 0x96, 0xA0, 0x3D, 0xCE, 0x25,
            0x9C, 0x4C, 0xF3, 0x4F, 0xF3, 0x87, 0xCE, 0xE9, 0xB7, 0x97, 0xA7, 0x35, 0xCF, 0x80,
            0x8E, 0xE7, 0xD2, 0x94,
        ],
    ];
    config.pce_console_wiring.unwrap_or_else(|| {
        if KNOWN_TURBOGRAFX_CD_SHA256.contains(&content_sha256) {
            PceConsoleWiring::TurboGrafx16
        } else {
            PceConsoleWiring::PcEngine
        }
    })
}

fn path_extension_is(path: &Path, expected: &str) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case(expected))
}

#[cfg(not(target_arch = "wasm32"))]
fn resolve_pce_cd_system_card(
    config: &BackendLoadConfig,
    cue_path: &Path,
    console_wiring: PceConsoleWiring,
    require_open_fixture: bool,
) -> anyhow::Result<super::super::firmware::ResolvedFirmwareBytes> {
    #[cfg(test)]
    if let Some(bytes) = config.pce_cd_system_card_override {
        return Ok(super::super::firmware::ResolvedFirmwareBytes {
            bytes: bytes.to_vec(),
            sha256: config
                .pce_cd_system_card_sha256_override
                .unwrap_or_else(|| zeff_firmware::sha256_bytes(bytes)),
            manifest: zeff_emu_common::replay::ReplayFirmwareManifest::External {
                firmware_id: "nec.pce.cd.system_card".to_owned(),
                variant: Some("test-override".to_owned()),
                sha256: config
                    .pce_cd_system_card_sha256_override
                    .unwrap_or_else(|| zeff_firmware::sha256_bytes(bytes)),
            },
        });
    }
    super::super::firmware::resolve_pce_cd_system_card_with_manifest(
        config.firmware_inventory.as_deref(),
        &config.firmware_search_dirs,
        Some(cue_path),
        console_wiring,
        require_open_fixture,
    )
}

#[cfg(target_arch = "wasm32")]
pub(super) fn load_pce_cd_backend(
    _source_path: &Path,
    _cue_path: &Path,
    _preloaded_data: Option<Vec<u8>>,
    _config: &BackendLoadConfig,
) -> anyhow::Result<LoadedBackend> {
    anyhow::bail!("PC Engine CD-ROM2 direct CUE sets are not available in the browser build")
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use std::io::Write;

    use super::*;
    use crate::emu_backend::pce_profiles::LEMMINGS_JAPAN_CANONICAL_DISC_SHA256;
    use rars::rar50::{ArchiveEntry, Rar50Writer, WriterOptions};
    use rars::{ArchiveVersion, EntrySource, FeatureSet};

    fn firmware(sha256: [u8; 32]) -> zeff_firmware::PceSystemCardFirmware {
        zeff_firmware::classify_pce_system_card_sha256(sha256).unwrap()
    }

    #[test]
    fn known_super_cd_title_rejects_system_card_v2() {
        let result = check_minimum_system_card(
            LEMMINGS_JAPAN_CANONICAL_DISC_SHA256,
            firmware(zeff_firmware::PCE_SYSTEM_CARD_V2_JAPAN_SHA256),
        );
        assert_eq!(
            result,
            Err(
                crate::emu_backend::pce_cd::PceCdLoadError::SystemCardTierTooLow {
                    title: "Lemmings",
                    required: zeff_firmware::PceSystemCardTier::Version3,
                    selected: zeff_firmware::PceSystemCardTier::Version2,
                }
            )
        );
        assert_eq!(
            result.unwrap_err().to_string(),
            "Lemmings requires System Card Version3, but the selected firmware is Version2"
        );
    }

    #[test]
    fn known_super_cd_title_accepts_system_card_v3() {
        assert!(
            check_minimum_system_card(
                LEMMINGS_JAPAN_CANONICAL_DISC_SHA256,
                firmware(zeff_firmware::PCE_SYSTEM_CARD_V3_JAPAN_SHA256),
            )
            .is_ok()
        );
    }

    #[test]
    fn unknown_title_remains_allowed_with_system_card_v2() {
        let mut unknown = LEMMINGS_JAPAN_CANONICAL_DISC_SHA256;
        unknown[31] ^= 1;
        assert!(
            check_minimum_system_card(
                unknown,
                firmware(zeff_firmware::PCE_SYSTEM_CARD_V2_JAPAN_SHA256),
            )
            .is_ok()
        );
    }

    fn assert_native_multi_cue_picker(archive: &Path) {
        let system_card: &'static [u8] = Box::leak(vec![0; 262_144].into_boxed_slice());
        let config = BackendLoadConfig {
            pce_cd_system_card_override: Some(system_card),
            pce_cd_system_card_sha256_override: Some(zeff_firmware::PCE_SYSTEM_CARD_V3_USA_SHA256),
            pce_console_wiring: Some(PceConsoleWiring::TurboGrafx16),
            ..BackendLoadConfig::default()
        };
        let cancel = Arc::new(AtomicBool::new(false));
        let progress =
            Arc::new(super::super::super::pce_cd_archive::PceCdPackageProgress::default());
        let selection =
            prepare_native_archive_backend(archive, None, None, &config, &cancel, &progress)
                .unwrap();
        let PreparedNativeArchiveBackend::Selection(entries) = selection else {
            panic!("multi-CUE package did not offer a native archive selection");
        };
        assert_eq!(
            entries
                .iter()
                .map(|entry| entry.name.as_str())
                .collect::<Vec<_>>(),
            ["disc-a/disc.cue", "disc-b/disc.cue"]
        );
        assert!(
            entries
                .iter()
                .all(|entry| entry.system == super::super::ActiveSystem::Pce)
        );

        let selected = prepare_native_archive_backend(
            archive,
            Some(entries[1].index),
            None,
            &config,
            &cancel,
            &progress,
        )
        .unwrap();
        let PreparedNativeArchiveBackend::Ready {
            rom_path, system, ..
        } = selected
        else {
            panic!("selected multi-CUE package entry did not load");
        };
        assert_eq!(rom_path, archive.join("disc-b").join("disc.cue"));
        assert_eq!(system, super::super::ActiveSystem::Pce);

        let reopened = prepare_native_archive_backend(
            archive,
            None,
            Some(&rom_path),
            &config,
            &cancel,
            &progress,
        )
        .unwrap();
        let PreparedNativeArchiveBackend::Ready {
            rom_path: reopened_path,
            ..
        } = reopened
        else {
            panic!("fresh virtual member lookup did not reopen the selected CUE");
        };
        assert_eq!(reopened_path, rom_path);
    }

    #[test]
    fn native_multi_cue_picker_revalidates_selected_rar_member() {
        let archive = std::env::temp_dir().join(format!(
            "zeff-pce-rar-native-multi-cue-{}.rar",
            std::process::id()
        ));
        let cue = b"FILE \"disc.bin\" BINARY\nTRACK 01 MODE1/2048\nINDEX 01 00:00:00\n";
        let entries = [
            (b"disc-a/disc.cue".to_vec(), cue.to_vec()),
            (b"disc-a/disc.bin".to_vec(), vec![0; 2_048]),
            (b"disc-b/disc.cue".to_vec(), cue.to_vec()),
            (b"disc-b/disc.bin".to_vec(), vec![0; 2_048]),
        ]
        .into_iter()
        .map(|(name, data)| {
            ArchiveEntry::new(
                name,
                EntrySource::from_bytes(std::sync::Arc::<[u8]>::from(data)),
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
        std::fs::write(&archive, bytes).unwrap();

        assert_native_multi_cue_picker(&archive);
        let _ = std::fs::remove_file(archive);
    }

    #[test]
    fn native_multi_cue_picker_revalidates_selected_zip_member() {
        let archive = std::env::temp_dir().join(format!(
            "zeff-pce-zip-native-multi-cue-{}.zip",
            std::process::id()
        ));
        let cue = b"FILE \"disc.bin\" BINARY\nTRACK 01 MODE1/2048\nINDEX 01 00:00:00\n";
        let mut writer = zip::ZipWriter::new(std::fs::File::create(&archive).unwrap());
        for (name, data) in [
            ("disc-a/disc.cue", cue.as_slice()),
            ("disc-a/disc.bin", &[0; 2_048]),
            ("disc-b/disc.cue", cue.as_slice()),
            ("disc-b/disc.bin", &[0; 2_048]),
        ] {
            writer
                .start_file(name, zip::write::SimpleFileOptions::default())
                .unwrap();
            writer.write_all(data).unwrap();
        }
        writer.finish().unwrap();

        assert_native_multi_cue_picker(&archive);
        let _ = std::fs::remove_file(archive);
    }

    #[test]
    fn native_zip_preparation_honors_an_explicit_cue_in_mixed_media() {
        let directory = crate::test_support::test_directory("native-pce-mixed-zip").unwrap();
        let archive = directory.path().join("mixed.zip");
        let cue = b"FILE \"disc.bin\" BINARY\nTRACK 01 MODE1/2048\nINDEX 01 00:00:00\n";
        let mut writer = zip::ZipWriter::new(std::fs::File::create(&archive).unwrap());
        for (name, data) in [
            ("game.pce", &[0; 64][..]),
            ("disc/disc.cue", cue.as_slice()),
            ("disc/disc.bin", &[0; 2_048][..]),
        ] {
            writer
                .start_file(name, zip::write::SimpleFileOptions::default())
                .unwrap();
            writer.write_all(data).unwrap();
        }
        writer.finish().unwrap();

        let system_card: &'static [u8] = Box::leak(vec![0; 262_144].into_boxed_slice());
        let config = BackendLoadConfig {
            pce_cd_system_card_override: Some(system_card),
            pce_cd_system_card_sha256_override: Some(zeff_firmware::PCE_SYSTEM_CARD_V3_USA_SHA256),
            pce_console_wiring: Some(PceConsoleWiring::TurboGrafx16),
            ..BackendLoadConfig::default()
        };
        let cancel = Arc::new(AtomicBool::new(false));
        let progress =
            Arc::new(super::super::super::pce_cd_archive::PceCdPackageProgress::default());
        let selected_cue = archive.join("disc").join("disc.cue");
        let prepared = prepare_native_archive_backend(
            &archive,
            None,
            Some(&selected_cue),
            &config,
            &cancel,
            &progress,
        )
        .unwrap();
        let PreparedNativeArchiveBackend::Ready {
            rom_path, system, ..
        } = prepared
        else {
            panic!("explicit CUE did not prepare a PC Engine CD backend");
        };
        assert_eq!(rom_path, selected_cue);
        assert_eq!(system, super::super::ActiveSystem::Pce);
    }
}
