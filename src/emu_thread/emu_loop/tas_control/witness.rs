use crate::emu_backend::nes::{NesPersistentLoadOutcome, NesTasLoadProvenance};
use crate::emu_backend::{ActiveSystem, EmuBackend};
use crate::emu_thread::{
    TasControlAcquireRejectedReason as Rejected, TasControlLeaseWitness, TasExecutionProfile,
};
use crate::tas_project::TasDigest;

pub(super) struct DirectNesProfileFacts {
    pub(super) system: ActiveSystem,
    pub(super) identity_metadata_matches: bool,
    pub(super) provenance: Option<NesTasLoadProvenance>,
    pub(super) current_sample_rate: Option<u32>,
    pub(super) effective_media_sha256: [u8; 32],
    pub(super) firmware_present: bool,
    pub(super) standard_console_hardware: bool,
    pub(super) supported_controller_topology: bool,
    pub(super) removable_media_present: bool,
    pub(super) cheats_present: bool,
}

pub(super) fn build_tas_witness(
    backend: &EmuBackend,
    cheats_present: bool,
    profile: TasExecutionProfile,
) -> Result<TasControlLeaseWitness, Rejected> {
    match profile {
        TasExecutionProfile::DirectNesCartridge => {
            build_direct_nes_witness(backend, cheats_present)
        }
        TasExecutionProfile::DirectGbRomOnlyDmg => build_direct_gb_witness(backend, cheats_present),
    }
}

fn build_direct_nes_witness(
    backend: &EmuBackend,
    cheats_present: bool,
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
        firmware_present: !metadata.firmware.is_empty(),
        standard_console_hardware: backend
            .nes()
            .is_some_and(|nes| nes.has_standard_console_hardware()),
        supported_controller_topology: backend.nes_has_standard_or_zapper_controller_topology()
            == Some(true),
        removable_media_present: backend.media_slot_snapshot().is_some(),
        cheats_present,
    };
    validate_direct_nes_profile(&facts)?;

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
        sync_config_sha256: crate::emu_backend::loader::direct_nes_tas_sync_config_sha256(),
    })
}

fn build_direct_gb_witness(
    backend: &EmuBackend,
    cheats_present: bool,
) -> Result<TasControlLeaseWitness, Rejected> {
    crate::emu_backend::loader::validate_direct_gb_tas_runtime(backend, cheats_present)
        .map_err(|_| Rejected::StateWitnessUnavailable)?;
    let (frame_count, state_bytes) =
        capture_current_state(|| backend.frame_count(), || backend.encode_state_bytes())?;
    crate::emu_backend::loader::validate_direct_gb_tas_state(&state_bytes)
        .map_err(|_| Rejected::StateWitnessUnavailable)?;
    let provenance = backend
        .gb_tas_load_provenance()
        .ok_or(Rejected::LoadProvenanceUnavailable)?;
    let metadata = backend.replay_metadata();
    let effective_media_sha256 = metadata
        .rom_sha256
        .ok_or(Rejected::IdentityMetadataMismatch)?;
    Ok(TasControlLeaseWitness {
        profile: TasExecutionProfile::DirectGbRomOnlyDmg,
        frame_count,
        source_media_sha256: TasDigest(provenance.load.raw_source_media_sha256),
        effective_media_sha256: TasDigest(effective_media_sha256),
        current_state_sha256: TasDigest::from_bytes(&state_bytes),
        current_state_bytes: state_bytes,
        determinism_abi: zeff_gb_core::save_state::TAS_DETERMINISM_ABI_ID,
        state_format_compatibility_id: zeff_gb_core::save_state::TAS_STATE_FORMAT_COMPATIBILITY_ID,
        sync_config_sha256: crate::emu_backend::loader::direct_gb_tas_sync_config_sha256(),
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
    if facts.system != ActiveSystem::Nes {
        return Err(Rejected::UnsupportedSystem);
    }
    if !facts.identity_metadata_matches {
        return Err(Rejected::IdentityMetadataMismatch);
    }
    let Some(provenance) = facts.provenance else {
        return Err(Rejected::LoadProvenanceUnavailable);
    };
    if !provenance.direct_nes_file {
        return Err(Rejected::DirectNesFileRequired);
    }
    if provenance.raw_source_media_sha256 != facts.effective_media_sha256 {
        return Err(Rejected::SourceMediaMismatch);
    }
    if provenance.any_mod_enabled || provenance.any_mod_applied {
        return Err(Rejected::ModsEnabledOrApplied);
    }
    if provenance.persistent_load != NesPersistentLoadOutcome::Absent {
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
