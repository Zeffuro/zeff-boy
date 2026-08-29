use std::path::{Path, PathBuf};
use std::sync::Arc;
#[cfg(not(target_arch = "wasm32"))]
use std::sync::atomic::{AtomicBool, Ordering};

use zeff_pce_core::hardware::{PceConsoleWiring, PceHuCardBoard};

use super::{BackendLoadConfig, EmuBackend, LoadedBackend};
use crate::emu_core_trait::EmulatorCore;

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
    let (cue_path, loaded_disc) = if source_path == cue_path && path_extension_is(cue_path, "cue") {
        (
            cue_path.to_path_buf(),
            super::super::pce_cd::load_direct_cue_with_mods(cue_path, config.apply_mods)?,
        )
    } else if source_path == cue_path && path_extension_is(cue_path, "chd") {
        (
            cue_path.to_path_buf(),
            super::super::pce_cd::load_direct_chd_with_mods(cue_path, config.apply_mods)?,
        )
    } else if source_path == cue_path && path_extension_is(cue_path, "iso") {
        let actual_cue = super::super::pce_cd::cue_path_for_iso(cue_path)?;
        let loaded =
            super::super::pce_cd::load_direct_cue_with_mods(&actual_cue, config.apply_mods)?;
        (actual_cue, loaded)
    } else if path_extension_is(source_path, "7z") {
        let cancel = AtomicBool::new(false);
        let progress = super::super::pce_cd_archive::PceCdPackageProgress::default();
        let (actual, loaded) = super::super::pce_cd_archive::load_7z_cue_with_control_and_mods(
            source_path,
            &cancel,
            &progress,
            config.pce_cd_archive_memory_limit_mib,
            config.apply_mods,
        )?;
        if actual != cue_path {
            return Err(super::super::pce_cd::PceCdLoadError::ArchiveChanged.into());
        }
        (actual, loaded)
    } else if path_extension_is(source_path, "rar") {
        let cancel = Arc::new(AtomicBool::new(false));
        let progress = Arc::new(super::super::pce_cd_archive::PceCdPackageProgress::default());
        let (actual, loaded) = super::super::pce_cd_rar::load_rar_cue_with_control_and_mods(
            source_path,
            cancel,
            progress,
            config.apply_mods,
        )?;
        if actual != cue_path {
            return Err(super::super::pce_cd::PceCdLoadError::ArchiveChanged.into());
        }
        (actual, loaded)
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
    let mut backend = super::super::PceBackend::new_cdrom2(
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
    if config.pce_load_battery_bram {
        load_pce_cd_bram(&mut backend);
        super::systems::log_sram_result(backend.try_load_memory_base128());
    }
    backend.set_firmware_manifests(vec![system_card.manifest]);
    if let Some((buttons, dpad)) = config.initial_input {
        backend.set_input(buttons, dpad);
    }
    Ok(LoadedBackend {
        backend: EmuBackend::from_pce(backend),
        original_crc32: loaded_disc.mod_crc32,
    })
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
    let mut backend = super::super::PceBackend::new_cdrom2(
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
pub(crate) fn prepare_seven_zip_backend(
    source_path: &Path,
    selected_entry_index: Option<usize>,
    expected_rom_path: Option<&Path>,
    config: &BackendLoadConfig,
    cancel: &AtomicBool,
    progress: &super::super::pce_cd_archive::PceCdPackageProgress,
) -> anyhow::Result<PreparedSevenZipBackend> {
    check_package_cancel(cancel)?;
    match super::super::pce_cd_archive::inspect_7z_contents(
        source_path,
        config.pce_cd_archive_memory_limit_mib,
    )? {
        super::super::pce_cd_archive::SevenZipContents::Cd { cue_path } => {
            if selected_entry_index.is_some()
                || expected_rom_path.is_some_and(|expected| expected != cue_path)
            {
                return Err(super::super::pce_cd::PceCdLoadError::ArchiveChanged.into());
            }
            let (actual, loaded) =
                prepare_pce_cd_7z_backend(source_path, Some(&cue_path), config, cancel, progress)?;
            Ok(PreparedSevenZipBackend::Ready {
                rom_path: actual,
                system: super::super::ActiveSystem::Pce,
                loaded,
            })
        }
        super::super::pce_cd_archive::SevenZipContents::Roms(entries) => {
            let selected = if let Some(index) = selected_entry_index {
                entries.iter().find(|entry| entry.index == index)
            } else if let Some(expected) = expected_rom_path {
                entries.iter().find(|entry| {
                    entry
                        .name
                        .split('/')
                        .fold(source_path.to_path_buf(), |path, part| path.join(part))
                        == expected
                })
            } else if entries.len() == 1 {
                entries.first()
            } else {
                return Ok(PreparedSevenZipBackend::Selection(entries));
            }
            .ok_or(super::super::pce_cd::PceCdLoadError::ArchiveChanged)?;
            let (rom_path, bytes, system) =
                super::super::pce_cd_archive::load_7z_rom_entry_with_control(
                    source_path,
                    selected.index,
                    cancel,
                    progress,
                    config.pce_cd_archive_memory_limit_mib,
                )?;
            check_package_cancel(cancel)?;
            progress.set_phase(super::super::pce_cd_archive::PceCdPackageLoadPhase::Building);
            let loaded = super::load_backend_from_rom_source(
                system,
                source_path,
                &rom_path,
                Some(bytes),
                config.clone(),
            )?;
            check_package_cancel(cancel)?;
            progress.set_phase(super::super::pce_cd_archive::PceCdPackageLoadPhase::Complete);
            Ok(PreparedSevenZipBackend::Ready {
                rom_path,
                system,
                loaded,
            })
        }
    }
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
pub(crate) fn prepare_native_archive_backend(
    source_path: &Path,
    selected_entry_index: Option<usize>,
    expected_rom_path: Option<&Path>,
    config: &BackendLoadConfig,
    cancel: &Arc<AtomicBool>,
    progress: &Arc<super::super::pce_cd_archive::PceCdPackageProgress>,
) -> anyhow::Result<PreparedNativeArchiveBackend> {
    if path_extension_is(source_path, "7z") {
        return Ok(
            match prepare_seven_zip_backend(
                source_path,
                selected_entry_index,
                expected_rom_path,
                config,
                cancel,
                progress,
            )? {
                PreparedSevenZipBackend::Ready {
                    rom_path,
                    system,
                    loaded,
                } => PreparedNativeArchiveBackend::Ready {
                    rom_path,
                    system,
                    loaded,
                },
                PreparedSevenZipBackend::Selection(entries) => {
                    PreparedNativeArchiveBackend::Selection(entries)
                }
            },
        );
    }
    if !path_extension_is(source_path, "rar") || selected_entry_index.is_some() {
        return Err(super::super::pce_cd::PceCdLoadError::PackagedCdSetUnsupported.into());
    }
    check_package_cancel(cancel)?;
    let cue_path = super::super::pce_cd_rar::inspect_rar_cue_path(source_path)?;
    if expected_rom_path.is_some_and(|expected| expected != cue_path) {
        return Err(super::super::pce_cd::PceCdLoadError::ArchiveChanged.into());
    }
    let (actual, loaded_disc) = super::super::pce_cd_rar::load_rar_cue_with_control_and_mods(
        source_path,
        Arc::clone(cancel),
        Arc::clone(progress),
        config.apply_mods,
    )?;
    if actual != cue_path {
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
    Ok(PreparedNativeArchiveBackend::Ready {
        rom_path: cue_path,
        system: super::super::ActiveSystem::Pce,
        loaded,
    })
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
    use super::*;
    use crate::emu_backend::pce_profiles::LEMMINGS_JAPAN_CANONICAL_DISC_SHA256;

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
}
