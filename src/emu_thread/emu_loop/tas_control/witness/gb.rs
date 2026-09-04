use zeff_gb_core::hardware::GameBoySerialDevice;
use zeff_gb_core::hardware::ppu::DmgPalettePreset;
use zeff_gb_core::hardware::types::hardware_mode::{HardwareMode, HardwareModePreference};

use super::{Rejected, capture_current_state};
use crate::emu_backend::{ActiveSystem, EmuBackend};
use crate::emu_thread::{
    TasControlLeaseWitness, TasExecutionProfile, TasLoadedProfileObservation,
    TasPersistenceContract,
};
use crate::tas_project::TasDigest;

pub(super) fn observe_direct_gb(
    backend: &EmuBackend,
    cheats_present: bool,
) -> TasLoadedProfileObservation {
    observe(
        backend,
        cheats_present,
        TasExecutionProfile::DirectGbCartridgeDmg,
    )
}

pub(super) fn observe_direct_gbc(
    backend: &EmuBackend,
    cheats_present: bool,
) -> TasLoadedProfileObservation {
    observe(
        backend,
        cheats_present,
        TasExecutionProfile::DirectGbCartridgeCgb,
    )
}

fn observe(
    backend: &EmuBackend,
    cheats_present: bool,
    profile: TasExecutionProfile,
) -> TasLoadedProfileObservation {
    let metadata = backend.replay_metadata();
    let provenance = backend.gb_tas_load_provenance();
    let load = provenance.map(|view| view.load);
    let expected_core_family = format!("{:?}", zeff_emu_common::system::CoreFamily::GameBoy);
    let expected_firmware =
        crate::emu_backend::firmware::default_firmware_manifests_for_active_system(
            ActiveSystem::GameBoy,
        );
    let is_cgb = profile == TasExecutionProfile::DirectGbCartridgeCgb;
    TasLoadedProfileObservation {
        profile,
        system: backend.system(),
        identity_metadata_matches: metadata.system.as_deref() == Some(ActiveSystem::GameBoy.code())
            && metadata.core_family.as_deref() == Some(expected_core_family.as_str()),
        load_provenance_available: provenance.is_some(),
        direct_source: load.map(|load| {
            if is_cgb {
                load.direct_gbc_file
            } else {
                load.direct_gb_file
            }
        }),
        source_media_sha256: load.map(|load| TasDigest(load.tas_source_media_sha256)),
        effective_media_sha256: metadata.rom_sha256.map(TasDigest),
        mods_absent: load.map(|load| !load.any_mod_enabled && !load.any_mod_applied),
        persistent_state_absent: load.map(|load| {
            load.persistent_load == crate::emu_backend::gb::GbPersistentLoadOutcome::Absent
        }),
        project_owned_persistence: None,
        initial_input_neutral: load
            .map(|load| load.initial_input.buttons == 0 && load.initial_input.dpad == 0),
        configured_at_load_sample_rate: load.and_then(|load| load.configured_sample_rate),
        initial_sample_rate: load.map(|load| load.initial_sample_rate),
        current_sample_rate: provenance.map(|view| view.current_sample_rate),
        firmware_profile_matches: metadata.firmware == expected_firmware
            && metadata.firmware.iter().all(|firmware| {
                matches!(
                    firmware,
                    zeff_emu_common::replay::ReplayFirmwareManifest::Skipped { .. }
                )
            }),
        hardware_profile_matches: provenance.is_some_and(|view| {
            !view.load.external_boot_rom_used
                && !view.has_external_boot_rom
                && crate::emu_backend::loader::is_supported_direct_gb_tas_cartridge(
                    view.cartridge_type,
                    view.rom_size,
                    view.ram_size,
                    view.load.raw_source_media_len,
                )
                && if is_cgb {
                    view.load.requested_hardware_mode == HardwareModePreference::ForceCgb
                        && view.load.resolved_hardware_mode == HardwareMode::CGBNormal
                        && matches!(
                            view.current_hardware_mode,
                            HardwareMode::CGBNormal | HardwareMode::CGBDouble
                        )
                        && view.current_hardware_mode_preference == HardwareModePreference::ForceCgb
                        && view.is_cgb_exclusive
                } else {
                    view.load.requested_hardware_mode == HardwareModePreference::ForceDmg
                        && view.load.resolved_hardware_mode == HardwareMode::DMG
                        && view.current_hardware_mode == HardwareMode::DMG
                        && view.current_hardware_mode_preference == HardwareModePreference::ForceDmg
                        && !view.is_cgb_exclusive
                        && view.dmg_palette_preset == DmgPalettePreset::default()
                }
        }),
        controller_profile_matches: provenance
            .is_some_and(|view| view.current_serial_device == GameBoySerialDevice::Disconnected),
        removable_media_absent: backend.media_slot_snapshot().is_none(),
        cheats_absent: !cheats_present && metadata.cheat_sha256.is_none(),
    }
}

pub(super) fn build_direct_gb_witness(
    backend: &EmuBackend,
    cheats_present: bool,
    profile: TasExecutionProfile,
) -> Result<TasControlLeaseWitness, Rejected> {
    build_direct_gb_witness_with_persistence(
        backend,
        cheats_present,
        profile,
        TasPersistenceContract::Absent,
    )
}

pub(super) fn build_direct_gb_witness_with_persistence(
    backend: &EmuBackend,
    cheats_present: bool,
    profile: TasExecutionProfile,
    persistence: TasPersistenceContract,
) -> Result<TasControlLeaseWitness, Rejected> {
    match (profile, persistence) {
        (TasExecutionProfile::DirectGbCartridgeDmg, TasPersistenceContract::Absent) => {
            crate::emu_backend::loader::validate_direct_gb_tas_runtime(backend, cheats_present)
        }
        (
            TasExecutionProfile::DirectGbCartridgeDmg,
            TasPersistenceContract::GbBattery {
                byte_len,
                initial_sha256,
                ..
            },
        ) => (|| -> anyhow::Result<()> {
            crate::emu_backend::loader::validate_direct_gb_tas_runtime_with_project_sram(
                backend,
                cheats_present,
            )?;
            let bytes = backend
                .gb_tas_battery_bytes()
                .ok_or_else(|| anyhow::anyhow!("Game Boy battery state is unavailable"))?;
            anyhow::ensure!(
                bytes.len() as u64 == byte_len && TasDigest::from_bytes(&bytes) == initial_sha256,
                "Game Boy battery state does not match the project-owned state"
            );
            Ok(())
        })(),
        (
            TasExecutionProfile::DirectGbCartridgeDmg,
            TasPersistenceContract::GbRtcBattery {
                persistent_state,
                rtc_state,
                byte_len,
                initial_sha256,
                ..
            },
        ) => (|| -> anyhow::Result<()> {
            crate::emu_backend::loader::validate_direct_gb_tas_runtime_with_project_rtc(
                backend,
                cheats_present,
            )?;
            let witness = crate::emu_backend::loader::gb_rtc_persistence_witness(backend)?;
            anyhow::ensure!(
                witness.persistent_state == persistent_state
                    && witness.rtc_state == rtc_state
                    && witness.complete_byte_len == byte_len
                    && witness.complete_sha256 == initial_sha256,
                "Game Boy RTC state does not match the project-owned state"
            );
            Ok(())
        })(),
        (TasExecutionProfile::DirectGbCartridgeCgb, TasPersistenceContract::Absent) => {
            crate::emu_backend::loader::validate_direct_gbc_tas_runtime(backend, cheats_present)
        }
        (
            TasExecutionProfile::DirectGbCartridgeCgb,
            TasPersistenceContract::GbBattery {
                byte_len,
                initial_sha256,
                ..
            },
        ) => (|| -> anyhow::Result<()> {
            crate::emu_backend::loader::validate_direct_gbc_tas_runtime_with_project_sram(
                backend,
                cheats_present,
            )?;
            let bytes = backend
                .gb_tas_battery_bytes()
                .ok_or_else(|| anyhow::anyhow!("Game Boy Color battery state is unavailable"))?;
            anyhow::ensure!(
                bytes.len() as u64 == byte_len && TasDigest::from_bytes(&bytes) == initial_sha256,
                "Game Boy Color battery state does not match the project-owned state"
            );
            Ok(())
        })(),
        (
            TasExecutionProfile::DirectGbCartridgeCgb,
            TasPersistenceContract::GbRtcBattery {
                persistent_state,
                rtc_state,
                byte_len,
                initial_sha256,
                ..
            },
        ) => (|| -> anyhow::Result<()> {
            crate::emu_backend::loader::validate_direct_gbc_tas_runtime_with_project_rtc(
                backend,
                cheats_present,
            )?;
            let witness = crate::emu_backend::loader::gb_rtc_persistence_witness(backend)?;
            anyhow::ensure!(
                witness.persistent_state == persistent_state
                    && witness.rtc_state == rtc_state
                    && witness.complete_byte_len == byte_len
                    && witness.complete_sha256 == initial_sha256,
                "Game Boy Color RTC state does not match the project-owned state"
            );
            Ok(())
        })(),
        _ => Err(anyhow::anyhow!(
            "Game Boy persistence contract does not match the execution profile"
        )),
    }
    .map_err(|_| Rejected::StateWitnessUnavailable)?;
    let (frame_count, state_bytes) =
        capture_current_state(|| backend.frame_count(), || backend.encode_state_bytes())?;
    if profile == TasExecutionProfile::DirectGbCartridgeCgb {
        if matches!(persistence, TasPersistenceContract::GbRtcBattery { .. }) {
            crate::emu_backend::loader::validate_direct_gbc_state_for_backend_with_project_rtc(
                backend,
                &state_bytes,
                false,
            )
            .map_err(|_| Rejected::StateWitnessUnavailable)?;
        } else if matches!(persistence, TasPersistenceContract::GbBattery { .. }) {
            crate::emu_backend::loader::validate_direct_gbc_state_for_backend_with_project_sram(
                backend,
                &state_bytes,
                false,
            )
            .map_err(|_| Rejected::StateWitnessUnavailable)?;
        } else {
            crate::emu_backend::loader::validate_direct_gbc_state_for_backend(
                backend,
                &state_bytes,
                false,
            )
            .map_err(|_| Rejected::StateWitnessUnavailable)?;
        }
    } else {
        crate::emu_backend::loader::validate_direct_gb_tas_state(&state_bytes)
            .map_err(|_| Rejected::StateWitnessUnavailable)?;
    }
    let provenance = backend
        .gb_tas_load_provenance()
        .ok_or(Rejected::LoadProvenanceUnavailable)?;
    let effective_media_sha256 = backend
        .replay_metadata()
        .rom_sha256
        .ok_or(Rejected::IdentityMetadataMismatch)?;
    Ok(TasControlLeaseWitness {
        profile,
        frame_count,
        source_media_sha256: TasDigest(provenance.load.tas_source_media_sha256),
        effective_media_sha256: TasDigest(effective_media_sha256),
        current_state_sha256: TasDigest::from_bytes(&state_bytes),
        current_state_bytes: state_bytes,
        determinism_abi: zeff_gb_core::save_state::TAS_DETERMINISM_ABI_ID,
        state_format_compatibility_id: zeff_gb_core::save_state::TAS_STATE_FORMAT_COMPATIBILITY_ID,
        sync_config_sha256: if provenance.load.tas_sync_config_sha256 != [0; 32] {
            TasDigest(provenance.load.tas_sync_config_sha256)
        } else if profile == TasExecutionProfile::DirectGbCartridgeCgb {
            crate::emu_backend::loader::direct_gbc_tas_sync_config_sha256()
        } else {
            crate::emu_backend::loader::direct_gb_tas_sync_config_sha256()
        },
    })
}
