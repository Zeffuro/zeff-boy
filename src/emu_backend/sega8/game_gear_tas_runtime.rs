use anyhow::{Context, Result, ensure};
use zeff_emu_common::replay::ReplayFirmwareManifest;
use zeff_emu_common::save_ram::SaveRamKind;

use super::game_gear_tas_provenance::{
    GameGearTasControllerModel, GameGearTasPersistentLoadOutcome,
};
use crate::emu_backend::{ActiveSystem, EmuBackend};

const MAX_DIRECT_GAME_GEAR_ROM_BYTES: usize = 8 * 1024 * 1024;

#[derive(Clone, Copy)]
struct GameGearMachineFacts {
    rom_matches: bool,
    mapper_kind: zeff_sega8_core::hardware::cartridge::Sega8MapperKind,
    video_standard: zeff_sega8_core::hardware::timing::Sega8VideoStandard,
    console_region: zeff_sega8_core::hardware::region::Sega8Region,
    header_region: Option<zeff_sega8_core::hardware::region::Sega8Region>,
    serial_peer_present: bool,
}

fn validate_persistence_facts(
    kind: SaveRamKind,
    load: GameGearTasPersistentLoadOutcome,
    allow_project_storage: bool,
) -> Result<()> {
    let matches = if allow_project_storage {
        match kind {
            SaveRamKind::None => load == GameGearTasPersistentLoadOutcome::Absent,
            SaveRamKind::KnownBatteryBacked { size } => {
                size == 8 * 1024 && load != GameGearTasPersistentLoadOutcome::Unknown
            }
            _ => false,
        }
    } else {
        load == GameGearTasPersistentLoadOutcome::Absent && kind == SaveRamKind::None
    };
    ensure!(
        matches,
        "direct Game Gear TAS requires exact owned or absent cartridge persistence"
    );
    Ok(())
}

fn validate_machine_facts(facts: GameGearMachineFacts) -> Result<()> {
    ensure!(
        facts.rom_matches
            && facts.mapper_kind == zeff_sega8_core::hardware::cartridge::Sega8MapperKind::Sega
            && facts.video_standard == zeff_sega8_core::hardware::timing::Sega8VideoStandard::Ntsc
            && facts.header_region == Some(facts.console_region)
            && facts.console_region == zeff_sega8_core::hardware::region::Sega8Region::Export
            && !facts.serial_peer_present,
        "Game Gear core media, mapper, hardware, or serial configuration is incompatible"
    );
    Ok(())
}

pub(crate) fn validate_direct_game_gear_tas_runtime(
    backend: &EmuBackend,
    cheats_present: bool,
) -> Result<()> {
    validate_direct_game_gear_tas_execution_runtime(backend, cheats_present)?;
    validate_neutral_input(backend)
}

pub(crate) fn validate_direct_game_gear_tas_private_runtime(
    backend: &EmuBackend,
    cheats_present: bool,
) -> Result<()> {
    validate_direct_game_gear_tas_private_execution_runtime(backend, cheats_present)?;
    validate_neutral_input(backend)
}

pub(crate) fn validate_direct_game_gear_tas_private_execution_runtime(
    backend: &EmuBackend,
    cheats_present: bool,
) -> Result<()> {
    validate_direct_game_gear_tas_execution_runtime_inner(backend, cheats_present, true)
}

fn validate_neutral_input(backend: &EmuBackend) -> Result<()> {
    let sega8 = backend
        .sega8()
        .context("Game Gear backend became unavailable")?;
    use zeff_sega8_core::hardware::input::ControllerPort;
    ensure!(
        sega8.emu.bus().input().read_controller(ControllerPort::One) == 0xFF
            && sega8.emu.bus().input().read_controller(ControllerPort::Two) == 0xFF
            && !sega8.emu.bus().input().game_gear_start_pressed(),
        "direct Game Gear TAS acquisition requires neutral input"
    );
    Ok(())
}

pub(crate) fn validate_direct_game_gear_tas_execution_runtime(
    backend: &EmuBackend,
    cheats_present: bool,
) -> Result<()> {
    validate_direct_game_gear_tas_execution_runtime_inner(backend, cheats_present, false)
}

fn validate_direct_game_gear_tas_execution_runtime_inner(
    backend: &EmuBackend,
    cheats_present: bool,
    allow_project_storage: bool,
) -> Result<()> {
    ensure!(
        backend.system() == ActiveSystem::GameGear,
        "TAS runtime requires a Game Gear backend"
    );
    let metadata = backend.replay_metadata();
    let expected_core_family = format!("{:?}", zeff_emu_common::system::CoreFamily::Sega8);
    ensure!(
        metadata.system.as_deref() == Some(ActiveSystem::GameGear.code())
            && metadata.core_family.as_deref() == Some(expected_core_family.as_str()),
        "Game Gear backend identity metadata is incompatible"
    );
    let effective_media_sha256 = metadata
        .rom_sha256
        .context("Game Gear backend omitted its effective media identity")?;
    let sega8 = backend
        .sega8()
        .context("Game Gear backend became unavailable")?;
    let provenance = sega8
        .game_gear_tas_load_provenance()
        .context("Game Gear backend omitted load provenance")?;
    ensure!(
        (provenance.direct_gg_file
            || provenance.tas_source_media_sha256 != provenance.raw_source_media_sha256)
            && super::mapper_kind_from_paths(backend.source_path(), backend.rom_path()).is_none()
            && provenance.raw_source_media_sha256 == effective_media_sha256
            && (1..=MAX_DIRECT_GAME_GEAR_ROM_BYTES).contains(&provenance.raw_source_media_len),
        "Game Gear TAS requires one bounded .gg cartridge with core-detected mapper"
    );
    ensure!(
        !provenance.any_mod_enabled && !provenance.any_mod_applied,
        "direct Game Gear TAS requires mods to be disabled"
    );
    validate_persistence_facts(
        backend.save_ram_kind(),
        provenance.persistent_load,
        allow_project_storage,
    )?;
    ensure!(
        provenance
            .standard_mapper_ram_identity
            .is_some_and(|identity| {
                if allow_project_storage {
                    matches!(
                        identity.ram(),
                        zeff_sega8_core::hardware::cartridge::GameGearStandardMapperRam::Absent
                            | zeff_sega8_core::hardware::cartridge::GameGearStandardMapperRam::BatteryBacked8KiB
                    )
                } else {
                    identity.ram()
                        == zeff_sega8_core::hardware::cartridge::GameGearStandardMapperRam::Absent
                }
            }),
        "direct Game Gear TAS requires an exact supported board-catalog result"
    );
    ensure!(
        provenance.controller_model == GameGearTasControllerModel::BuiltInPadAndStart
            && provenance.initial_input.is_none(),
        "direct Game Gear TAS requires a neutral built-in pad and Start input"
    );
    let default_rate = zeff_sega8_core::emulator::DEFAULT_SAMPLE_RATE;
    ensure!(
        provenance.configured_sample_rate.is_none()
            && provenance.initial_sample_rate == default_rate
            && sega8.emu.sample_rate() == default_rate,
        "direct Game Gear TAS requires the default sample rate"
    );
    ensure!(
        !cheats_present && metadata.cheat_sha256.is_none(),
        "Game Gear TAS runtime enabled cheats"
    );
    ensure!(
        metadata.firmware.len() == 1
            && metadata.firmware.iter().all(|firmware| matches!(
                firmware,
                ReplayFirmwareManifest::Skipped { firmware_id, .. }
                    if firmware_id == "sega.gg.boot"
            ))
            && !sega8.emu.bus().has_boot_rom()
            && !sega8.emu.bus().boot_rom_enabled(),
        "direct Game Gear TAS requires the boot ROM to be absent"
    );
    let header_region = sega8.emu.bus().cartridge.header().and_then(|header| {
        zeff_sega8_core::hardware::region::Sega8Region::from_header_region(header.region)
    });
    validate_machine_facts(GameGearMachineFacts {
        rom_matches: sega8.emu.rom_hash() == effective_media_sha256,
        mapper_kind: sega8.emu.bus().mapper().kind(),
        video_standard: sega8.emu.video_standard(),
        console_region: sega8.emu.console_region(),
        header_region,
        serial_peer_present: sega8
            .emu
            .bus()
            .game_gear_serial()
            .debug_snapshot()
            .peer_present,
    })?;
    ensure!(
        backend.media_slot_snapshot().is_none(),
        "direct Game Gear TAS does not support removable media"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_validator_remains_available_for_a_catalogued_profile() {
        let _: fn(&EmuBackend, bool) -> Result<()> = validate_direct_game_gear_tas_runtime;
    }

    fn machine_facts() -> GameGearMachineFacts {
        GameGearMachineFacts {
            rom_matches: true,
            mapper_kind: zeff_sega8_core::hardware::cartridge::Sega8MapperKind::Sega,
            video_standard: zeff_sega8_core::hardware::timing::Sega8VideoStandard::Ntsc,
            console_region: zeff_sega8_core::hardware::region::Sega8Region::Export,
            header_region: Some(zeff_sega8_core::hardware::region::Sega8Region::Export),
            serial_peer_present: false,
        }
    }

    #[test]
    fn persistence_requires_catalogued_absence() {
        assert!(
            validate_persistence_facts(
                SaveRamKind::None,
                GameGearTasPersistentLoadOutcome::Absent,
                false,
            )
            .is_ok()
        );
        assert!(
            validate_persistence_facts(
                SaveRamKind::KnownBatteryBacked { size: 8 * 1024 },
                GameGearTasPersistentLoadOutcome::Absent,
                false,
            )
            .is_err()
        );
        assert!(
            validate_persistence_facts(
                SaveRamKind::KnownVolatile { size: 8 * 1024 },
                GameGearTasPersistentLoadOutcome::Absent,
                true,
            )
            .is_err()
        );
        assert!(
            validate_persistence_facts(
                SaveRamKind::MapperRamUnknown { size: 8 * 1024 },
                GameGearTasPersistentLoadOutcome::Absent,
                true,
            )
            .is_err()
        );
        assert!(
            validate_persistence_facts(
                SaveRamKind::None,
                GameGearTasPersistentLoadOutcome::Loaded,
                true,
            )
            .is_err()
        );
        assert!(
            validate_persistence_facts(
                SaveRamKind::KnownBatteryBacked { size: 8 * 1024 },
                GameGearTasPersistentLoadOutcome::Skipped,
                true,
            )
            .is_ok()
        );
    }

    #[test]
    fn machine_facts_reject_linked_peer_and_non_neutral_input() {
        let facts = machine_facts();
        assert!(validate_machine_facts(facts).is_ok());
        assert!(
            validate_machine_facts(GameGearMachineFacts {
                serial_peer_present: true,
                ..facts
            })
            .is_err()
        );
        assert!(
            validate_machine_facts(GameGearMachineFacts {
                console_region: zeff_sega8_core::hardware::region::Sega8Region::Japanese,
                ..facts
            })
            .is_err()
        );
    }
}
