use super::gb;
use crate::emu_backend::nes::NesPersistentLoadOutcome;
use crate::emu_backend::{ActiveSystem, EmuBackend};
use crate::emu_thread::{TasExecutionProfile, TasLoadedProfileObservation};
use crate::tas_project::TasDigest;

pub(in crate::emu_thread) fn observe_loaded_profile(
    backend: &EmuBackend,
    cheats_present: bool,
    profile: TasExecutionProfile,
) -> TasLoadedProfileObservation {
    match profile {
        TasExecutionProfile::DirectNesCartridge => observe_direct_nes(backend, cheats_present),
        TasExecutionProfile::DirectFdsDisk => observe_direct_fds(backend, cheats_present),
        TasExecutionProfile::DirectGbCartridgeDmg => gb::observe_direct_gb(backend, cheats_present),
        TasExecutionProfile::DirectGbCartridgeCgb => {
            gb::observe_direct_gbc(backend, cheats_present)
        }
        TasExecutionProfile::DirectColecoCartridge => {
            observe_direct_coleco(backend, cheats_present)
        }
        TasExecutionProfile::DirectSmsCartridge => observe_direct_sms(backend, cheats_present),
        TasExecutionProfile::DirectGameGearCartridge => {
            observe_direct_game_gear(backend, cheats_present)
        }
        TasExecutionProfile::DirectGbaCartridge => observe_direct_gba(backend, cheats_present),
        TasExecutionProfile::DirectSg1000Cartridge => {
            observe_direct_sg1000(backend, cheats_present)
        }
        TasExecutionProfile::DirectWsCartridge => observe_direct_ws(backend, cheats_present),
        TasExecutionProfile::DirectPceHuCard
        | TasExecutionProfile::DirectPceSixButtonHuCard
        | TasExecutionProfile::DirectPceMultitapHuCard => {
            observe_direct_pce(backend, cheats_present, profile)
        }
        TasExecutionProfile::DirectPceCd | TasExecutionProfile::DirectPceMultitapCd => {
            observe_direct_pce(backend, cheats_present, profile)
        }
    }
}

fn observe_direct_fds(backend: &EmuBackend, cheats_present: bool) -> TasLoadedProfileObservation {
    let metadata = backend.replay_metadata();
    let provenance = backend.nes_tas_load_provenance();
    let load = provenance.map(|view| view.load);
    let expected_firmware = matches!(
        metadata.firmware.as_slice(),
        [zeff_emu_common::replay::ReplayFirmwareManifest::External { firmware_id, .. }]
            if firmware_id == "nintendo.fds.bios"
    );
    let topology_matches = backend.nes().is_some_and(|nes| {
        let snapshot = nes.media_slot_snapshot();
        nes.has_standard_console_hardware()
            && backend.nes_has_standard_controller_topology() == Some(true)
            && snapshot.is_some_and(|snapshot| {
                snapshot.inserted()
                    && snapshot
                        .state
                        .side
                        .is_some_and(|side| side < snapshot.side_count)
                    && !snapshot.state.write_protected
                    && (1..=2).contains(&snapshot.side_count)
            })
    });
    let privately_owned = backend
        .nes()
        .is_some_and(|nes| !nes.host_persistence_enabled());
    TasLoadedProfileObservation {
        profile: TasExecutionProfile::DirectFdsDisk,
        system: backend.system(),
        identity_metadata_matches: metadata.system.as_deref() == Some(ActiveSystem::Nes.code())
            && metadata.core_family.as_deref()
                == Some(format!("{:?}", zeff_emu_common::system::CoreFamily::Nes).as_str()),
        load_provenance_available: provenance.is_some(),
        direct_source: Some(privately_owned),
        source_media_sha256: load.map(|load| TasDigest(load.raw_source_media_sha256)),
        effective_media_sha256: metadata.rom_sha256.map(TasDigest),
        mods_absent: load.map(|load| !load.any_mod_enabled && !load.any_mod_applied),
        persistent_state_absent: load
            .map(|load| load.persistent_load == NesPersistentLoadOutcome::Absent),
        project_owned_persistence: None,
        initial_input_neutral: load
            .map(|load| load.initial_input.buttons == 0 && load.initial_input.dpad == 0),
        configured_at_load_sample_rate: load.and_then(|load| load.configured_sample_rate),
        initial_sample_rate: load.map(|load| load.initial_sample_rate),
        current_sample_rate: provenance.map(|view| view.current_sample_rate),
        firmware_profile_matches: expected_firmware,
        hardware_profile_matches: topology_matches,
        controller_profile_matches: topology_matches,
        removable_media_absent: topology_matches,
        cheats_absent: !cheats_present && metadata.cheat_sha256.is_none(),
    }
}

fn observe_direct_nes(backend: &EmuBackend, cheats_present: bool) -> TasLoadedProfileObservation {
    let metadata = backend.replay_metadata();
    let provenance = backend.nes_tas_load_provenance();
    let load = provenance.map(|view| view.load);
    let expected_core_family = format!("{:?}", zeff_emu_common::system::CoreFamily::Nes);
    TasLoadedProfileObservation {
        profile: TasExecutionProfile::DirectNesCartridge,
        system: backend.system(),
        identity_metadata_matches: metadata.system.as_deref() == Some(ActiveSystem::Nes.code())
            && metadata.core_family.as_deref() == Some(expected_core_family.as_str()),
        load_provenance_available: provenance.is_some(),
        direct_source: load
            .map(|load| load.direct_nes_file || load.raw_source_media_sha256 != backend.rom_hash()),
        source_media_sha256: load.map(|load| TasDigest(load.raw_source_media_sha256)),
        effective_media_sha256: Some(TasDigest(backend.rom_hash())),
        mods_absent: load.map(|load| !load.any_mod_enabled && !load.any_mod_applied),
        persistent_state_absent: load
            .map(|load| load.persistent_load == NesPersistentLoadOutcome::Absent)
            .map(|absent| absent && !backend.save_ram_kind().is_battery_backed()),
        project_owned_persistence: None,
        initial_input_neutral: load
            .map(|load| load.initial_input.buttons == 0 && load.initial_input.dpad == 0),
        configured_at_load_sample_rate: load.and_then(|load| load.configured_sample_rate),
        initial_sample_rate: load.map(|load| load.initial_sample_rate),
        current_sample_rate: provenance.map(|view| view.current_sample_rate),
        firmware_profile_matches: metadata.firmware.is_empty(),
        hardware_profile_matches: backend
            .nes()
            .is_some_and(|nes| nes.has_standard_console_hardware()),
        controller_profile_matches: backend.nes_has_standard_or_zapper_controller_topology()
            == Some(true),
        removable_media_absent: backend.media_slot_snapshot().is_none(),
        cheats_absent: !cheats_present,
    }
}

fn observe_direct_coleco(
    backend: &EmuBackend,
    cheats_present: bool,
) -> TasLoadedProfileObservation {
    let metadata = backend.replay_metadata();
    let provenance = backend
        .coleco()
        .and_then(crate::emu_backend::ColecoBackend::tas_load_provenance);
    let load = provenance.map(|view| view.load);
    let expected_core_family = format!("{:?}", zeff_emu_common::system::CoreFamily::ColecoVision);
    TasLoadedProfileObservation {
        profile: TasExecutionProfile::DirectColecoCartridge,
        system: backend.system(),
        identity_metadata_matches: metadata.system.as_deref() == Some(ActiveSystem::Coleco.code())
            && metadata.core_family.as_deref() == Some(expected_core_family.as_str()),
        load_provenance_available: provenance.is_some(),
        direct_source: load.map(|load| {
            load.direct_col_file || load.tas_source_media_sha256 != load.raw_source_media_sha256
        }),
        source_media_sha256: load.map(|load| TasDigest(load.tas_source_media_sha256)),
        effective_media_sha256: metadata.rom_sha256.map(TasDigest),
        mods_absent: load.map(|load| !load.any_mod_enabled && !load.any_mod_applied),
        persistent_state_absent: Some(
            backend.save_ram_kind() == zeff_emu_common::save_ram::SaveRamKind::None,
        ),
        project_owned_persistence: None,
        initial_input_neutral: load.map(|load| load.initial_input.is_none()),
        configured_at_load_sample_rate: load.and_then(|load| load.configured_sample_rate),
        initial_sample_rate: load.map(|load| load.initial_sample_rate),
        current_sample_rate: provenance.map(|view| view.current_sample_rate),
        firmware_profile_matches: backend.coleco().is_some_and(|coleco| {
            matches!(
                metadata.firmware.as_slice(),
                [zeff_emu_common::replay::ReplayFirmwareManifest::External {
                    firmware_id,
                    sha256,
                    ..
                }] if firmware_id == "coleco.vision.bios" && *sha256 == coleco.emu.bios_hash()
            )
        }),
        hardware_profile_matches: backend.system() == ActiveSystem::Coleco,
        controller_profile_matches: provenance.is_some_and(|view| {
            view.current_controllers == [zeff_coleco_core::StandardController::default(); 2]
        }),
        removable_media_absent: backend.media_slot_snapshot().is_none(),
        cheats_absent: !cheats_present && metadata.cheat_sha256.is_none(),
    }
}
fn observe_direct_sms(backend: &EmuBackend, cheats_present: bool) -> TasLoadedProfileObservation {
    let metadata = backend.replay_metadata();
    let provenance = backend
        .sega8()
        .and_then(crate::emu_backend::Sega8Backend::sms_tas_load_provenance);
    let load = provenance.map(|view| view.load);
    let expected_core_family = format!("{:?}", zeff_emu_common::system::CoreFamily::Sega8);
    let allowed_ram = matches!(
        backend.save_ram_kind(),
        zeff_emu_common::save_ram::SaveRamKind::None
            | zeff_emu_common::save_ram::SaveRamKind::KnownVolatile { .. }
    );
    TasLoadedProfileObservation {
        profile: TasExecutionProfile::DirectSmsCartridge,
        system: backend.system(),
        identity_metadata_matches: metadata.system.as_deref()
            == Some(ActiveSystem::MasterSystem.code())
            && metadata.core_family.as_deref() == Some(expected_core_family.as_str()),
        load_provenance_available: provenance.is_some(),
        direct_source: load.map(|load| {
            load.direct_sms_file || load.tas_source_media_sha256 != load.raw_source_media_sha256
        }),
        source_media_sha256: load.map(|load| TasDigest(load.tas_source_media_sha256)),
        effective_media_sha256: metadata.rom_sha256.map(TasDigest),
        mods_absent: load.map(|load| !load.any_mod_enabled && !load.any_mod_applied),
        persistent_state_absent: Some(allowed_ram),
        project_owned_persistence: None,
        initial_input_neutral: load.map(|load| load.initial_input.is_none()),
        configured_at_load_sample_rate: load.and_then(|load| load.configured_sample_rate),
        initial_sample_rate: load.map(|load| load.initial_sample_rate),
        current_sample_rate: provenance.map(|view| view.current_sample_rate),
        firmware_profile_matches: metadata.firmware.len() == 1
            && metadata.firmware.iter().all(|firmware| {
                matches!(
                    firmware,
                    zeff_emu_common::replay::ReplayFirmwareManifest::Skipped { firmware_id, .. }
                        if firmware_id == "sega.sms.boot"
                )
            }),
        hardware_profile_matches:
            crate::emu_backend::loader::validate_direct_sms_tas_execution_runtime(
                backend,
                cheats_present,
            )
            .is_ok(),
        controller_profile_matches: provenance
            .is_some_and(|view| view.current_controller_raw == [0xFF; 2]),
        removable_media_absent: backend.media_slot_snapshot().is_none(),
        cheats_absent: !cheats_present && metadata.cheat_sha256.is_none(),
    }
}

fn observe_direct_game_gear(
    backend: &EmuBackend,
    cheats_present: bool,
) -> TasLoadedProfileObservation {
    let metadata = backend.replay_metadata();
    let provenance = backend
        .sega8()
        .and_then(crate::emu_backend::Sega8Backend::game_gear_tas_load_provenance);
    let expected_core_family = format!("{:?}", zeff_emu_common::system::CoreFamily::Sega8);
    let controller_neutral = backend.sega8().is_some_and(|sega8| {
        use zeff_sega8_core::hardware::input::ControllerPort;
        sega8.emu.bus().input().read_controller(ControllerPort::One) == 0xFF
            && sega8.emu.bus().input().read_controller(ControllerPort::Two) == 0xFF
            && !sega8.emu.bus().input().game_gear_start_pressed()
    });
    TasLoadedProfileObservation {
        profile: TasExecutionProfile::DirectGameGearCartridge,
        system: backend.system(),
        identity_metadata_matches: metadata.system.as_deref()
            == Some(ActiveSystem::GameGear.code())
            && metadata.core_family.as_deref() == Some(expected_core_family.as_str()),
        load_provenance_available: provenance.is_some(),
        direct_source: provenance.map(|load| {
            load.direct_gg_file || load.tas_source_media_sha256 != load.raw_source_media_sha256
        }),
        source_media_sha256: provenance.map(|load| TasDigest(load.tas_source_media_sha256)),
        effective_media_sha256: metadata.rom_sha256.map(TasDigest),
        mods_absent: provenance.map(|load| !load.any_mod_enabled && !load.any_mod_applied),
        persistent_state_absent: provenance.map(|load| {
            load.persistent_load
                == crate::emu_backend::sega8::GameGearTasPersistentLoadOutcome::Absent
                && backend.save_ram_kind() == zeff_emu_common::save_ram::SaveRamKind::None
        }),
        project_owned_persistence: None,
        initial_input_neutral: provenance.map(|load| load.initial_input.is_none()),
        configured_at_load_sample_rate: provenance.and_then(|load| load.configured_sample_rate),
        initial_sample_rate: provenance.map(|load| load.initial_sample_rate),
        current_sample_rate: backend.sega8().map(|sega8| sega8.emu.sample_rate()),
        firmware_profile_matches: metadata.firmware.len() == 1
            && metadata.firmware.iter().all(|firmware| {
                matches!(
                    firmware,
                    zeff_emu_common::replay::ReplayFirmwareManifest::Skipped { firmware_id, .. }
                        if firmware_id == "sega.gg.boot"
                )
            }),
        hardware_profile_matches:
            crate::emu_backend::loader::validate_direct_game_gear_tas_private_runtime(
                backend,
                cheats_present,
            )
            .is_ok(),
        controller_profile_matches: controller_neutral,
        removable_media_absent: backend.media_slot_snapshot().is_none(),
        cheats_absent: !cheats_present && metadata.cheat_sha256.is_none(),
    }
}

fn observe_direct_sg1000(
    backend: &EmuBackend,
    cheats_present: bool,
) -> TasLoadedProfileObservation {
    let metadata = backend.replay_metadata();
    let provenance = backend
        .sega8()
        .and_then(crate::emu_backend::Sega8Backend::sg1000_tas_load_provenance);
    let load = provenance.map(|view| view.load);
    let expected_core_family = format!("{:?}", zeff_emu_common::system::CoreFamily::Sega8);
    TasLoadedProfileObservation {
        profile: TasExecutionProfile::DirectSg1000Cartridge,
        system: backend.system(),
        identity_metadata_matches: metadata.system.as_deref() == Some(ActiveSystem::Sg1000.code())
            && metadata.core_family.as_deref() == Some(expected_core_family.as_str()),
        load_provenance_available: provenance.is_some(),
        direct_source: load.map(|load| {
            load.direct_sg_file || load.tas_source_media_sha256 != load.raw_source_media_sha256
        }),
        source_media_sha256: load.map(|load| TasDigest(load.tas_source_media_sha256)),
        effective_media_sha256: metadata.rom_sha256.map(TasDigest),
        mods_absent: load.map(|load| !load.any_mod_enabled && !load.any_mod_applied),
        persistent_state_absent: Some(
            backend.save_ram_kind() == zeff_emu_common::save_ram::SaveRamKind::None,
        ),
        project_owned_persistence: None,
        initial_input_neutral: load.map(|load| load.initial_input.is_none()),
        configured_at_load_sample_rate: load.and_then(|load| load.configured_sample_rate),
        initial_sample_rate: load.map(|load| load.initial_sample_rate),
        current_sample_rate: provenance.map(|view| view.current_sample_rate),
        firmware_profile_matches: metadata.firmware.is_empty(),
        hardware_profile_matches:
            crate::emu_backend::loader::validate_direct_sg1000_tas_execution_runtime(
                backend,
                cheats_present,
            )
            .is_ok(),
        controller_profile_matches: provenance
            .is_some_and(|view| view.current_controller_raw == [0xFF; 2]),
        removable_media_absent: backend.media_slot_snapshot().is_none(),
        cheats_absent: !cheats_present && metadata.cheat_sha256.is_none(),
    }
}

fn observe_direct_ws(backend: &EmuBackend, cheats_present: bool) -> TasLoadedProfileObservation {
    let metadata = backend.replay_metadata();
    let provenance = backend
        .ws()
        .and_then(crate::emu_backend::WsBackend::tas_load_provenance);
    let load = provenance.map(|view| view.load);
    let expected_core_family = format!("{:?}", zeff_emu_common::system::CoreFamily::WonderSwan);
    TasLoadedProfileObservation {
        profile: TasExecutionProfile::DirectWsCartridge,
        system: backend.system(),
        identity_metadata_matches: metadata.system.as_deref()
            == Some(ActiveSystem::WonderSwan.code())
            && metadata.core_family.as_deref() == Some(expected_core_family.as_str()),
        load_provenance_available: provenance.is_some(),
        direct_source: load.map(|load| {
            load.direct_ws_file || load.tas_source_media_sha256 != load.raw_source_media_sha256
        }),
        source_media_sha256: load.map(|load| TasDigest(load.tas_source_media_sha256)),
        effective_media_sha256: metadata.rom_sha256.map(TasDigest),
        mods_absent: load.map(|load| !load.any_mod_enabled && !load.any_mod_applied),
        persistent_state_absent: load.map(|load| {
            load.persistent_load == crate::emu_backend::ws::WsTasPersistentLoadOutcome::Absent
                && backend.save_ram_kind() == zeff_emu_common::save_ram::SaveRamKind::None
        }),
        project_owned_persistence: None,
        initial_input_neutral: load.map(|load| load.initial_input.is_none()),
        configured_at_load_sample_rate: load.and_then(|load| load.configured_sample_rate),
        initial_sample_rate: load.map(|load| load.initial_sample_rate),
        current_sample_rate: provenance.map(|view| view.current_sample_rate),
        firmware_profile_matches: metadata.firmware.is_empty(),
        hardware_profile_matches:
            crate::emu_backend::loader::validate_direct_ws_tas_linked_runtime(
                backend,
                cheats_present,
            )
            .is_ok(),
        controller_profile_matches: load.is_some_and(|load| load.initial_input.is_none()),
        removable_media_absent: backend.media_slot_snapshot().is_none(),
        cheats_absent: !cheats_present && metadata.cheat_sha256.is_none(),
    }
}

fn observe_direct_pce(
    backend: &EmuBackend,
    cheats_present: bool,
    profile: TasExecutionProfile,
) -> TasLoadedProfileObservation {
    let metadata = backend.replay_metadata();
    let provenance = backend
        .pce()
        .and_then(crate::emu_backend::PceBackend::tas_load_provenance);
    let load = provenance.map(|view| view.load);
    let expected_core_family = format!("{:?}", zeff_emu_common::system::CoreFamily::PcEngine);
    let runtime_matches = match profile {
        TasExecutionProfile::DirectPceHuCard => {
            crate::emu_backend::loader::validate_direct_pce_tas_execution_runtime(
                backend,
                cheats_present,
            )
            .is_ok()
        }
        TasExecutionProfile::DirectPceSixButtonHuCard => {
            crate::emu_backend::loader::validate_direct_pce_six_button_tas_execution_runtime(
                backend,
                cheats_present,
            )
            .is_ok()
        }
        TasExecutionProfile::DirectPceMultitapHuCard => {
            crate::emu_backend::loader::validate_direct_pce_multitap_tas_execution_runtime(
                backend,
                cheats_present,
            )
            .is_ok()
        }
        TasExecutionProfile::DirectPceCd => {
            crate::emu_backend::loader::validate_direct_pce_cd_tas_execution_runtime(
                backend,
                cheats_present,
            )
            .is_ok()
        }
        TasExecutionProfile::DirectPceMultitapCd => {
            crate::emu_backend::loader::validate_direct_pce_multitap_cd_tas_execution_runtime(
                backend,
                cheats_present,
            )
            .is_ok()
        }
        _ => unreachable!("invalid PC Engine TAS profile"),
    };
    TasLoadedProfileObservation {
        profile,
        system: backend.system(),
        identity_metadata_matches: metadata.system.as_deref() == Some(ActiveSystem::Pce.code())
            && metadata.core_family.as_deref() == Some(expected_core_family.as_str()),
        load_provenance_available: provenance.is_some(),
        direct_source: load.map(|load| {
            if matches!(
                profile,
                TasExecutionProfile::DirectPceCd | TasExecutionProfile::DirectPceMultitapCd
            ) {
                load.direct_pce_cd
            } else {
                load.direct_pce_file || load.tas_source_media_sha256 != load.raw_source_media_sha256
            }
        }),
        source_media_sha256: load.map(|load| TasDigest(load.tas_source_media_sha256)),
        effective_media_sha256: metadata.rom_sha256.map(TasDigest),
        mods_absent: load.map(|load| !load.any_mod_enabled && !load.any_mod_applied),
        persistent_state_absent: load.map(|load| {
            load.persistent_load == crate::emu_backend::pce::PceTasPersistentLoadOutcome::Skipped
                && (matches!(
                    profile,
                    TasExecutionProfile::DirectPceCd | TasExecutionProfile::DirectPceMultitapCd
                ) || crate::emu_backend::loader::direct_pce_tas_host_persistence_absent(
                    backend,
                ))
        }),
        project_owned_persistence: None,
        initial_input_neutral: load.map(|load| load.initial_input.is_none()),
        configured_at_load_sample_rate: load.and_then(|load| load.configured_sample_rate),
        initial_sample_rate: load.map(|load| load.initial_sample_rate),
        current_sample_rate: provenance.map(|view| view.current_sample_rate),
        firmware_profile_matches: if matches!(
            profile,
            TasExecutionProfile::DirectPceCd | TasExecutionProfile::DirectPceMultitapCd
        ) {
            runtime_matches
        } else {
            metadata.firmware.is_empty()
        },
        hardware_profile_matches: runtime_matches,
        controller_profile_matches: runtime_matches,
        removable_media_absent: if matches!(
            profile,
            TasExecutionProfile::DirectPceCd | TasExecutionProfile::DirectPceMultitapCd
        ) {
            false
        } else {
            backend.media_slot_snapshot().is_none()
        },
        cheats_absent: !cheats_present && metadata.cheat_sha256.is_none(),
    }
}

fn observe_direct_gba(backend: &EmuBackend, cheats_present: bool) -> TasLoadedProfileObservation {
    let metadata = backend.replay_metadata();
    let provenance = backend.gba_tas_load_provenance();
    let load = provenance.map(|view| view.load);
    let expected_core_family = format!("{:?}", zeff_emu_common::system::CoreFamily::GameBoyAdvance);
    TasLoadedProfileObservation {
        profile: TasExecutionProfile::DirectGbaCartridge,
        system: backend.system(),
        identity_metadata_matches: metadata.system.as_deref()
            == Some(ActiveSystem::GameBoyAdvance.code())
            && metadata.core_family.as_deref() == Some(expected_core_family.as_str()),
        load_provenance_available: provenance.is_some(),
        direct_source: load.map(|load| load.direct_gba_file),
        source_media_sha256: load.map(|load| TasDigest(load.tas_source_media_sha256)),
        effective_media_sha256: metadata.rom_sha256.map(TasDigest),
        mods_absent: load.map(|load| !load.any_mod_enabled && !load.any_mod_applied),
        persistent_state_absent: load.map(|load| {
            load.persistent_load == crate::emu_backend::gba::GbaTasPersistentLoadOutcome::Absent
        }),
        project_owned_persistence: None,
        initial_input_neutral: load
            .map(|load| load.initial_input.buttons == 0 && load.initial_input.dpad == 0),
        configured_at_load_sample_rate: load.and_then(|load| load.configured_sample_rate),
        initial_sample_rate: load.map(|load| load.initial_sample_rate),
        current_sample_rate: provenance.map(|view| view.current_sample_rate),
        firmware_profile_matches: matches!(
            metadata.firmware.as_slice(),
            [zeff_emu_common::replay::ReplayFirmwareManifest::Hle {
                firmware_id,
                implementation,
                compatibility_version: 1,
            }] if firmware_id == "nintendo.gba.bios" && implementation == "zeff-gba-hle"
        ),
        hardware_profile_matches:
            crate::emu_backend::gba::validate_direct_gba_tas_private_execution_runtime(
                backend,
                cheats_present,
            )
            .is_ok(),
        controller_profile_matches: load
            .is_some_and(|load| load.initial_input.buttons == 0 && load.initial_input.dpad == 0),
        removable_media_absent: backend.media_slot_snapshot().is_none(),
        cheats_absent: !cheats_present && metadata.cheat_sha256.is_none(),
    }
}
