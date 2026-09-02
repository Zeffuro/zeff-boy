use crate::emu_backend::nes::{NesPersistentLoadOutcome, NesTasLoadProvenance};
use crate::emu_backend::{ActiveSystem, EmuBackend};
use crate::emu_thread::{
    TasControlAcquireRejectedReason as Rejected, TasControlLeaseWitness, TasExecutionProfile,
    TasPersistenceContract,
};
use crate::tas_project::TasDigest;

mod gb;
mod observation;

pub(in crate::emu_thread) use observation::observe_loaded_profile;

pub(super) struct DirectNesProfileFacts {
    pub(super) system: ActiveSystem,
    pub(super) identity_metadata_matches: bool,
    pub(super) provenance: Option<NesTasLoadProvenance>,
    pub(super) current_sample_rate: Option<u32>,
    pub(super) effective_media_sha256: [u8; 32],
    pub(super) battery_backed: bool,
    pub(super) battery_state_available: bool,
    pub(super) firmware_present: bool,
    pub(super) standard_console_hardware: bool,
    pub(super) supported_controller_topology: bool,
    pub(super) removable_media_present: bool,
    pub(super) cheats_present: bool,
}

pub(in crate::emu_thread) fn build_tas_witness_for_persistence(
    backend: &EmuBackend,
    cheats_present: bool,
    profile: TasExecutionProfile,
    persistence: TasPersistenceContract,
) -> Result<TasControlLeaseWitness, Rejected> {
    match persistence {
        TasPersistenceContract::Absent => build_tas_witness(backend, cheats_present, profile),
        TasPersistenceContract::NesBattery { .. }
            if profile == TasExecutionProfile::DirectNesCartridge =>
        {
            build_direct_nes_witness_with_persistence(backend, cheats_present, persistence)
        }
        TasPersistenceContract::GbBattery { .. } | TasPersistenceContract::GbRtcBattery { .. } => {
            gb::build_direct_gb_witness_with_persistence(
                backend,
                cheats_present,
                profile,
                persistence,
            )
        }
        TasPersistenceContract::GbaBattery { .. }
        | TasPersistenceContract::GbaRtcBattery { .. }
            if profile == TasExecutionProfile::DirectGbaCartridge =>
        {
            build_direct_gba_witness_with_persistence(backend, cheats_present, persistence)
        }
        TasPersistenceContract::GameGearBattery8KiB { .. }
            if profile == TasExecutionProfile::DirectGameGearCartridge =>
        {
            build_direct_game_gear_witness_with_persistence(backend, cheats_present, persistence)
        }
        TasPersistenceContract::WsBattery { .. } | TasPersistenceContract::WsRtcBattery { .. }
            if profile == TasExecutionProfile::DirectWsCartridge =>
        {
            build_direct_ws_witness_with_persistence(backend, cheats_present, persistence)
        }
        TasPersistenceContract::NesBattery { .. } => Err(Rejected::PersistentStateNotAbsent),
        TasPersistenceContract::GbaBattery { .. }
        | TasPersistenceContract::GbaRtcBattery { .. } => Err(Rejected::PersistentStateNotAbsent),
        TasPersistenceContract::GameGearBattery8KiB { .. } => {
            Err(Rejected::PersistentStateNotAbsent)
        }
        TasPersistenceContract::WsBattery { .. } | TasPersistenceContract::WsRtcBattery { .. } => {
            Err(Rejected::PersistentStateNotAbsent)
        }
    }
}

pub(in crate::emu_thread) fn build_tas_witness(
    backend: &EmuBackend,
    cheats_present: bool,
    profile: TasExecutionProfile,
) -> Result<TasControlLeaseWitness, Rejected> {
    match profile {
        TasExecutionProfile::DirectNesCartridge => {
            build_direct_nes_witness(backend, cheats_present)
        }
        TasExecutionProfile::DirectFdsDisk => build_direct_fds_witness(backend, cheats_present),
        TasExecutionProfile::DirectGbCartridgeDmg => {
            gb::build_direct_gb_witness(backend, cheats_present, profile)
        }
        TasExecutionProfile::DirectGbCartridgeCgb => {
            gb::build_direct_gb_witness(backend, cheats_present, profile)
        }
        TasExecutionProfile::DirectColecoCartridge => {
            build_direct_coleco_witness(backend, cheats_present)
        }
        TasExecutionProfile::DirectSmsCartridge => {
            build_direct_sms_witness(backend, cheats_present)
        }
        TasExecutionProfile::DirectGameGearCartridge => {
            build_direct_game_gear_witness(backend, cheats_present)
        }
        TasExecutionProfile::DirectGbaCartridge => {
            build_direct_gba_witness(backend, cheats_present)
        }
        TasExecutionProfile::DirectSg1000Cartridge => {
            build_direct_sg1000_witness(backend, cheats_present)
        }
        TasExecutionProfile::DirectWsCartridge => build_direct_ws_witness(backend, cheats_present),
        TasExecutionProfile::DirectPceHuCard | TasExecutionProfile::DirectPceSixButtonHuCard => {
            build_direct_pce_witness(backend, cheats_present, profile)
        }
        TasExecutionProfile::DirectPceCd => build_direct_pce_cd_witness(backend, cheats_present),
    }
}

fn build_direct_fds_witness(
    backend: &EmuBackend,
    cheats_present: bool,
) -> Result<TasControlLeaseWitness, Rejected> {
    crate::emu_backend::loader::validate_fds_tas_private_runtime(backend, cheats_present)
        .map_err(|_| Rejected::NonStandardConsoleHardware)?;
    let provenance = backend
        .nes_tas_load_provenance()
        .ok_or(Rejected::LoadProvenanceUnavailable)?;
    if provenance.load.persistent_load != NesPersistentLoadOutcome::Absent {
        return Err(Rejected::PersistentStateNotAbsent);
    }
    let (frame_count, state_bytes) =
        capture_current_state(|| backend.frame_count(), || backend.encode_state_bytes())?;
    Ok(TasControlLeaseWitness {
        profile: TasExecutionProfile::DirectFdsDisk,
        frame_count,
        source_media_sha256: TasDigest(provenance.load.raw_source_media_sha256),
        effective_media_sha256: TasDigest(backend.rom_hash()),
        current_state_sha256: TasDigest::from_bytes(&state_bytes),
        current_state_bytes: state_bytes,
        determinism_abi: zeff_nes_core::save_state::TAS_DETERMINISM_ABI_ID,
        state_format_compatibility_id: zeff_nes_core::save_state::TAS_STATE_FORMAT_COMPATIBILITY_ID,
        sync_config_sha256: TasDigest(provenance.load.sync_config_sha256),
    })
}

fn build_direct_nes_witness(
    backend: &EmuBackend,
    cheats_present: bool,
) -> Result<TasControlLeaseWitness, Rejected> {
    build_direct_nes_witness_with_persistence(
        backend,
        cheats_present,
        TasPersistenceContract::Absent,
    )
}

fn build_direct_nes_witness_with_persistence(
    backend: &EmuBackend,
    cheats_present: bool,
    persistence: TasPersistenceContract,
) -> Result<TasControlLeaseWitness, Rejected> {
    let metadata = backend.replay_metadata();
    let provenance = backend.nes_tas_load_provenance();
    let expected_core_family = format!("{:?}", zeff_emu_common::system::CoreFamily::Nes);
    let facts = DirectNesProfileFacts {
        system: backend.system(),
        identity_metadata_matches: metadata.system.as_deref() == Some(ActiveSystem::Nes.code())
            && metadata.core_family.as_deref() == Some(expected_core_family.as_str()),
        provenance: provenance.map(|view| *view.load),
        current_sample_rate: provenance.map(|view| view.current_sample_rate),
        effective_media_sha256: backend.rom_hash(),
        battery_backed: backend.save_ram_kind().is_battery_backed(),
        battery_state_available: backend
            .nes()
            .is_some_and(|nes| nes.emu.dump_battery_sram().is_some()),
        firmware_present: !metadata.firmware.is_empty(),
        standard_console_hardware: backend
            .nes()
            .is_some_and(|nes| nes.has_standard_console_hardware()),
        supported_controller_topology: backend.nes_has_standard_or_zapper_controller_topology()
            == Some(true),
        removable_media_present: backend.media_slot_snapshot().is_some(),
        cheats_present,
    };
    match persistence {
        TasPersistenceContract::Absent => validate_direct_nes_profile(&facts)?,
        TasPersistenceContract::NesBattery {
            byte_len,
            initial_sha256,
            ..
        } => {
            validate_direct_nes_profile_mode(&facts, true)?;
            let bytes = backend
                .nes_tas_battery_bytes()
                .ok_or(Rejected::PersistentStateNotAbsent)?;
            if bytes.len() as u64 != byte_len || TasDigest::from_bytes(&bytes) != initial_sha256 {
                return Err(Rejected::PersistentStateNotAbsent);
            }
        }
        TasPersistenceContract::GbBattery { .. }
        | TasPersistenceContract::GbRtcBattery { .. }
        | TasPersistenceContract::GbaBattery { .. }
        | TasPersistenceContract::GbaRtcBattery { .. }
        | TasPersistenceContract::GameGearBattery8KiB { .. }
        | TasPersistenceContract::WsBattery { .. }
        | TasPersistenceContract::WsRtcBattery { .. } => {
            return Err(Rejected::PersistentStateNotAbsent);
        }
    }

    let (frame_count, state_bytes) =
        capture_current_state(|| backend.frame_count(), || backend.encode_state_bytes())?;
    let provenance = facts
        .provenance
        .expect("validated direct NES profile must have provenance");
    let current_state_sha256 = TasDigest::from_bytes(&state_bytes);
    Ok(TasControlLeaseWitness {
        profile: TasExecutionProfile::DirectNesCartridge,
        frame_count,
        source_media_sha256: TasDigest(provenance.raw_source_media_sha256),
        effective_media_sha256: TasDigest(facts.effective_media_sha256),
        current_state_bytes: state_bytes,
        current_state_sha256,
        determinism_abi: zeff_nes_core::save_state::TAS_DETERMINISM_ABI_ID,
        state_format_compatibility_id: zeff_nes_core::save_state::TAS_STATE_FORMAT_COMPATIBILITY_ID,
        sync_config_sha256: TasDigest(provenance.sync_config_sha256),
    })
}

fn build_direct_coleco_witness(
    backend: &EmuBackend,
    cheats_present: bool,
) -> Result<TasControlLeaseWitness, Rejected> {
    crate::emu_backend::loader::validate_direct_coleco_tas_runtime(backend, cheats_present)
        .map_err(|_| Rejected::StateWitnessUnavailable)?;
    let (frame_count, state_bytes) =
        capture_current_state(|| backend.frame_count(), || backend.encode_state_bytes())?;
    let state_identity =
        zeff_coleco_core::save_state::inspect_current_native_tas_state_identity(&state_bytes)
            .map_err(|_| Rejected::StateWitnessUnavailable)?;
    let provenance = backend
        .coleco()
        .and_then(crate::emu_backend::ColecoBackend::tas_load_provenance)
        .ok_or(Rejected::LoadProvenanceUnavailable)?;
    let effective_media_sha256 = backend
        .replay_metadata()
        .rom_sha256
        .ok_or(Rejected::IdentityMetadataMismatch)?;
    if state_identity.bios_sha256 != backend.coleco().unwrap().emu.bios_hash()
        || state_identity.cartridge_sha256 != effective_media_sha256
    {
        return Err(Rejected::StateWitnessUnavailable);
    }
    Ok(TasControlLeaseWitness {
        profile: TasExecutionProfile::DirectColecoCartridge,
        frame_count,
        source_media_sha256: TasDigest(provenance.load.tas_source_media_sha256),
        effective_media_sha256: TasDigest(effective_media_sha256),
        current_state_sha256: TasDigest::from_bytes(&state_bytes),
        current_state_bytes: state_bytes,
        determinism_abi: zeff_coleco_core::save_state::TAS_DETERMINISM_ABI_ID,
        state_format_compatibility_id:
            zeff_coleco_core::save_state::TAS_STATE_FORMAT_COMPATIBILITY_ID,
        sync_config_sha256: TasDigest(provenance.load.tas_sync_config_sha256),
    })
}

fn build_direct_sms_witness(
    backend: &EmuBackend,
    cheats_present: bool,
) -> Result<TasControlLeaseWitness, Rejected> {
    crate::emu_backend::loader::validate_direct_sms_tas_runtime(backend, cheats_present)
        .map_err(|_| Rejected::StateWitnessUnavailable)?;
    let (frame_count, state_bytes) =
        capture_current_state(|| backend.frame_count(), || backend.encode_state_bytes())?;
    let sega8 = backend.sega8().ok_or(Rejected::UnsupportedSystem)?;
    zeff_sega8_core::save_state::inspect_current_native_sms_tas_state(&sega8.emu, &state_bytes)
        .map_err(|_| Rejected::StateWitnessUnavailable)?;
    let provenance = sega8
        .sms_tas_load_provenance()
        .ok_or(Rejected::LoadProvenanceUnavailable)?;
    let effective_media_sha256 = backend
        .replay_metadata()
        .rom_sha256
        .ok_or(Rejected::IdentityMetadataMismatch)?;
    Ok(TasControlLeaseWitness {
        profile: TasExecutionProfile::DirectSmsCartridge,
        frame_count,
        source_media_sha256: TasDigest(provenance.load.tas_source_media_sha256),
        effective_media_sha256: TasDigest(effective_media_sha256),
        current_state_sha256: TasDigest::from_bytes(&state_bytes),
        current_state_bytes: state_bytes,
        determinism_abi: zeff_sega8_core::save_state::TAS_DETERMINISM_ABI_ID,
        state_format_compatibility_id:
            zeff_sega8_core::save_state::TAS_STATE_FORMAT_COMPATIBILITY_ID,
        sync_config_sha256: TasDigest(provenance.load.tas_sync_config_sha256),
    })
}

fn build_direct_game_gear_witness(
    backend: &EmuBackend,
    cheats_present: bool,
) -> Result<TasControlLeaseWitness, Rejected> {
    build_direct_game_gear_witness_with_persistence(
        backend,
        cheats_present,
        TasPersistenceContract::Absent,
    )
}

fn build_direct_game_gear_witness_with_persistence(
    backend: &EmuBackend,
    cheats_present: bool,
    persistence: TasPersistenceContract,
) -> Result<TasControlLeaseWitness, Rejected> {
    match persistence {
        TasPersistenceContract::Absent => {
            crate::emu_backend::loader::validate_direct_game_gear_tas_runtime(
                backend,
                cheats_present,
            )
            .map_err(|_| Rejected::StateWitnessUnavailable)?;
        }
        TasPersistenceContract::GameGearBattery8KiB {
            byte_len,
            initial_sha256,
            ..
        } => {
            crate::emu_backend::loader::validate_direct_game_gear_tas_private_runtime(
                backend,
                cheats_present,
            )
            .map_err(|_| Rejected::StateWitnessUnavailable)?;
            let bytes = backend
                .game_gear_tas_battery_bytes()
                .ok_or(Rejected::PersistentStateNotAbsent)?;
            if byte_len != 8 * 1024
                || bytes.len() as u64 != byte_len
                || TasDigest::from_bytes(&bytes) != initial_sha256
            {
                return Err(Rejected::PersistentStateNotAbsent);
            }
        }
        _ => return Err(Rejected::PersistentStateNotAbsent),
    }
    let (frame_count, state_bytes) =
        capture_current_state(|| backend.frame_count(), || backend.encode_state_bytes())?;
    let sega8 = backend.sega8().ok_or(Rejected::UnsupportedSystem)?;
    let inspection = zeff_sega8_core::save_state::inspect_current_native_game_gear_tas_state(
        &sega8.emu,
        &state_bytes,
    )
    .map_err(|_| Rejected::StateWitnessUnavailable)?;
    let persistence_matches = match persistence {
        TasPersistenceContract::Absent => {
            inspection.save_ram_kind == zeff_emu_common::save_ram::SaveRamKind::None
        }
        TasPersistenceContract::GameGearBattery8KiB { byte_len, .. } => {
            inspection.save_ram_kind
                == zeff_emu_common::save_ram::SaveRamKind::known_battery_backed(8 * 1024)
                && byte_len == 8 * 1024
        }
        _ => false,
    };
    if !persistence_matches || inspection.serial.peer_present {
        return Err(Rejected::StateWitnessUnavailable);
    }
    let provenance = sega8
        .game_gear_tas_load_provenance()
        .ok_or(Rejected::LoadProvenanceUnavailable)?;
    let effective_media_sha256 = backend
        .replay_metadata()
        .rom_sha256
        .ok_or(Rejected::IdentityMetadataMismatch)?;
    Ok(TasControlLeaseWitness {
        profile: TasExecutionProfile::DirectGameGearCartridge,
        frame_count,
        source_media_sha256: TasDigest(provenance.tas_source_media_sha256),
        effective_media_sha256: TasDigest(effective_media_sha256),
        current_state_sha256: TasDigest::from_bytes(&state_bytes),
        current_state_bytes: state_bytes,
        determinism_abi: zeff_sega8_core::save_state::GAME_GEAR_TAS_DETERMINISM_ABI_ID,
        state_format_compatibility_id:
            zeff_sega8_core::save_state::TAS_STATE_FORMAT_COMPATIBILITY_ID,
        sync_config_sha256: TasDigest(provenance.tas_sync_config_sha256),
    })
}

fn build_direct_gba_witness(
    backend: &EmuBackend,
    cheats_present: bool,
) -> Result<TasControlLeaseWitness, Rejected> {
    build_direct_gba_witness_with_persistence(
        backend,
        cheats_present,
        TasPersistenceContract::Absent,
    )
}

fn build_direct_gba_witness_with_persistence(
    backend: &EmuBackend,
    cheats_present: bool,
    persistence: TasPersistenceContract,
) -> Result<TasControlLeaseWitness, Rejected> {
    match persistence {
        TasPersistenceContract::Absent => {
            crate::emu_backend::gba::validate_direct_gba_tas_runtime(backend, cheats_present)
                .map_err(|_| Rejected::StateWitnessUnavailable)?;
        }
        TasPersistenceContract::GbaBattery {
            kind,
            byte_len,
            initial_sha256,
            ..
        } => {
            crate::emu_backend::gba::validate_direct_gba_tas_private_runtime(
                backend,
                cheats_present,
            )
            .map_err(|_| Rejected::StateWitnessUnavailable)?;
            let (actual_kind, bytes) = backend
                .gba_tas_battery_component()
                .map_err(|_| Rejected::StateWitnessUnavailable)?
                .ok_or(Rejected::PersistentStateNotAbsent)?;
            if actual_kind != kind
                || bytes.len() as u64 != byte_len
                || TasDigest::from_bytes(&bytes) != initial_sha256
            {
                return Err(Rejected::PersistentStateNotAbsent);
            }
        }
        TasPersistenceContract::GbaRtcBattery {
            kind,
            persistent_state,
            rtc_state,
            byte_len,
            initial_sha256,
            ..
        } => {
            crate::emu_backend::gba::validate_direct_gba_tas_private_runtime(
                backend,
                cheats_present,
            )
            .map_err(|_| Rejected::StateWitnessUnavailable)?;
            let witness = crate::emu_backend::gba::gba_rtc_persistence_witness(backend)
                .map_err(|_| Rejected::StateWitnessUnavailable)?;
            if witness.backup_kind != kind
                || witness.persistent_state != persistent_state
                || witness.rtc_state != rtc_state
                || witness.complete_byte_len != byte_len
                || witness.complete_sha256 != initial_sha256
            {
                return Err(Rejected::PersistentStateNotAbsent);
            }
        }
        TasPersistenceContract::NesBattery { .. }
        | TasPersistenceContract::GbBattery { .. }
        | TasPersistenceContract::GbRtcBattery { .. }
        | TasPersistenceContract::GameGearBattery8KiB { .. }
        | TasPersistenceContract::WsBattery { .. }
        | TasPersistenceContract::WsRtcBattery { .. } => {
            return Err(Rejected::PersistentStateNotAbsent);
        }
    }
    let (frame_count, state_bytes) =
        capture_current_state(|| backend.frame_count(), || backend.encode_state_bytes())?;
    let gba = backend.gba().ok_or(Rejected::UnsupportedSystem)?;
    let inspection =
        zeff_gba_core::save_state::inspect_current_native_gba_tas_state(&gba.emu, &state_bytes)
            .map_err(|_| Rejected::StateWitnessUnavailable)?;
    let persistence_matches = match persistence {
        TasPersistenceContract::Absent => {
            inspection.save_ram_kind == zeff_emu_common::save_ram::SaveRamKind::None
                && inspection.battery_data.is_none()
        }
        TasPersistenceContract::GbaBattery {
            byte_len,
            initial_sha256,
            ..
        } => inspection.battery_data.as_ref().is_some_and(|bytes| {
            inspection.save_ram_kind.is_battery_backed()
                && bytes.len() as u64 == byte_len
                && TasDigest::from_bytes(bytes) == initial_sha256
        }),
        TasPersistenceContract::GbaRtcBattery { .. } => inspection.rtc_present,
        TasPersistenceContract::NesBattery { .. }
        | TasPersistenceContract::GbBattery { .. }
        | TasPersistenceContract::GbRtcBattery { .. }
        | TasPersistenceContract::GameGearBattery8KiB { .. }
        | TasPersistenceContract::WsBattery { .. }
        | TasPersistenceContract::WsRtcBattery { .. } => false,
    };
    if inspection.rom_sha256 != gba.emu.rom_hash()
        || !persistence_matches
        || inspection.rtc_present
            != matches!(persistence, TasPersistenceContract::GbaRtcBattery { .. })
        || inspection.external_bios
        || inspection.executing_in_bios
        || inspection.sample_rate != crate::emu_backend::gba::DIRECT_GBA_SAMPLE_RATE
    {
        return Err(Rejected::StateWitnessUnavailable);
    }
    let provenance = backend
        .gba_tas_load_provenance()
        .ok_or(Rejected::LoadProvenanceUnavailable)?;
    let effective_media_sha256 = backend
        .replay_metadata()
        .rom_sha256
        .ok_or(Rejected::IdentityMetadataMismatch)?;
    Ok(TasControlLeaseWitness {
        profile: TasExecutionProfile::DirectGbaCartridge,
        frame_count,
        source_media_sha256: TasDigest(provenance.load.tas_source_media_sha256),
        effective_media_sha256: TasDigest(effective_media_sha256),
        current_state_sha256: TasDigest::from_bytes(&state_bytes),
        current_state_bytes: state_bytes,
        determinism_abi: zeff_gba_core::save_state::TAS_DETERMINISM_ABI_ID,
        state_format_compatibility_id: zeff_gba_core::save_state::TAS_STATE_FORMAT_COMPATIBILITY_ID,
        sync_config_sha256: TasDigest(provenance.load.tas_sync_config_sha256),
    })
}

fn build_direct_sg1000_witness(
    backend: &EmuBackend,
    cheats_present: bool,
) -> Result<TasControlLeaseWitness, Rejected> {
    crate::emu_backend::loader::validate_direct_sg1000_tas_runtime(backend, cheats_present)
        .map_err(|_| Rejected::StateWitnessUnavailable)?;
    let (frame_count, state_bytes) =
        capture_current_state(|| backend.frame_count(), || backend.encode_state_bytes())?;
    let sega8 = backend.sega8().ok_or(Rejected::UnsupportedSystem)?;
    zeff_sega8_core::save_state::inspect_current_native_sg1000_tas_state(&sega8.emu, &state_bytes)
        .map_err(|_| Rejected::StateWitnessUnavailable)?;
    let provenance = sega8
        .sg1000_tas_load_provenance()
        .ok_or(Rejected::LoadProvenanceUnavailable)?;
    let effective_media_sha256 = backend
        .replay_metadata()
        .rom_sha256
        .ok_or(Rejected::IdentityMetadataMismatch)?;
    Ok(TasControlLeaseWitness {
        profile: TasExecutionProfile::DirectSg1000Cartridge,
        frame_count,
        source_media_sha256: TasDigest(provenance.load.tas_source_media_sha256),
        effective_media_sha256: TasDigest(effective_media_sha256),
        current_state_sha256: TasDigest::from_bytes(&state_bytes),
        current_state_bytes: state_bytes,
        determinism_abi: zeff_sega8_core::save_state::SG1000_TAS_DETERMINISM_ABI_ID,
        state_format_compatibility_id:
            zeff_sega8_core::save_state::TAS_STATE_FORMAT_COMPATIBILITY_ID,
        sync_config_sha256: TasDigest(provenance.load.tas_sync_config_sha256),
    })
}

fn build_direct_ws_witness(
    backend: &EmuBackend,
    cheats_present: bool,
) -> Result<TasControlLeaseWitness, Rejected> {
    build_direct_ws_witness_with_persistence(
        backend,
        cheats_present,
        TasPersistenceContract::Absent,
    )
}

fn build_direct_ws_witness_with_persistence(
    backend: &EmuBackend,
    cheats_present: bool,
    persistence: TasPersistenceContract,
) -> Result<TasControlLeaseWitness, Rejected> {
    let inspection = match persistence {
        TasPersistenceContract::Absent => {
            if backend.save_ram_kind() != zeff_emu_common::save_ram::SaveRamKind::None {
                return Err(Rejected::PersistentStateNotAbsent);
            }
            crate::emu_backend::loader::validate_direct_ws_tas_runtime(backend, cheats_present)
                .map_err(|_| Rejected::StateWitnessUnavailable)?
        }
        TasPersistenceContract::WsBattery {
            save_kind,
            byte_len,
            initial_sha256,
            ..
        } => {
            let inspection = crate::emu_backend::loader::validate_direct_ws_tas_private_runtime(
                backend,
                cheats_present,
            )
            .map_err(|_| Rejected::StateWitnessUnavailable)?;
            let bytes = backend
                .ws_tas_battery_bytes()
                .ok_or(Rejected::PersistentStateNotAbsent)?;
            if inspection.save_kind != save_kind
                || inspection.cartridge_save_len as u64 != byte_len
                || bytes.len() as u64 != byte_len
                || TasDigest::from_bytes(&bytes) != initial_sha256
            {
                return Err(Rejected::PersistentStateNotAbsent);
            }
            inspection
        }
        TasPersistenceContract::WsRtcBattery {
            save_kind,
            persistent_state,
            rtc_state,
            byte_len,
            initial_sha256,
            ..
        } => {
            let inspection = crate::emu_backend::loader::validate_direct_ws_tas_private_runtime(
                backend,
                cheats_present,
            )
            .map_err(|_| Rejected::StateWitnessUnavailable)?;
            let witness = crate::emu_backend::ws::ws_rtc_persistence_witness(backend)
                .map_err(|_| Rejected::PersistentStateNotAbsent)?;
            if inspection.save_kind != save_kind
                || witness.save_kind != save_kind
                || witness.persistent_state != persistent_state
                || witness.rtc_state != rtc_state
                || witness.complete_byte_len != byte_len
                || witness.complete_sha256 != initial_sha256
            {
                return Err(Rejected::PersistentStateNotAbsent);
            }
            inspection
        }
        _ => return Err(Rejected::PersistentStateNotAbsent),
    };
    let (frame_count, state_bytes) =
        capture_current_state(|| backend.frame_count(), || backend.encode_state_bytes())?;
    let ws = backend.ws().ok_or(Rejected::UnsupportedSystem)?;
    let state_inspection = zeff_ws_core::save_state::inspect_current_native_wonder_swan_tas_state(
        &ws.emu,
        &state_bytes,
    )
    .map_err(|_| Rejected::StateWitnessUnavailable)?;
    if inspection.rom_sha256 != state_inspection.rom_sha256
        || inspection.orientation != state_inspection.orientation
        || inspection.save_kind != state_inspection.save_kind
        || inspection.cartridge_save_len != state_inspection.cartridge_save_len
        || inspection.cartridge_save_sha256 != state_inspection.cartridge_save_sha256
        || inspection.rtc_present != state_inspection.rtc_present
        || inspection.rtc != state_inspection.rtc
        || !state_inspection.uart.is_disconnected()
    {
        return Err(Rejected::StateWitnessUnavailable);
    }
    let provenance = ws
        .tas_load_provenance()
        .ok_or(Rejected::LoadProvenanceUnavailable)?;
    let effective_media_sha256 = backend
        .replay_metadata()
        .rom_sha256
        .ok_or(Rejected::IdentityMetadataMismatch)?;
    Ok(TasControlLeaseWitness {
        profile: TasExecutionProfile::DirectWsCartridge,
        frame_count,
        source_media_sha256: TasDigest(provenance.load.tas_source_media_sha256),
        effective_media_sha256: TasDigest(effective_media_sha256),
        current_state_sha256: TasDigest::from_bytes(&state_bytes),
        current_state_bytes: state_bytes,
        determinism_abi: zeff_ws_core::save_state::TAS_DETERMINISM_ABI_ID,
        state_format_compatibility_id: zeff_ws_core::save_state::TAS_STATE_FORMAT_COMPATIBILITY_ID,
        sync_config_sha256: TasDigest(provenance.load.tas_sync_config_sha256),
    })
}

fn build_direct_pce_witness(
    backend: &EmuBackend,
    cheats_present: bool,
    profile: TasExecutionProfile,
) -> Result<TasControlLeaseWitness, Rejected> {
    let inspection = match profile {
        TasExecutionProfile::DirectPceHuCard => {
            crate::emu_backend::loader::validate_direct_pce_tas_runtime(backend, cheats_present)
        }
        TasExecutionProfile::DirectPceSixButtonHuCard => {
            crate::emu_backend::loader::validate_direct_pce_six_button_tas_runtime(
                backend,
                cheats_present,
            )
        }
        _ => return Err(Rejected::UnsupportedSystem),
    }
    .map_err(|_| Rejected::StateWitnessUnavailable)?;
    let (frame_count, state_bytes) =
        capture_current_state(|| backend.frame_count(), || backend.encode_state_bytes())?;
    let pce = backend.pce().ok_or(Rejected::UnsupportedSystem)?;
    let state_inspection = pce
        .inspect_current_native_tas_state(&state_bytes)
        .map_err(|_| Rejected::StateWitnessUnavailable)?;
    if inspection.normalized_rom_sha256 != state_inspection.normalized_rom_sha256
        || frame_count != state_inspection.projection.frame_count
        || !pce.tas_frame_counters_match()
    {
        return Err(Rejected::StateWitnessUnavailable);
    }
    let provenance = pce
        .tas_load_provenance()
        .ok_or(Rejected::LoadProvenanceUnavailable)?;
    let effective_media_sha256 = backend
        .replay_metadata()
        .rom_sha256
        .ok_or(Rejected::IdentityMetadataMismatch)?;
    Ok(TasControlLeaseWitness {
        profile,
        frame_count,
        source_media_sha256: TasDigest(provenance.load.raw_source_media_sha256),
        effective_media_sha256: TasDigest(effective_media_sha256),
        current_state_sha256: TasDigest::from_bytes(&state_bytes),
        current_state_bytes: state_bytes,
        determinism_abi: zeff_pce_core::hardware::save_state::tas::TAS_DETERMINISM_ABI_ID,
        state_format_compatibility_id:
            zeff_pce_core::hardware::save_state::tas::TAS_STATE_FORMAT_COMPATIBILITY_ID,
        sync_config_sha256: TasDigest(provenance.load.tas_sync_config_sha256),
    })
}

fn build_direct_pce_cd_witness(
    backend: &EmuBackend,
    cheats_present: bool,
) -> Result<TasControlLeaseWitness, Rejected> {
    let inspection =
        crate::emu_backend::loader::validate_direct_pce_cd_tas_runtime(backend, cheats_present)
            .map_err(|_| Rejected::StateWitnessUnavailable)?;
    let (frame_count, state_bytes) =
        capture_current_state(|| backend.frame_count(), || backend.encode_state_bytes())?;
    let pce = backend.pce().ok_or(Rejected::UnsupportedSystem)?;
    let state_inspection = pce
        .inspect_current_native_cd_tas_state_for_profile(
            &state_bytes,
            inspection.arcade_card_enabled,
            inspection.memory_base_enabled,
        )
        .map_err(|_| Rejected::StateWitnessUnavailable)?;
    if inspection.system_card_sha256 != state_inspection.system_card_sha256
        || inspection.disc_sha256 != state_inspection.disc_sha256
        || frame_count != state_inspection.projection.frame_count
        || !pce.tas_frame_counters_match()
    {
        return Err(Rejected::StateWitnessUnavailable);
    }
    let provenance = pce
        .tas_load_provenance()
        .ok_or(Rejected::LoadProvenanceUnavailable)?;
    Ok(TasControlLeaseWitness {
        profile: TasExecutionProfile::DirectPceCd,
        frame_count,
        source_media_sha256: TasDigest(provenance.load.tas_source_media_sha256),
        effective_media_sha256: TasDigest(inspection.disc_sha256),
        current_state_sha256: TasDigest::from_bytes(&state_bytes),
        current_state_bytes: state_bytes,
        determinism_abi: zeff_pce_core::hardware::save_state::tas::TAS_DETERMINISM_ABI_ID,
        state_format_compatibility_id:
            zeff_pce_core::hardware::save_state::tas::TAS_STATE_FORMAT_COMPATIBILITY_ID,
        sync_config_sha256: TasDigest(provenance.load.tas_sync_config_sha256),
    })
}

fn capture_current_state<F, E>(mut frame_count: F, encode: E) -> Result<(u64, Vec<u8>), Rejected>
where
    F: FnMut() -> u64,
    E: FnOnce() -> anyhow::Result<Vec<u8>>,
{
    let before = frame_count();
    let bytes = encode().map_err(|_| Rejected::StateWitnessUnavailable)?;
    if frame_count() != before {
        return Err(Rejected::StateWitnessUnavailable);
    }
    Ok((before, bytes))
}

pub(super) fn validate_direct_nes_profile(facts: &DirectNesProfileFacts) -> Result<(), Rejected> {
    validate_direct_nes_profile_mode(facts, false)
}

fn validate_direct_nes_profile_mode(
    facts: &DirectNesProfileFacts,
    allow_project_battery: bool,
) -> Result<(), Rejected> {
    if facts.system != ActiveSystem::Nes {
        return Err(Rejected::UnsupportedSystem);
    }
    if !facts.identity_metadata_matches {
        return Err(Rejected::IdentityMetadataMismatch);
    }
    let Some(provenance) = facts.provenance else {
        return Err(Rejected::LoadProvenanceUnavailable);
    };
    if !provenance.direct_nes_file
        && provenance.raw_source_media_sha256 == facts.effective_media_sha256
    {
        return Err(Rejected::DirectNesFileRequired);
    }
    let direct_media = provenance.raw_source_media_sha256 == facts.effective_media_sha256;
    let direct_sync = crate::emu_backend::loader::direct_nes_tas_sync_config_sha256().0;
    let direct_battery_sync =
        crate::emu_backend::loader::direct_nes_battery_tas_sync_config_sha256().0;
    let expected_direct_sync = if facts.battery_backed {
        direct_battery_sync
    } else {
        direct_sync
    };
    if (direct_media && provenance.sync_config_sha256 != expected_direct_sync)
        || (!direct_media
            && (provenance.sync_config_sha256 == direct_sync
                || provenance.sync_config_sha256 == direct_battery_sync))
    {
        return Err(Rejected::SourceMediaMismatch);
    }
    if provenance.any_mod_enabled || provenance.any_mod_applied {
        return Err(Rejected::ModsEnabledOrApplied);
    }
    let persistence_matches = provenance.persistent_load == NesPersistentLoadOutcome::Absent
        && if allow_project_battery {
            facts.battery_backed && facts.battery_state_available
        } else {
            !facts.battery_backed && !facts.battery_state_available
        };
    if !persistence_matches {
        return Err(Rejected::PersistentStateNotAbsent);
    }
    if provenance.initial_input.buttons != 0 || provenance.initial_input.dpad != 0 {
        return Err(Rejected::NonNeutralInitialInput);
    }
    let default_rate = zeff_nes_core::hardware::constants::NES_DEFAULT_HOST_SAMPLE_RATE_HZ;
    if provenance
        .configured_sample_rate
        .is_some_and(|rate| rate != default_rate)
        || provenance.initial_sample_rate != default_rate
        || facts.current_sample_rate != Some(default_rate)
    {
        return Err(Rejected::NonDefaultSampleRate);
    }
    if facts.firmware_present {
        return Err(Rejected::FirmwarePresent);
    }
    if !facts.standard_console_hardware {
        return Err(Rejected::NonStandardConsoleHardware);
    }
    if !facts.supported_controller_topology {
        return Err(Rejected::NonStandardControllerTopology);
    }
    if facts.removable_media_present {
        return Err(Rejected::RemovableMediaPresent);
    }
    if facts.cheats_present {
        return Err(Rejected::CheatsPresent);
    }
    Ok(())
}

#[cfg(test)]
mod tests;
