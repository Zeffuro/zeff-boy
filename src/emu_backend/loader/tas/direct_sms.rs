use anyhow::{Context, Result, ensure};
use zeff_emu_common::replay::ReplayFirmwareManifest;
use zeff_emu_common::save_ram::SaveRamKind;

use super::{ActiveSystem, EmuBackend};
use crate::tas_project::{
    TasCameraInput, TasDeviceIdentity, TasDigest, TasExternalIdentity, TasProject,
    TasProjectIdentity,
};

const SMS_CONTROLLER_CONFIGURATION: &[u8] = b"zeff-tas-device-config-v1\0sms-standard-controller\0";
const SMS_SYNC_CONFIGURATION: &[u8] = b"zeff-tas-sync-config-v1\0sms-direct-cartridge\0hardware=master-system-ntsc-export\0mapper=core-auto\0controllers=two-standard\0mods=disabled\0external-persistent-state=absent\0volatile-mapper-ram=allowed\0initial-input=neutral\0sample-rate=48000\0boot-rom=absent\0";
const SMS_ZIP_SYNC_CONFIGURATION: &[u8] = b"zeff-tas-sync-config-v1\0sms-zip-member\0hardware=master-system-ntsc-export\0mapper=core-auto\0controllers=two-standard\0mods=disabled\0external-persistent-state=absent\0volatile-mapper-ram=allowed\0initial-input=neutral\0sample-rate=48000\0boot-rom=absent\0member=";

pub(crate) fn direct_sms_tas_sync_config_sha256() -> TasDigest {
    TasDigest::from_bytes(SMS_SYNC_CONFIGURATION)
}

pub(crate) fn zip_sms_tas_sync_config_sha256(member_name: &str) -> TasDigest {
    let mut bytes = Vec::with_capacity(SMS_ZIP_SYNC_CONFIGURATION.len() + member_name.len());
    bytes.extend_from_slice(SMS_ZIP_SYNC_CONFIGURATION);
    bytes.extend_from_slice(member_name.as_bytes());
    TasDigest::from_bytes(&bytes)
}

fn direct_sms_tas_devices() -> Vec<TasDeviceIdentity> {
    ["p1", "p2"]
        .into_iter()
        .map(|port| TasDeviceIdentity {
            port: port.to_owned(),
            device: "sms-standard-controller".to_owned(),
            configuration_sha256: TasDigest::from_bytes(SMS_CONTROLLER_CONFIGURATION),
        })
        .collect()
}

fn allowed_save_ram_kind(kind: SaveRamKind) -> bool {
    matches!(kind, SaveRamKind::None | SaveRamKind::KnownVolatile { .. })
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

pub(crate) fn direct_sms_tas_identity(
    backend: &EmuBackend,
    source_bytes: &[u8],
    start_state: &[u8],
) -> Result<TasProjectIdentity> {
    let identity = sms_tas_identity(
        backend,
        TasDigest::from_bytes(source_bytes),
        direct_sms_tas_sync_config_sha256(),
        start_state,
    )?;
    ensure!(
        identity.source_media_sha256 == identity.effective_media_sha256,
        "direct Master System loader changed media bytes"
    );
    Ok(identity)
}

pub(crate) fn zip_sms_tas_identity(
    backend: &EmuBackend,
    archive_sha256: [u8; 32],
    member_name: &str,
    start_state: &[u8],
) -> Result<TasProjectIdentity> {
    sms_tas_identity(
        backend,
        TasDigest(archive_sha256),
        zip_sms_tas_sync_config_sha256(member_name),
        start_state,
    )
}

fn sms_tas_identity(
    backend: &EmuBackend,
    source_media_sha256: TasDigest,
    sync_config_sha256: TasDigest,
    start_state: &[u8],
) -> Result<TasProjectIdentity> {
    validate_direct_sms_tas_runtime(backend, false)?;
    let metadata = backend.replay_metadata();
    let effective_media_sha256 = TasDigest(
        metadata
            .rom_sha256
            .context("Master System backend omitted its effective media identity")?,
    );
    ensure!(
        backend.encode_state_bytes()?.as_slice() == start_state,
        "Master System TAS start state differs from the loaded baseline"
    );
    ensure!(
        TasDigest(current_state_rom_sha256(start_state)?) == effective_media_sha256,
        "Master System TAS start state identity differs from the loaded core"
    );
    Ok(TasProjectIdentity {
        system: metadata
            .system
            .context("Master System backend omitted its system identity")?,
        core_family: metadata
            .core_family
            .context("Master System backend omitted its core-family identity")?,
        determinism_abi: zeff_sega8_core::save_state::TAS_DETERMINISM_ABI_ID.to_owned(),
        source_media_sha256,
        effective_media_sha256,
        patches: Vec::new(),
        firmware: metadata
            .firmware
            .iter()
            .map(super::tas_firmware_identity)
            .collect(),
        devices: direct_sms_tas_devices(),
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

pub(crate) fn validate_direct_sms_tas_project_identity(project: &TasProject) -> Result<()> {
    let identity = project.identity();
    ensure!(
        identity.system == ActiveSystem::MasterSystem.code()
            && identity.core_family == format!("{:?}", zeff_emu_common::system::CoreFamily::Sega8),
        "TAS project does not identify the native Master System core"
    );
    ensure!(
        identity.determinism_abi == zeff_sega8_core::save_state::TAS_DETERMINISM_ABI_ID
            && identity.state_format_compatibility_id
                == zeff_sega8_core::save_state::TAS_STATE_FORMAT_COMPATIBILITY_ID,
        "TAS project uses an incompatible Master System determinism or state format"
    );
    ensure!(
        ((identity.source_media_sha256 == identity.effective_media_sha256
            && identity.sync_config_sha256 == direct_sms_tas_sync_config_sha256())
            || (identity.source_media_sha256 != identity.effective_media_sha256
                && identity.sync_config_sha256 != direct_sms_tas_sync_config_sha256()))
            && identity.patches.is_empty()
            && identity.devices == direct_sms_tas_devices(),
        "TAS project media, devices, or sync configuration is incompatible"
    );
    ensure!(
        identity.firmware.iter().all(|firmware| matches!(
            firmware,
            crate::tas_project::TasFirmwareIdentity::Skipped { firmware_id, .. }
                if firmware_id == "sega.sms.boot"
        )) && identity.firmware.len() == 1,
        "direct Master System TAS requires the boot ROM to be skipped"
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
        "Master System start state identity differs from the project"
    );
    Ok(())
}

pub(crate) fn validate_direct_sms_tas_branch_scope(
    project: &TasProject,
    branch_id: &str,
) -> Result<()> {
    validate_direct_sms_tas_project_identity(project)?;
    ensure!(
        project.replay_start() == &Default::default(),
        "direct Master System TAS execution does not support replay start metadata"
    );
    let branch = project
        .branch(branch_id)
        .with_context(|| format!("unknown TAS branch {branch_id:?}"))?;
    ensure!(
        branch.events().is_empty(),
        "direct Master System TAS execution does not support replay events"
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
            "direct Master System TAS execution supports two standard digital pads only"
        );
    }
    Ok(())
}

pub(crate) fn validate_direct_sms_tas_project_witness(
    project: &TasProject,
    branch_id: &str,
    witness: super::TasProjectRuntimeWitness<'_>,
) -> Result<()> {
    validate_direct_sms_tas_branch_scope(project, branch_id)?;
    let identity = project.identity();
    ensure!(
        witness.source_media_sha256 == identity.source_media_sha256
            && witness.effective_media_sha256 == identity.effective_media_sha256
            && witness.determinism_abi == identity.determinism_abi
            && witness.state_format_compatibility_id == identity.state_format_compatibility_id
            && witness.sync_config_sha256 == identity.sync_config_sha256,
        "worker Master System identity does not match the TAS project"
    );
    ensure!(
        TasDigest::from_bytes(witness.current_state_bytes) == witness.current_state_sha256,
        "worker current-state witness digest is inconsistent"
    );
    let state_rom_sha256 = current_state_rom_sha256(witness.current_state_bytes)?;
    ensure!(
        TasDigest(state_rom_sha256) == identity.effective_media_sha256,
        "worker Master System state identity does not match the TAS project"
    );
    Ok(())
}

pub(crate) fn validate_direct_sms_tas_runtime(
    backend: &EmuBackend,
    cheats_present: bool,
) -> Result<()> {
    validate_direct_sms_tas_execution_runtime(backend, cheats_present)?;
    let provenance = backend
        .sega8()
        .and_then(crate::emu_backend::Sega8Backend::sms_tas_load_provenance)
        .context("Master System backend omitted load provenance")?;
    ensure!(
        provenance.current_controller_raw == [0xFF; 2],
        "direct Master System TAS acquisition requires neutral controllers"
    );
    Ok(())
}

pub(crate) fn validate_direct_sms_tas_execution_runtime(
    backend: &EmuBackend,
    cheats_present: bool,
) -> Result<()> {
    ensure!(
        backend.system() == ActiveSystem::MasterSystem,
        "TAS execution profile requires a Master System backend"
    );
    let metadata = backend.replay_metadata();
    let expected_core_family = format!("{:?}", zeff_emu_common::system::CoreFamily::Sega8);
    ensure!(
        metadata.system.as_deref() == Some(ActiveSystem::MasterSystem.code())
            && metadata.core_family.as_deref() == Some(expected_core_family.as_str()),
        "Master System backend identity metadata is incompatible"
    );
    let effective_media_sha256 = metadata
        .rom_sha256
        .context("Master System backend omitted its effective media identity")?;
    let sega8 = backend
        .sega8()
        .context("Master System backend became unavailable")?;
    let provenance = sega8
        .sms_tas_load_provenance()
        .context("Master System backend omitted load provenance")?;
    ensure!(
        (provenance.load.direct_sms_file
            || provenance.load.tas_source_media_sha256 != provenance.load.raw_source_media_sha256)
            && crate::emu_backend::sega8::mapper_kind_from_paths(
                backend.source_path(),
                backend.rom_path(),
            )
            .is_none()
            && provenance.load.raw_source_media_sha256 == effective_media_sha256
            && (1..=super::direct_sms_loader::MAX_DIRECT_SMS_ROM_BYTES as usize)
                .contains(&provenance.load.raw_source_media_len),
        "Master System TAS execution requires one bounded .sms cartridge with core-detected mapper"
    );
    ensure!(
        !provenance.load.any_mod_enabled && !provenance.load.any_mod_applied,
        "direct Master System TAS execution requires mods to be disabled"
    );
    ensure!(
        provenance.load.initial_input.is_none(),
        "direct Master System TAS execution requires neutral initial controllers"
    );
    let default_rate = zeff_sega8_core::emulator::DEFAULT_SAMPLE_RATE;
    ensure!(
        provenance.load.configured_sample_rate.is_none()
            && provenance.load.initial_sample_rate == default_rate
            && provenance.current_sample_rate == default_rate,
        "direct Master System TAS execution requires the default sample rate"
    );
    ensure!(
        allowed_save_ram_kind(backend.save_ram_kind()),
        "direct Master System TAS execution requires absent or known-volatile mapper RAM"
    );
    ensure!(
        !cheats_present && metadata.cheat_sha256.is_none(),
        "Master System TAS execution enabled cheats"
    );
    ensure!(
        metadata.firmware.len() == 1
            && metadata.firmware.iter().all(|firmware| matches!(
                firmware,
                ReplayFirmwareManifest::Skipped { firmware_id, .. }
                    if firmware_id == "sega.sms.boot"
            ))
            && !sega8.emu.bus().has_boot_rom(),
        "direct Master System TAS execution requires the boot ROM to be absent"
    );
    ensure!(
        sega8.emu.rom_hash() == effective_media_sha256
            && sega8.emu.video_standard()
                == zeff_sega8_core::hardware::timing::Sega8VideoStandard::Ntsc
            && sega8.emu.console_region() == zeff_sega8_core::hardware::region::Sega8Region::Export,
        "Master System core media or hardware configuration is incompatible"
    );
    ensure!(
        backend.media_slot_snapshot().is_none(),
        "direct Master System TAS execution does not support removable media"
    );
    Ok(())
}

pub(crate) fn validate_direct_sms_tas_state(
    backend: &mut EmuBackend,
    state: &[u8],
) -> Result<zeff_sega8_core::save_state::CurrentNativeSmsTasStateProjection> {
    validate_direct_sms_tas_execution_runtime(backend, false)?;
    let sega8 = match backend {
        EmuBackend::Sega8(sega8) => sega8,
        _ => anyhow::bail!("TAS state requires a Master System backend"),
    };
    let inspection =
        zeff_sega8_core::save_state::inspect_current_native_sms_tas_state(&sega8.emu, state)?;
    ensure!(
        inspection.rom_sha256 == sega8.emu.rom_hash()
            && allowed_save_ram_kind(inspection.save_ram_kind)
            && inspection.video_standard
                == zeff_sega8_core::hardware::timing::Sega8VideoStandard::Ntsc
            && inspection.console_region == zeff_sega8_core::hardware::region::Sega8Region::Export
            && !inspection.boot_rom_enabled,
        "Master System TAS state is outside the direct execution profile"
    );
    let projection = zeff_sega8_core::save_state::validate_and_load_current_native_sms_tas_state(
        &mut sega8.emu,
        state,
    )?;
    ensure!(
        projection.framebuffer.len() == ActiveSystem::MasterSystem.framebuffer_len()
            && projection.framebuffer.as_ref() == sega8.emu.framebuffer(),
        "Master System TAS state did not restore its exact framebuffer"
    );
    validate_direct_sms_tas_execution_runtime(backend, false)?;
    Ok(projection)
}
