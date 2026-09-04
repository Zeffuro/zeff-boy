use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::Context;
use zeff_gb_core::hardware::types::hardware_mode::HardwareModePreference;
use zeff_pce_core::hardware::{
    PceArcadeCardMode, PceConsoleWiring, PceControllerMode, PceHuCardBoard,
};
use zeff_sega8_core::hardware::cartridge::GameGearStandardMapperRamIdentity;
use zeff_sega8_core::hardware::region::Sega8Region;
use zeff_sega8_core::hardware::timing::Sega8VideoStandard;

use super::{ActiveSystem, EmuBackend};
mod pce_cd;
mod systems;
#[cfg(not(target_arch = "wasm32"))]
mod tas;

#[cfg(not(target_arch = "wasm32"))]
pub(crate) use pce_cd::{PreparedNativeArchiveBackend, prepare_native_archive_backend};
#[cfg(all(not(target_arch = "wasm32"), test))]
pub(crate) use pce_cd::{
    PreparedSevenZipBackend, prepare_pce_cd_7z_backend, prepare_seven_zip_backend,
};
#[cfg(not(target_arch = "wasm32"))]
#[allow(unused_imports)]
pub(crate) use tas::{
    DirectColecoTasExecutionLoader, DirectFdsTasExecutionLoader, DirectGameGearTasExecutionLoader,
    DirectGbTasExecutionLoader, DirectGbaTasExecutionLoader, DirectGbcTasExecutionLoader,
    DirectNesTasExecutionLoader, DirectPceCdTasExecutionLoader, DirectPceTasExecutionLoader,
    DirectSg1000TasExecutionLoader, DirectSmsTasExecutionLoader, DirectWsTasExecutionLoader,
    GbRtcPersistenceWitness, PrivateTasExecutionLoader, classify_direct_tas_execution_profile,
    direct_pce_multitap_cd_ppf_tas_sync_config_sha256, gb_rtc_persistence_witness,
    is_direct_pce_cd_archive_ppf_tas_sync_config_sha256, select_private_tas_execution_attachment,
    select_private_tas_execution_loader, select_private_tas_execution_loader_for_project,
    select_private_tas_execution_loader_for_replay,
    select_private_tas_execution_loader_with_rom_path,
};
#[cfg(all(not(target_arch = "wasm32"), test))]
pub(crate) use tas::{
    MAX_NES_CARTRIDGE_BYTES, direct_nes_tas_identity,
    direct_pce_cd_archive_ppf_tas_sync_configs_for_test, read_nes_cartridge_bounded,
};
#[cfg(not(target_arch = "wasm32"))]
#[allow(unused_imports)]
pub(crate) use tas::{
    TasProjectRuntimeWitness, direct_pce_tas_host_persistence_absent,
    restore_direct_game_gear_tas_private_execution_state, validate_current_nes_start_state,
    validate_direct_coleco_tas_execution_runtime, validate_direct_coleco_tas_runtime,
    validate_direct_coleco_tas_state, validate_direct_game_gear_tas_execution_runtime,
    validate_direct_game_gear_tas_private_execution_runtime,
    validate_direct_game_gear_tas_private_runtime, validate_direct_game_gear_tas_private_state,
    validate_direct_game_gear_tas_runtime, validate_direct_game_gear_tas_state,
    validate_direct_gbc_state_for_backend, validate_direct_gbc_state_for_backend_with_project_rtc,
    validate_direct_gbc_state_for_backend_with_project_sram, validate_direct_gbc_tas_runtime,
    validate_direct_gbc_tas_runtime_with_project_rtc,
    validate_direct_gbc_tas_runtime_with_project_sram,
    validate_direct_pce_cd_tas_execution_runtime, validate_direct_pce_cd_tas_runtime,
    validate_direct_pce_cd_tas_state, validate_direct_pce_multitap_cd_tas_execution_runtime,
    validate_direct_pce_multitap_cd_tas_runtime, validate_direct_pce_multitap_cd_tas_state,
    validate_direct_pce_multitap_tas_execution_runtime, validate_direct_pce_multitap_tas_runtime,
    validate_direct_pce_six_button_tas_execution_runtime,
    validate_direct_pce_six_button_tas_runtime, validate_direct_pce_tas_execution_runtime,
    validate_direct_pce_tas_runtime, validate_direct_pce_tas_state,
    validate_direct_sg1000_tas_execution_runtime, validate_direct_sg1000_tas_runtime,
    validate_direct_sg1000_tas_state, validate_direct_sms_tas_execution_runtime,
    validate_direct_sms_tas_runtime, validate_direct_sms_tas_state,
    validate_direct_ws_tas_execution_runtime, validate_direct_ws_tas_linked_runtime,
    validate_direct_ws_tas_private_execution_runtime, validate_direct_ws_tas_private_runtime,
    validate_direct_ws_tas_private_state, validate_direct_ws_tas_runtime,
    validate_direct_ws_tas_state, validate_fds_tas_branch_scope,
    validate_fds_tas_execution_runtime, validate_fds_tas_private_runtime,
    validate_tas_project_witness,
};
#[cfg(all(not(target_arch = "wasm32"), test))]
pub(crate) use tas::{TestGameGearBoardCatalogGuard, register_test_game_gear_board_catalog_entry};
#[cfg(not(target_arch = "wasm32"))]
#[allow(unused_imports)]
pub(crate) use tas::{
    direct_coleco_tas_sync_config_sha256, direct_game_gear_tas_sync_config_sha256,
    direct_gb_tas_sync_config_sha256, direct_gbc_tas_sync_config_sha256,
    direct_nes_battery_tas_sync_config_sha256, direct_nes_tas_sync_config_sha256,
    direct_sg1000_tas_sync_config_sha256, direct_sms_tas_sync_config_sha256,
    direct_ws_tas_sync_config_sha256, gb_rtc_complete_persistence_bytes,
    is_supported_direct_gb_tas_cartridge, validate_direct_gb_tas_runtime,
    validate_direct_gb_tas_runtime_with_project_rtc,
    validate_direct_gb_tas_runtime_with_project_sram, validate_direct_gb_tas_state,
    zip_gb_battery_tas_sync_config_sha256, zip_gb_tas_sync_config_sha256,
    zip_gbc_battery_tas_sync_config_sha256, zip_gbc_tas_sync_config_sha256,
    zip_nes_battery_tas_sync_config_sha256, zip_nes_tas_sync_config_sha256,
};
#[cfg(all(not(target_arch = "wasm32"), test))]
pub(crate) use tas::{register_test_pce_cd_ppf_stack, register_test_pce_cd_system_card};

#[derive(Clone, Debug)]
pub(crate) struct BackendLoadConfig {
    pub(crate) gb_hardware_mode_preference: HardwareModePreference,
    pub(crate) sample_rate: Option<u32>,
    pub(crate) apply_mods: bool,
    pub(crate) initial_input: Option<(u8, u8)>,
    pub(crate) gb_tas_source_media: Option<([u8; 32], usize, [u8; 32])>,
    pub(crate) gb_load_battery_sram: bool,
    pub(crate) gb_rtc_time_override: Option<u64>,
    pub(crate) gba_load_battery_sram: bool,
    pub(crate) gba_seed_rtc_from_host: bool,
    pub(crate) nes_load_battery_sram: bool,
    pub(crate) sega8_load_battery_sram: bool,
    pub(crate) ws_load_battery_sram: bool,
    pub(crate) game_gear_standard_mapper_ram_identity: Option<GameGearStandardMapperRamIdentity>,
    pub(crate) sega8_video_standard: Option<Sega8VideoStandard>,
    pub(crate) sega8_console_region: Option<Sega8Region>,
    pub(crate) pce_console_wiring: Option<PceConsoleWiring>,
    pub(crate) pce_hucard_board: Option<PceHuCardBoard>,
    pub(crate) pce_cartridge_hardware: Option<zeff_pce_core::hardware::PceCartridgeHardware>,
    pub(crate) pce_cd_tas_source_media: Option<([u8; 32], usize, [u8; 32])>,
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) pce_cd_tas_archive_cue:
        Option<crate::emu_backend::pce_cd_archive::PceCdArchiveCueIdentity>,
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) pce_cd_tas_rar_cue:
        Option<crate::emu_backend::pce_cd_archive::PceCdArchiveCueIdentity>,
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) pce_cd_tas_zip_cue:
        Option<crate::emu_backend::pce_cd_archive::PceCdArchiveCueIdentity>,
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) pce_cd_tas_ppf_stack: Option<crate::emu_backend::pce_cd::PceCdTasPpfStack>,
    pub(crate) pce_controller_mode: zeff_pce_core::hardware::PceControllerMode,
    pub(crate) pce_memory_base_mode: zeff_pce_core::hardware::PceMemoryBaseMode,
    pub(crate) pce_arcade_card_mode: PceArcadeCardMode,
    pub(crate) pce_cd_archive_memory_limit_mib: usize,
    pub(crate) pce_load_battery_bram: bool,
    pub(crate) firmware_search_dirs: Vec<PathBuf>,
    pub(crate) firmware_inventory: Option<Arc<zeff_firmware::FirmwareInventory>>,
    pub(crate) gb_use_external_boot_rom: bool,
    pub(crate) gba_use_external_bios: bool,
    pub(crate) sega8_use_external_boot_rom: bool,
    #[cfg(test)]
    pub(crate) fds_bios_override: Option<&'static [u8]>,
    #[cfg(test)]
    pub(crate) coleco_bios_override: Option<&'static [u8]>,
    #[cfg(test)]
    pub(crate) pce_cd_system_card_override: Option<&'static [u8]>,
    #[cfg(test)]
    pub(crate) pce_cd_system_card_sha256_override: Option<[u8; 32]>,
}

impl Default for BackendLoadConfig {
    fn default() -> Self {
        Self {
            gb_hardware_mode_preference: HardwareModePreference::Auto,
            sample_rate: None,
            apply_mods: false,
            initial_input: None,
            gb_tas_source_media: None,
            gb_load_battery_sram: true,
            gb_rtc_time_override: None,
            gba_load_battery_sram: true,
            gba_seed_rtc_from_host: true,
            nes_load_battery_sram: true,
            sega8_load_battery_sram: true,
            ws_load_battery_sram: true,
            game_gear_standard_mapper_ram_identity: None,
            sega8_video_standard: None,
            sega8_console_region: None,
            pce_console_wiring: None,
            pce_hucard_board: None,
            pce_cartridge_hardware: None,
            pce_cd_tas_source_media: None,
            #[cfg(not(target_arch = "wasm32"))]
            pce_cd_tas_archive_cue: None,
            #[cfg(not(target_arch = "wasm32"))]
            pce_cd_tas_rar_cue: None,
            #[cfg(not(target_arch = "wasm32"))]
            pce_cd_tas_zip_cue: None,
            #[cfg(not(target_arch = "wasm32"))]
            pce_cd_tas_ppf_stack: None,
            pce_controller_mode: zeff_pce_core::hardware::PceControllerMode::Automatic,
            pce_memory_base_mode: zeff_pce_core::hardware::PceMemoryBaseMode::Automatic,
            pce_arcade_card_mode: PceArcadeCardMode::Automatic,
            pce_cd_archive_memory_limit_mib: 128,
            pce_load_battery_bram: true,
            firmware_search_dirs: Vec::new(),
            firmware_inventory: None,
            gb_use_external_boot_rom: false,
            gba_use_external_bios: false,
            sega8_use_external_boot_rom: false,
            #[cfg(test)]
            fds_bios_override: None,
            #[cfg(test)]
            coleco_bios_override: None,
            #[cfg(test)]
            pce_cd_system_card_override: None,
            #[cfg(test)]
            pce_cd_system_card_sha256_override: None,
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn zip_tas_source_media(
    source_path: &Path,
    rom_path: &Path,
    extension: &str,
    max_member_bytes: u64,
    raw_sha256: [u8; 32],
    sync: impl FnOnce(&str) -> [u8; 32],
) -> Option<([u8; 32], usize, [u8; 32])> {
    has_extension(source_path, "zip")
        .then(|| {
            crate::rom_archive::extract_bounded_zip_member(
                source_path,
                Some(rom_path),
                extension,
                128 * 1024 * 1024,
                max_member_bytes,
            )
        })?
        .ok()
        .filter(|selected| zeff_firmware::sha256_bytes(&selected.bytes) == raw_sha256)
        .map(|selected| {
            let sync = sync(&selected.member_name);
            (selected.archive_sha256, selected.archive_len, sync)
        })
}

fn has_extension(path: &Path, extension: &str) -> bool {
    path.extension()
        .and_then(|value| value.to_str())
        .is_some_and(|value| value.eq_ignore_ascii_case(extension))
}

pub(crate) struct LoadedBackend {
    pub(crate) backend: EmuBackend,
    pub(crate) original_crc32: u32,
}

pub(crate) fn load_backend_from_rom_source(
    system: ActiveSystem,
    source_path: &Path,
    rom_path: &Path,
    preloaded_data: Option<Vec<u8>>,
    config: BackendLoadConfig,
) -> anyhow::Result<LoadedBackend> {
    let loaded_from_source_path = preloaded_data.is_none();
    load_backend_from_rom_source_inner(
        system,
        source_path,
        rom_path,
        preloaded_data,
        config,
        loaded_from_source_path,
    )
}

#[cfg(not(target_arch = "wasm32"))]
pub(in crate::emu_backend::loader) fn load_backend_from_bounded_direct_source(
    system: ActiveSystem,
    source_path: &Path,
    source_data: Vec<u8>,
    config: BackendLoadConfig,
) -> anyhow::Result<LoadedBackend> {
    load_backend_from_rom_source_inner(
        system,
        source_path,
        source_path,
        Some(source_data),
        config,
        true,
    )
}

fn load_backend_from_rom_source_inner(
    system: ActiveSystem,
    source_path: &Path,
    rom_path: &Path,
    preloaded_data: Option<Vec<u8>>,
    config: BackendLoadConfig,
    loaded_from_source_path: bool,
) -> anyhow::Result<LoadedBackend> {
    if system == ActiveSystem::Pce && pce_cd::is_pce_cd_path(rom_path) {
        return pce_cd::load_pce_cd_backend(source_path, rom_path, preloaded_data, &config);
    }
    let mut rom_data = match preloaded_data {
        Some(data) => data,
        None => std::fs::read(source_path).with_context(|| {
            format!("Failed to read {} ROM", systems::system_load_label(system))
        })?,
    };

    let raw_source_media_sha256 = matches!(
        system,
        ActiveSystem::GameBoy
            | ActiveSystem::GameBoyAdvance
            | ActiveSystem::Nes
            | ActiveSystem::Coleco
            | ActiveSystem::MasterSystem
            | ActiveSystem::GameGear
            | ActiveSystem::Sg1000
            | ActiveSystem::Pce
            | ActiveSystem::WonderSwan
    )
    .then(|| zeff_firmware::sha256_bytes(&rom_data));
    let raw_source_media_len = rom_data.len();
    #[cfg(not(target_arch = "wasm32"))]
    let nes_tas_media = if system != ActiveSystem::Nes {
        None
    } else if loaded_from_source_path
        && source_path == rom_path
        && source_path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| extension.eq_ignore_ascii_case("nes"))
    {
        Some((
            raw_source_media_sha256.expect("NES source hash must exist for NES"),
            tas::direct_nes_tas_sync_config_sha256().0,
            tas::direct_nes_battery_tas_sync_config_sha256().0,
        ))
    } else if source_path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("zip"))
    {
        crate::rom_archive::extract_bounded_zip_member(
            source_path,
            Some(rom_path),
            "nes",
            tas::MAX_NES_ZIP_BYTES,
            tas::MAX_NES_CARTRIDGE_BYTES,
        )
        .ok()
        .filter(|selected| {
            zeff_firmware::sha256_bytes(&selected.bytes)
                == raw_source_media_sha256.expect("NES source hash must exist for NES")
        })
        .map(|selected| {
            (
                selected.archive_sha256,
                tas::zip_nes_tas_sync_config_sha256(&selected.member_name).0,
                tas::zip_nes_battery_tas_sync_config_sha256(&selected.member_name).0,
            )
        })
    } else {
        None
    };
    #[cfg(target_arch = "wasm32")]
    let nes_tas_media: Option<([u8; 32], [u8; 32], [u8; 32])> = None;
    #[cfg(not(target_arch = "wasm32"))]
    let gba_tas_media = if system != ActiveSystem::GameBoyAdvance {
        None
    } else if source_path == rom_path
        && source_path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| extension.eq_ignore_ascii_case("gba"))
    {
        Some((
            raw_source_media_sha256.expect("GBA source hash must exist for GBA"),
            raw_source_media_len,
            super::gba::direct_gba_tas_sync_config_sha256().0,
        ))
    } else if source_path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("zip"))
    {
        crate::rom_archive::extract_bounded_zip_member(
            source_path,
            Some(rom_path),
            "gba",
            128 * 1024 * 1024,
            super::gba::MAX_DIRECT_GBA_ROM_BYTES,
        )
        .ok()
        .filter(|selected| {
            zeff_firmware::sha256_bytes(&selected.bytes)
                == raw_source_media_sha256.expect("GBA source hash must exist for GBA")
        })
        .map(|selected| {
            (
                selected.archive_sha256,
                selected.archive_len,
                super::gba::zip_gba_tas_sync_config_sha256(&selected.member_name).0,
            )
        })
    } else {
        None
    };
    #[cfg(target_arch = "wasm32")]
    let gba_tas_media: Option<([u8; 32], usize, [u8; 32])> = None;
    #[cfg(not(target_arch = "wasm32"))]
    let pce_tas_media = if system != ActiveSystem::Pce {
        None
    } else if source_path == rom_path
        && source_path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| extension.eq_ignore_ascii_case("pce"))
    {
        tas::classify_direct_pce_tas_hardware(&rom_data)
            .ok()
            .map(|mut profile| {
                if config.pce_controller_mode != PceControllerMode::Automatic {
                    profile.controller_mode = config.pce_controller_mode;
                }
                (
                    raw_source_media_sha256
                        .expect("PC Engine source hash must exist for PC Engine"),
                    raw_source_media_len,
                    tas::direct_pce_tas_sync_config_sha256_for_profile(profile).0,
                )
            })
    } else if source_path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("zip"))
    {
        crate::rom_archive::extract_bounded_zip_member(
            source_path,
            Some(rom_path),
            "pce",
            128 * 1024 * 1024,
            tas::MAX_DIRECT_PCE_HUCARD_BYTES,
        )
        .ok()
        .filter(|selected| {
            zeff_firmware::sha256_bytes(&selected.bytes)
                == raw_source_media_sha256.expect("PC Engine source hash must exist for PC Engine")
        })
        .and_then(|selected| {
            let mut profile = tas::classify_direct_pce_tas_hardware(&selected.bytes).ok()?;
            if config.pce_controller_mode != PceControllerMode::Automatic {
                profile.controller_mode = config.pce_controller_mode;
            }
            Some((
                selected.archive_sha256,
                selected.archive_len,
                tas::zip_pce_tas_sync_config_sha256_for_profile(profile, &selected.member_name).0,
            ))
        })
    } else {
        None
    };
    #[cfg(target_arch = "wasm32")]
    let pce_tas_media: Option<([u8; 32], usize, [u8; 32])> = None;
    let mod_load = if config.apply_mods {
        systems::apply_mods_if_any(system, &mut rom_data)
    } else {
        systems::ModLoadOutcome {
            original_crc32: crc32fast::hash(&rom_data),
            any_enabled: false,
            any_applied: false,
        }
    };
    let coleco_provenance = (system == ActiveSystem::Coleco).then(|| {
        super::coleco::ColecoTasLoadProvenanceSeed::new(
            raw_source_media_sha256.expect("Coleco source hash must exist for Coleco"),
            raw_source_media_len,
            source_path,
            rom_path,
            super::coleco::ColecoTasLoadSetup {
                loaded_from_source_path,
                any_mod_enabled: mod_load.any_enabled,
                any_mod_applied: mod_load.any_applied,
                initial_input: config.initial_input,
                configured_sample_rate: config.sample_rate,
                tas_source_media: {
                    #[cfg(not(target_arch = "wasm32"))]
                    {
                        zip_tas_source_media(
                            source_path,
                            rom_path,
                            "col",
                            zeff_coleco_core::constants::MAX_CARTRIDGE_SIZE as u64,
                            raw_source_media_sha256
                                .expect("Coleco source hash must exist for Coleco"),
                            |member| tas::zip_coleco_tas_sync_config_sha256(member).0,
                        )
                        .or(Some((
                            raw_source_media_sha256
                                .expect("Coleco source hash must exist for Coleco"),
                            raw_source_media_len,
                            tas::direct_coleco_tas_sync_config_sha256().0,
                        )))
                    }
                    #[cfg(target_arch = "wasm32")]
                    {
                        None
                    }
                },
            },
        )
    });
    let nes_provenance = (system == ActiveSystem::Nes).then(|| {
        super::nes::NesTasLoadProvenanceSeed::new(
            raw_source_media_sha256.expect("NES source hash must exist for NES"),
            source_path,
            rom_path,
            super::nes::NesTasLoadSetup {
                loaded_from_source_path,
                any_mod_enabled: mod_load.any_enabled,
                any_mod_applied: mod_load.any_applied,
                initial_input: config.initial_input,
                configured_sample_rate: config.sample_rate,
                tas_source_media_sha256: nes_tas_media.map(|media| media.0),
                tas_sync_config_sha256: nes_tas_media.map(|media| media.1),
                tas_battery_sync_config_sha256: nes_tas_media.map(|media| media.2),
            },
        )
    });
    let gb_provenance = (system == ActiveSystem::GameBoy).then(|| {
        super::gb::GbTasLoadProvenanceSeed::new(
            raw_source_media_sha256.expect("GB source hash must exist for GB"),
            raw_source_media_len,
            source_path,
            rom_path,
            super::gb::GbTasLoadSetup {
                loaded_from_source_path,
                any_mod_enabled: mod_load.any_enabled,
                any_mod_applied: mod_load.any_applied,
                initial_input: config.initial_input,
                configured_sample_rate: config.sample_rate,
                requested_hardware_mode: config.gb_hardware_mode_preference,
                tas_source_media: config.gb_tas_source_media,
                rtc_time_override: config.gb_rtc_time_override,
            },
        )
    });
    let gba_provenance = (system == ActiveSystem::GameBoyAdvance).then(|| {
        super::gba::GbaTasLoadProvenanceSeed::new(
            raw_source_media_sha256.expect("GBA source hash must exist for GBA"),
            raw_source_media_len,
            source_path,
            rom_path,
            super::gba::GbaTasLoadSetup {
                loaded_from_source_path,
                any_mod_enabled: mod_load.any_enabled,
                any_mod_applied: mod_load.any_applied,
                initial_input: config.initial_input,
                configured_sample_rate: config.sample_rate,
                external_bios_selected: config.gba_use_external_bios,
                tas_source_media: gba_tas_media,
            },
        )
    });
    let sega8_provenance = match system {
        ActiveSystem::MasterSystem => Some(super::sega8::Sega8TasLoadProvenanceSeed::MasterSystem(
            super::sega8::SmsTasLoadProvenanceSeed::new(
                raw_source_media_sha256.expect("SMS source hash must exist for Master System"),
                raw_source_media_len,
                source_path,
                rom_path,
                super::sega8::SmsTasLoadSetup {
                    loaded_from_source_path,
                    any_mod_enabled: mod_load.any_enabled,
                    any_mod_applied: mod_load.any_applied,
                    initial_input: config.initial_input,
                    configured_sample_rate: config.sample_rate,
                    tas_source_media: {
                        #[cfg(not(target_arch = "wasm32"))]
                        {
                            zip_tas_source_media(
                                source_path,
                                rom_path,
                                "sms",
                                tas::MAX_DIRECT_SMS_ROM_BYTES,
                                raw_source_media_sha256
                                    .expect("SMS source hash must exist for Master System"),
                                |member| tas::zip_sms_tas_sync_config_sha256(member).0,
                            )
                            .or(Some((
                                raw_source_media_sha256
                                    .expect("SMS source hash must exist for Master System"),
                                raw_source_media_len,
                                tas::direct_sms_tas_sync_config_sha256().0,
                            )))
                        }
                        #[cfg(target_arch = "wasm32")]
                        {
                            None
                        }
                    },
                },
            ),
        )),
        ActiveSystem::GameGear => Some(super::sega8::Sega8TasLoadProvenanceSeed::GameGear(
            super::sega8::GameGearTasLoadProvenanceSeed::new(
                raw_source_media_sha256.expect("Game Gear source hash must exist for Game Gear"),
                raw_source_media_len,
                source_path,
                rom_path,
                super::sega8::GameGearTasLoadSetup {
                    loaded_from_source_path,
                    any_mod_enabled: mod_load.any_enabled,
                    any_mod_applied: mod_load.any_applied,
                    initial_input: config.initial_input,
                    configured_sample_rate: config.sample_rate,
                    standard_mapper_ram_identity: config
                        .game_gear_standard_mapper_ram_identity
                        .or_else(|| {
                            zeff_sega8_core::hardware::cartridge::game_gear_standard_mapper_ram_for_identity(
                                zeff_sega8_core::hardware::cartridge::GameGearCartridgeIdentity {
                                    sha256: raw_source_media_sha256.expect(
                                        "Game Gear source hash must exist for Game Gear",
                                    ),
                                    source_len: raw_source_media_len,
                                },
                            )
                        }),
                    tas_source_media: {
                        #[cfg(not(target_arch = "wasm32"))]
                        {
                            zip_tas_source_media(
                                source_path,
                                rom_path,
                                "gg",
                                tas::MAX_DIRECT_GAME_GEAR_ROM_BYTES,
                                raw_source_media_sha256
                                    .expect("Game Gear source hash must exist for Game Gear"),
                                |member| {
                                    tas::zip_game_gear_tas_sync_config_sha256(
                                        tas::DirectGameGearTasBoardChoice::CataloguedAbsent,
                                        member,
                                    )
                                    .0
                                },
                            )
                            .or(Some((
                                raw_source_media_sha256
                                    .expect("Game Gear source hash must exist for Game Gear"),
                                raw_source_media_len,
                                tas::direct_game_gear_tas_sync_config_sha256().0,
                            )))
                        }
                        #[cfg(target_arch = "wasm32")]
                        {
                            None
                        }
                    },
                },
            ),
        )),
        ActiveSystem::Sg1000 => Some(super::sega8::Sega8TasLoadProvenanceSeed::Sg1000(
            super::sega8::Sg1000TasLoadProvenanceSeed::new(
                raw_source_media_sha256.expect("SG-1000 source hash must exist for SG-1000"),
                raw_source_media_len,
                source_path,
                rom_path,
                super::sega8::Sg1000TasLoadSetup {
                    loaded_from_source_path,
                    any_mod_enabled: mod_load.any_enabled,
                    any_mod_applied: mod_load.any_applied,
                    initial_input: config.initial_input,
                    configured_sample_rate: config.sample_rate,
                    tas_source_media: {
                        #[cfg(not(target_arch = "wasm32"))]
                        {
                            zip_tas_source_media(
                                source_path,
                                rom_path,
                                rom_path
                                    .extension()
                                    .and_then(|value| value.to_str())
                                    .unwrap_or("sg"),
                                tas::MAX_DIRECT_SG1000_ROM_BYTES,
                                raw_source_media_sha256
                                    .expect("SG-1000 source hash must exist for SG-1000"),
                                |member| tas::zip_sg1000_tas_sync_config_sha256(member).0,
                            )
                            .or(Some((
                                raw_source_media_sha256
                                    .expect("SG-1000 source hash must exist for SG-1000"),
                                raw_source_media_len,
                                tas::direct_sg1000_tas_sync_config_sha256().0,
                            )))
                        }
                        #[cfg(target_arch = "wasm32")]
                        {
                            None
                        }
                    },
                },
            ),
        )),
        _ => None,
    };
    let ws_provenance = (system == ActiveSystem::WonderSwan).then(|| {
        super::ws::WsTasLoadProvenanceSeed::new(
            raw_source_media_sha256.expect("WonderSwan source hash must exist for WonderSwan"),
            raw_source_media_len,
            source_path,
            rom_path,
            super::ws::WsTasLoadSetup {
                loaded_from_source_path,
                any_mod_enabled: mod_load.any_enabled,
                any_mod_applied: mod_load.any_applied,
                initial_input: config.initial_input,
                configured_sample_rate: config.sample_rate,
                tas_source_media: {
                    #[cfg(not(target_arch = "wasm32"))]
                    {
                        let system = if rom_path
                            .extension()
                            .and_then(|extension| extension.to_str())
                            .is_some_and(|extension| extension.eq_ignore_ascii_case("wsc"))
                        {
                            zeff_ws_core::hardware::cartridge::MinimumSystem::WonderSwanColor
                        } else {
                            zeff_ws_core::hardware::cartridge::MinimumSystem::WonderSwan
                        };
                        let orientation = if rom_data
                            .get(rom_data.len().saturating_sub(4))
                            .is_some_and(|value| value & 1 != 0)
                        {
                            zeff_ws_core::hardware::cartridge::RomOrientation::Vertical
                        } else {
                            zeff_ws_core::hardware::cartridge::RomOrientation::Horizontal
                        };
                        let save_kind = rom_data
                            .get(rom_data.len().saturating_sub(5))
                            .copied()
                            .map(zeff_ws_core::hardware::cartridge::SaveKind::from_byte)
                            .unwrap_or(zeff_ws_core::hardware::cartridge::SaveKind::None);
                        let battery =
                            save_kind != zeff_ws_core::hardware::cartridge::SaveKind::None;
                        let rtc = rom_data
                            .get(rom_data.len().saturating_sub(3))
                            .is_some_and(|value| *value != 0);
                        zip_tas_source_media(
                            source_path,
                            rom_path,
                            rom_path
                                .extension()
                                .and_then(|value| value.to_str())
                                .unwrap_or("ws"),
                            tas::MAX_DIRECT_WS_ROM_BYTES,
                            raw_source_media_sha256
                                .expect("WonderSwan source hash must exist for WonderSwan"),
                            |member| {
                                if rtc {
                                    tas::zip_ws_rtc_tas_sync_config_sha256(
                                        system,
                                        orientation,
                                        save_kind,
                                        member,
                                    )
                                    .map(|sync| sync.0)
                                    .unwrap_or([0; 32])
                                } else {
                                    tas::zip_ws_tas_sync_config_sha256(
                                        system,
                                        orientation,
                                        battery,
                                        member,
                                    )
                                    .map(|sync| sync.0)
                                    .unwrap_or([0; 32])
                                }
                            },
                        )
                        .or_else(|| {
                            Some((
                                raw_source_media_sha256
                                    .expect("WonderSwan source hash must exist for WonderSwan"),
                                raw_source_media_len,
                                if rtc {
                                    tas::direct_ws_rtc_tas_sync_config_sha256(
                                        system,
                                        orientation,
                                        save_kind,
                                    )
                                    .map(|sync| sync.0)
                                    .unwrap_or([0; 32])
                                } else if battery {
                                    tas::direct_ws_battery_tas_sync_config_sha256(
                                        system,
                                        orientation,
                                    )
                                    .map(|sync| sync.0)
                                    .unwrap_or([0; 32])
                                } else {
                                    tas::direct_ws_tas_sync_config_sha256(system, orientation)
                                        .map(|sync| sync.0)
                                        .unwrap_or([0; 32])
                                },
                            ))
                        })
                    }
                    #[cfg(target_arch = "wasm32")]
                    {
                        None
                    }
                },
            },
        )
    });
    let pce_provenance = (system == ActiveSystem::Pce).then(|| {
        super::pce::PceTasLoadProvenanceSeed::new(
            raw_source_media_sha256.expect("PC Engine source hash must exist for PC Engine"),
            raw_source_media_len,
            source_path,
            rom_path,
            super::pce::PceTasLoadSetup {
                loaded_from_source_path,
                any_mod_enabled: mod_load.any_enabled,
                any_mod_applied: mod_load.any_applied,
                initial_input: config.initial_input,
                configured_sample_rate: config.sample_rate,
                selected_wiring: config.pce_console_wiring,
                selected_board: config.pce_hucard_board,
                selected_hardware: config.pce_cartridge_hardware,
                selected_controller_mode: config.pce_controller_mode,
                selected_memory_base_mode: config.pce_memory_base_mode,
                selected_arcade_card_mode: config.pce_arcade_card_mode,
                tas_source_media: pce_tas_media,
            },
        )
    });

    let default_firmware_manifests =
        super::firmware::default_firmware_manifests_for_active_system(system);
    let mut backend = match system {
        ActiveSystem::GameBoy => systems::load_gb_backend(
            &rom_data,
            source_path,
            rom_path,
            &config,
            gb_provenance.expect("GB load provenance must exist for GB"),
        )?,
        ActiveSystem::Nes => systems::load_nes_backend(
            &rom_data,
            source_path,
            rom_path,
            &config,
            nes_provenance.expect("NES load provenance must exist for NES"),
        )?,
        ActiveSystem::Coleco => systems::load_coleco_backend(
            &rom_data,
            source_path,
            rom_path,
            &config,
            coleco_provenance.expect("Coleco load provenance must exist for Coleco"),
        )?,
        ActiveSystem::Pce => systems::load_pce_backend(
            &rom_data,
            source_path,
            rom_path,
            &config,
            pce_provenance.expect("PC Engine load provenance must exist for PC Engine"),
        )?,
        ActiveSystem::GameBoyAdvance => systems::load_gba_backend(
            &rom_data,
            source_path,
            rom_path,
            &config,
            gba_provenance.expect("GBA load provenance must exist for GBA"),
        )?,
        ActiveSystem::WonderSwan => systems::load_ws_backend(
            &rom_data,
            source_path,
            rom_path,
            &config,
            ws_provenance.expect("WonderSwan load provenance must exist for WonderSwan"),
        )?,
        ActiveSystem::MasterSystem | ActiveSystem::GameGear | ActiveSystem::Sg1000 => {
            systems::load_sega8_backend(
                system,
                &rom_data,
                source_path,
                rom_path,
                &config,
                sega8_provenance,
            )?
        }
    };

    if !default_firmware_manifests.is_empty()
        && !(system == ActiveSystem::GameBoy && config.gb_use_external_boot_rom)
        && !(system == ActiveSystem::GameBoyAdvance && config.gba_use_external_bios)
        && system != ActiveSystem::Coleco
        && !(matches!(system, ActiveSystem::MasterSystem | ActiveSystem::GameGear)
            && config.sega8_use_external_boot_rom)
    {
        backend.set_firmware_manifests(default_firmware_manifests);
    }
    if let Some((buttons, dpad)) = config.initial_input {
        backend.set_input(buttons, dpad);
    }
    Ok(LoadedBackend {
        backend,
        original_crc32: mod_load.original_crc32,
    })
}
