use anyhow::{Context, Result, ensure};
use zeff_emu_common::save_ram::SaveRamKind;

use super::{ActiveSystem, EmuBackend};
use crate::tas_project::{
    TasCameraInput, TasDeviceIdentity, TasDigest, TasExternalIdentity, TasProject,
    TasProjectIdentity,
};

const SG1000_CONTROLLER_CONFIGURATION: &[u8] =
    b"zeff-tas-device-config-v1\0sg1000-standard-controller\0";
const SG1000_SYNC_CONFIGURATION: &[u8] = b"zeff-tas-sync-config-v1\0sg1000-direct-cartridge\0hardware=sg1000-ntsc-japanese\0mapper=core-auto-sega-only\0controllers=two-standard\0mods=disabled\0external-state=absent\0type-b-ram=absent\0initial-input=neutral\0sample-rate=48000\0firmware=absent\0";
const SG1000_ZIP_SYNC_CONFIGURATION: &[u8] = b"zeff-tas-sync-config-v1\0sg1000-zip-member\0hardware=sg1000-ntsc-japanese\0mapper=core-auto-sega-only\0controllers=two-standard\0mods=disabled\0external-state=absent\0type-b-ram=absent\0initial-input=neutral\0sample-rate=48000\0firmware=absent\0member=";

pub(crate) fn direct_sg1000_tas_sync_config_sha256() -> TasDigest {
    TasDigest::from_bytes(SG1000_SYNC_CONFIGURATION)
}

pub(crate) fn zip_sg1000_tas_sync_config_sha256(member_name: &str) -> TasDigest {
    let mut bytes = Vec::with_capacity(SG1000_ZIP_SYNC_CONFIGURATION.len() + member_name.len());
    bytes.extend_from_slice(SG1000_ZIP_SYNC_CONFIGURATION);
    bytes.extend_from_slice(member_name.as_bytes());
    TasDigest::from_bytes(&bytes)
}

fn direct_sg1000_tas_devices() -> Vec<TasDeviceIdentity> {
    ["p1", "p2"]
        .into_iter()
        .map(|port| TasDeviceIdentity {
            port: port.to_owned(),
            device: "sg1000-standard-controller".to_owned(),
            configuration_sha256: TasDigest::from_bytes(SG1000_CONTROLLER_CONFIGURATION),
        })
        .collect()
}

fn current_state_rom_sha256(state: &[u8]) -> Result<[u8; 32]> {
    ensure!(
        state.len() >= 44 && state[..8] == zeff_sega8_core::save_state::SAVE_STATE_MAGIC,
        "TAS requires a native Sega 8-bit save-state"
    );
    let version = u32::from_le_bytes(state[8..12].try_into().expect("length checked"));
    ensure!(
        version == zeff_sega8_core::save_state::SAVE_STATE_FORMAT_VERSION,
        "TAS requires current native Sega 8-bit state"
    );
    Ok(state[12..44].try_into().expect("length checked"))
}

pub(crate) fn direct_sg1000_tas_identity(
    backend: &EmuBackend,
    source_bytes: &[u8],
    start_state: &[u8],
) -> Result<TasProjectIdentity> {
    let identity = sg1000_tas_identity(
        backend,
        TasDigest::from_bytes(source_bytes),
        direct_sg1000_tas_sync_config_sha256(),
        start_state,
    )?;
    ensure!(
        identity.source_media_sha256 == identity.effective_media_sha256,
        "direct SG-1000 loader changed media bytes"
    );
    Ok(identity)
}

pub(crate) fn zip_sg1000_tas_identity(
    backend: &EmuBackend,
    archive_sha256: [u8; 32],
    member_name: &str,
    start_state: &[u8],
) -> Result<TasProjectIdentity> {
    sg1000_tas_identity(
        backend,
        TasDigest(archive_sha256),
        zip_sg1000_tas_sync_config_sha256(member_name),
        start_state,
    )
}

fn sg1000_tas_identity(
    backend: &EmuBackend,
    source_media_sha256: TasDigest,
    sync_config_sha256: TasDigest,
    start_state: &[u8],
) -> Result<TasProjectIdentity> {
    validate_direct_sg1000_tas_runtime(backend, false)?;
    let metadata = backend.replay_metadata();
    let effective_media_sha256 = TasDigest(
        metadata
            .rom_sha256
            .context("SG-1000 backend omitted its effective media identity")?,
    );
    ensure!(
        backend.encode_state_bytes()?.as_slice() == start_state,
        "SG-1000 TAS start state differs from the loaded baseline"
    );
    ensure!(
        TasDigest(current_state_rom_sha256(start_state)?) == effective_media_sha256,
        "SG-1000 TAS start state identity differs from the loaded core"
    );
    Ok(TasProjectIdentity {
        system: metadata
            .system
            .context("SG-1000 backend omitted its system identity")?,
        core_family: metadata
            .core_family
            .context("SG-1000 backend omitted its core-family identity")?,
        determinism_abi: zeff_sega8_core::save_state::SG1000_TAS_DETERMINISM_ABI_ID.to_owned(),
        source_media_sha256,
        effective_media_sha256,
        patches: Vec::new(),
        firmware: Vec::new(),
        devices: direct_sg1000_tas_devices(),
        sync_config_sha256,
        persistent_state: TasExternalIdentity::Absent,
        rtc_state: TasExternalIdentity::Absent,
        sensor_state: TasExternalIdentity::Absent,
        cheats: TasExternalIdentity::Absent,
        state_format_compatibility_id:
            zeff_sega8_core::save_state::TAS_STATE_FORMAT_COMPATIBILITY_ID.to_owned(),
        start_state_sha256: TasDigest::from_bytes(start_state),
    })
}

pub(crate) fn validate_direct_sg1000_tas_project_identity(project: &TasProject) -> Result<()> {
    let identity = project.identity();
    ensure!(
        identity.system == ActiveSystem::Sg1000.code()
            && identity.core_family == format!("{:?}", zeff_emu_common::system::CoreFamily::Sega8),
        "TAS project does not identify the native SG-1000 core"
    );
    ensure!(
        identity.determinism_abi == zeff_sega8_core::save_state::SG1000_TAS_DETERMINISM_ABI_ID
            && identity.state_format_compatibility_id
                == zeff_sega8_core::save_state::TAS_STATE_FORMAT_COMPATIBILITY_ID,
        "TAS project uses an incompatible SG-1000 determinism or state format"
    );
    ensure!(
        ((identity.source_media_sha256 == identity.effective_media_sha256
            && identity.sync_config_sha256 == direct_sg1000_tas_sync_config_sha256())
            || (identity.source_media_sha256 != identity.effective_media_sha256
                && identity.sync_config_sha256 != direct_sg1000_tas_sync_config_sha256()))
            && identity.patches.is_empty()
            && identity.firmware.is_empty()
            && identity.devices == direct_sg1000_tas_devices(),
        "TAS project media, firmware, devices, or sync configuration is incompatible"
    );
    ensure!(
        identity.persistent_state == TasExternalIdentity::Absent
            && identity.rtc_state == TasExternalIdentity::Absent
            && identity.sensor_state == TasExternalIdentity::Absent
            && identity.cheats == TasExternalIdentity::Absent,
        "TAS project declares unsupported external state"
    );
    ensure!(
        TasDigest(current_state_rom_sha256(project.start_state())?)
            == identity.effective_media_sha256
            && TasDigest::from_bytes(project.start_state()) == identity.start_state_sha256,
        "SG-1000 start state identity differs from the project"
    );
    Ok(())
}

pub(crate) fn validate_direct_sg1000_tas_branch_scope(
    project: &TasProject,
    branch_id: &str,
) -> Result<()> {
    validate_direct_sg1000_tas_project_identity(project)?;
    ensure!(
        project.replay_start() == &Default::default(),
        "direct SG-1000 TAS execution does not support replay start metadata"
    );
    let branch = project
        .branch(branch_id)
        .with_context(|| format!("unknown TAS branch {branch_id:?}"))?;
    ensure!(
        branch.events().is_empty(),
        "direct SG-1000 TAS execution does not support replay events"
    );
    for span in branch.input_spans() {
        let input = span.input;
        ensure!(
            input.players[0].buttons & !0x03 == 0
                && input.players[0].dpad & !0x0F == 0
                && input.players[1].buttons & !0x03 == 0
                && input.players[1].dpad & !0x0F == 0
                && input.players[2..]
                    .iter()
                    .all(|player| *player == Default::default())
                && input.coleco == [crate::tas_project::TasColecoControllerInput::default(); 2]
                && input.zapper == Default::default()
                && input.tilt_x_bits == 0
                && input.tilt_y_bits == 0
                && matches!(input.camera, TasCameraInput::None),
            "direct SG-1000 TAS execution supports two standard digital pads only"
        );
    }
    Ok(())
}

pub(crate) fn validate_direct_sg1000_tas_project_witness(
    project: &TasProject,
    branch_id: &str,
    witness: super::TasProjectRuntimeWitness<'_>,
) -> Result<()> {
    validate_direct_sg1000_tas_branch_scope(project, branch_id)?;
    let identity = project.identity();
    ensure!(
        witness.source_media_sha256 == identity.source_media_sha256
            && witness.effective_media_sha256 == identity.effective_media_sha256
            && witness.determinism_abi == identity.determinism_abi
            && witness.state_format_compatibility_id == identity.state_format_compatibility_id
            && witness.sync_config_sha256 == identity.sync_config_sha256,
        "worker SG-1000 identity does not match the TAS project"
    );
    ensure!(
        TasDigest::from_bytes(witness.current_state_bytes) == witness.current_state_sha256
            && TasDigest(current_state_rom_sha256(witness.current_state_bytes)?)
                == identity.effective_media_sha256,
        "worker SG-1000 state identity does not match the TAS project"
    );
    Ok(())
}

pub(crate) fn validate_direct_sg1000_tas_runtime(
    backend: &EmuBackend,
    cheats_present: bool,
) -> Result<()> {
    validate_direct_sg1000_tas_execution_runtime(backend, cheats_present)?;
    let provenance = backend
        .sega8()
        .and_then(crate::emu_backend::Sega8Backend::sg1000_tas_load_provenance)
        .context("SG-1000 backend omitted load provenance")?;
    ensure!(
        provenance.current_controller_raw == [0xFF; 2],
        "direct SG-1000 TAS acquisition requires neutral controllers"
    );
    Ok(())
}

pub(crate) fn validate_direct_sg1000_tas_execution_runtime(
    backend: &EmuBackend,
    cheats_present: bool,
) -> Result<()> {
    ensure!(
        backend.system() == ActiveSystem::Sg1000,
        "TAS execution profile requires an SG-1000 backend"
    );
    let metadata = backend.replay_metadata();
    let expected_core_family = format!("{:?}", zeff_emu_common::system::CoreFamily::Sega8);
    ensure!(
        metadata.system.as_deref() == Some(ActiveSystem::Sg1000.code())
            && metadata.core_family.as_deref() == Some(expected_core_family.as_str()),
        "SG-1000 backend identity metadata is incompatible"
    );
    let effective_media_sha256 = metadata
        .rom_sha256
        .context("SG-1000 backend omitted its effective media identity")?;
    let sega8 = backend
        .sega8()
        .context("SG-1000 backend became unavailable")?;
    let provenance = sega8
        .sg1000_tas_load_provenance()
        .context("SG-1000 backend omitted load provenance")?;
    ensure!(
        (provenance.load.direct_sg_file
            || provenance.load.tas_source_media_sha256 != provenance.load.raw_source_media_sha256)
            && crate::emu_backend::sega8::mapper_kind_from_paths(
                backend.source_path(),
                backend.rom_path(),
            )
            .is_none()
            && provenance.load.raw_source_media_sha256 == effective_media_sha256
            && (1..=super::direct_sg1000_loader::MAX_DIRECT_SG1000_ROM_BYTES as usize)
                .contains(&provenance.load.raw_source_media_len),
        "SG-1000 TAS execution requires one bounded .sg or .sc cartridge with core-detected mapper"
    );
    ensure!(
        !provenance.load.any_mod_enabled && !provenance.load.any_mod_applied,
        "direct SG-1000 TAS execution requires mods to be disabled"
    );
    ensure!(
        provenance.load.persistent_load
            == crate::emu_backend::sega8::Sg1000TasPersistentLoadOutcome::Absent
            && backend.save_ram_kind() == SaveRamKind::None,
        "direct SG-1000 TAS execution requires absent persistent storage"
    );
    ensure!(
        provenance.load.initial_input.is_none(),
        "direct SG-1000 TAS execution requires neutral initial controllers"
    );
    ensure!(
        provenance.load.controller_model
            == crate::emu_backend::sega8::Sg1000TasControllerModel::TwoStandardPads,
        "direct SG-1000 TAS execution requires two standard controllers"
    );
    let default_rate = zeff_sega8_core::emulator::DEFAULT_SAMPLE_RATE;
    ensure!(
        provenance.load.configured_sample_rate.is_none()
            && provenance.load.initial_sample_rate == default_rate
            && provenance.current_sample_rate == default_rate,
        "direct SG-1000 TAS execution requires the default sample rate"
    );
    ensure!(
        !cheats_present && metadata.cheat_sha256.is_none(),
        "SG-1000 TAS execution enabled cheats"
    );
    ensure!(
        metadata.firmware.is_empty() && !sega8.emu.bus().has_boot_rom(),
        "direct SG-1000 TAS execution requires firmware to be absent"
    );
    validate_machine_facts(Sg1000MachineFacts {
        rom_matches: sega8.emu.rom_hash() == effective_media_sha256,
        mapper_kind: sega8.emu.bus().mapper().kind(),
        video_standard: sega8.emu.video_standard(),
        console_region: sega8.emu.console_region(),
        boot_rom_enabled: sega8.emu.bus().boot_rom_enabled(),
        type_b_ram_extension: sega8.emu.sg_type_b_ram_extension(),
    })?;
    ensure!(
        backend.media_slot_snapshot().is_none(),
        "direct SG-1000 TAS execution does not support removable media"
    );
    Ok(())
}

#[derive(Clone, Copy)]
struct Sg1000MachineFacts {
    rom_matches: bool,
    mapper_kind: zeff_sega8_core::hardware::cartridge::Sega8MapperKind,
    video_standard: zeff_sega8_core::hardware::timing::Sega8VideoStandard,
    console_region: zeff_sega8_core::hardware::region::Sega8Region,
    boot_rom_enabled: bool,
    type_b_ram_extension: bool,
}

fn validate_machine_facts(facts: Sg1000MachineFacts) -> Result<()> {
    ensure!(
        facts.rom_matches
            && facts.mapper_kind == zeff_sega8_core::hardware::cartridge::Sega8MapperKind::Sega
            && facts.video_standard == zeff_sega8_core::hardware::timing::Sega8VideoStandard::Ntsc
            && facts.console_region == zeff_sega8_core::hardware::region::Sega8Region::Japanese
            && !facts.boot_rom_enabled
            && !facts.type_b_ram_extension,
        "SG-1000 core media, mapper, hardware, or RAM-extension configuration is incompatible"
    );
    Ok(())
}

pub(crate) fn validate_direct_sg1000_tas_state(
    backend: &mut EmuBackend,
    state: &[u8],
) -> Result<zeff_sega8_core::save_state::CurrentNativeSg1000TasStateProjection> {
    validate_direct_sg1000_tas_execution_runtime(backend, false)?;
    let sega8 = match backend {
        EmuBackend::Sega8(sega8) => sega8,
        _ => anyhow::bail!("TAS state requires an SG-1000 backend"),
    };
    let inspection =
        zeff_sega8_core::save_state::inspect_current_native_sg1000_tas_state(&sega8.emu, state)?;
    validate_machine_facts(Sg1000MachineFacts {
        rom_matches: inspection.rom_sha256 == sega8.emu.rom_hash(),
        mapper_kind: inspection.mapper_kind,
        video_standard: inspection.video_standard,
        console_region: inspection.console_region,
        boot_rom_enabled: inspection.boot_rom_enabled,
        type_b_ram_extension: inspection.type_b_ram_extension,
    })?;
    ensure!(
        inspection.save_ram_kind == SaveRamKind::None,
        "SG-1000 TAS state declares unsupported storage"
    );
    let projection =
        zeff_sega8_core::save_state::validate_and_load_current_native_sg1000_tas_state(
            &mut sega8.emu,
            state,
        )?;
    ensure!(
        projection.framebuffer.len() == ActiveSystem::Sg1000.framebuffer_len()
            && projection.framebuffer.as_ref() == sega8.emu.framebuffer(),
        "SG-1000 TAS state did not restore its exact framebuffer"
    );
    validate_direct_sg1000_tas_execution_runtime(backend, false)?;
    Ok(projection)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts() -> Sg1000MachineFacts {
        Sg1000MachineFacts {
            rom_matches: true,
            mapper_kind: zeff_sega8_core::hardware::cartridge::Sega8MapperKind::Sega,
            video_standard: zeff_sega8_core::hardware::timing::Sega8VideoStandard::Ntsc,
            console_region: zeff_sega8_core::hardware::region::Sega8Region::Japanese,
            boot_rom_enabled: false,
            type_b_ram_extension: false,
        }
    }

    #[test]
    fn machine_facts_reject_type_b_and_nonstandard_mapper() {
        let mut type_b = facts();
        type_b.type_b_ram_extension = true;
        assert!(validate_machine_facts(type_b).is_err());
        let mut mapper = facts();
        mapper.mapper_kind = zeff_sega8_core::hardware::cartridge::Sega8MapperKind::Korean;
        assert!(validate_machine_facts(mapper).is_err());
    }
}
