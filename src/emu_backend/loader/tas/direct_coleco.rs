use anyhow::{Context, Result, ensure};
use zeff_emu_common::replay::ReplayFirmwareManifest;
use zeff_emu_common::save_ram::SaveRamKind;

use super::{ActiveSystem, EmuBackend};

use crate::tas_project::{
    TasCameraInput, TasDeviceIdentity, TasDigest, TasExternalIdentity, TasFirmwareIdentity,
    TasProject, TasProjectIdentity,
};

const COLECO_CONTROLLER_CONFIGURATION: &[u8] =
    b"zeff-tas-device-config-v1\0coleco-standard-controller-keypad\0";
const COLECO_SYNC_CONFIGURATION: &[u8] = b"zeff-tas-sync-config-v1\0coleco-direct-cartridge\0hardware=ntsc\0controllers=two-standard-keypad\0mods=disabled\0persistent-state=absent\0initial-input=neutral\0sample-rate=48000\0";
const COLECO_ZIP_SYNC_CONFIGURATION: &[u8] = b"zeff-tas-sync-config-v1\0coleco-zip-member\0hardware=ntsc\0controllers=two-standard-keypad\0mods=disabled\0persistent-state=absent\0initial-input=neutral\0sample-rate=48000\0member=";

pub(crate) fn direct_coleco_tas_sync_config_sha256() -> TasDigest {
    TasDigest::from_bytes(COLECO_SYNC_CONFIGURATION)
}

pub(crate) fn zip_coleco_tas_sync_config_sha256(member_name: &str) -> TasDigest {
    let mut bytes = Vec::with_capacity(COLECO_ZIP_SYNC_CONFIGURATION.len() + member_name.len());
    bytes.extend_from_slice(COLECO_ZIP_SYNC_CONFIGURATION);
    bytes.extend_from_slice(member_name.as_bytes());
    TasDigest::from_bytes(&bytes)
}

fn direct_coleco_tas_devices() -> Vec<TasDeviceIdentity> {
    ["p1", "p2"]
        .into_iter()
        .map(|port| TasDeviceIdentity {
            port: port.to_owned(),
            device: "coleco-standard-controller-keypad".to_owned(),
            configuration_sha256: TasDigest::from_bytes(COLECO_CONTROLLER_CONFIGURATION),
        })
        .collect()
}

fn direct_coleco_tas_firmware(backend: &EmuBackend) -> Result<Vec<TasFirmwareIdentity>> {
    let metadata = backend.replay_metadata();
    ensure!(
        matches!(
            metadata.firmware.as_slice(),
            [ReplayFirmwareManifest::External { firmware_id, .. }]
                if firmware_id == "coleco.vision.bios"
        ),
        "direct ColecoVision TAS requires one external retail BIOS"
    );
    Ok(metadata
        .firmware
        .iter()
        .map(super::tas_firmware_identity)
        .collect())
}

pub(crate) fn direct_coleco_tas_identity(
    backend: &EmuBackend,
    source_bytes: &[u8],
    start_state: &[u8],
) -> Result<TasProjectIdentity> {
    let identity = coleco_tas_identity(
        backend,
        TasDigest::from_bytes(source_bytes),
        direct_coleco_tas_sync_config_sha256(),
        start_state,
    )?;
    ensure!(
        identity.source_media_sha256 == identity.effective_media_sha256,
        "direct ColecoVision loader changed media bytes"
    );
    Ok(identity)
}

pub(crate) fn zip_coleco_tas_identity(
    backend: &EmuBackend,
    archive_sha256: [u8; 32],
    member_name: &str,
    start_state: &[u8],
) -> Result<TasProjectIdentity> {
    coleco_tas_identity(
        backend,
        TasDigest(archive_sha256),
        zip_coleco_tas_sync_config_sha256(member_name),
        start_state,
    )
}

fn coleco_tas_identity(
    backend: &EmuBackend,
    source_media_sha256: TasDigest,
    sync_config_sha256: TasDigest,
    start_state: &[u8],
) -> Result<TasProjectIdentity> {
    validate_direct_coleco_tas_runtime(backend, false)?;
    let metadata = backend.replay_metadata();
    let effective_media_sha256 = TasDigest(
        metadata
            .rom_sha256
            .context("ColecoVision backend omitted its effective media identity")?,
    );
    ensure!(
        backend.encode_state_bytes()?.as_slice() == start_state,
        "ColecoVision TAS start state differs from the loaded baseline"
    );
    let state_identity =
        zeff_coleco_core::save_state::inspect_current_native_tas_state_identity(start_state)?;
    let coleco = backend
        .coleco()
        .context("ColecoVision backend became unavailable")?;
    ensure!(
        state_identity.expansion_hardware == zeff_coleco_core::ExpansionHardware::Absent
            && state_identity.bios_sha256 == coleco.emu.bios_hash()
            && state_identity.cartridge_sha256 == effective_media_sha256.0,
        "ColecoVision TAS start state identity differs from the loaded core"
    );
    Ok(TasProjectIdentity {
        system: metadata
            .system
            .context("ColecoVision backend omitted its system identity")?,
        core_family: metadata
            .core_family
            .context("ColecoVision backend omitted its core-family identity")?,
        determinism_abi: zeff_coleco_core::save_state::TAS_DETERMINISM_ABI_ID.to_owned(),
        source_media_sha256,
        effective_media_sha256,
        patches: Vec::new(),
        firmware: direct_coleco_tas_firmware(backend)?,
        devices: direct_coleco_tas_devices(),
        sync_config_sha256,
        persistent_state: TasExternalIdentity::Absent,
        rtc_state: TasExternalIdentity::Absent,
        sensor_state: TasExternalIdentity::Absent,
        cheats: TasExternalIdentity::Absent,
        state_format_compatibility_id:
            zeff_coleco_core::save_state::TAS_STATE_FORMAT_COMPATIBILITY_ID.to_owned(),
        start_state_sha256: TasDigest::from_bytes(start_state),
    })
}

pub(crate) fn validate_direct_coleco_tas_project_identity(project: &TasProject) -> Result<()> {
    let identity = project.identity();
    ensure!(
        identity.system == ActiveSystem::Coleco.code()
            && identity.core_family
                == format!("{:?}", zeff_emu_common::system::CoreFamily::ColecoVision),
        "TAS project does not identify the native ColecoVision core"
    );
    ensure!(
        identity.determinism_abi == zeff_coleco_core::save_state::TAS_DETERMINISM_ABI_ID
            && identity.state_format_compatibility_id
                == zeff_coleco_core::save_state::TAS_STATE_FORMAT_COMPATIBILITY_ID,
        "TAS project uses an incompatible ColecoVision determinism or state format"
    );
    ensure!(
        ((identity.source_media_sha256 == identity.effective_media_sha256
            && identity.sync_config_sha256 == direct_coleco_tas_sync_config_sha256())
            || (identity.source_media_sha256 != identity.effective_media_sha256
                && identity.sync_config_sha256 != direct_coleco_tas_sync_config_sha256()))
            && identity.patches.is_empty()
            && identity.devices == direct_coleco_tas_devices(),
        "TAS project media, devices, or sync configuration is incompatible"
    );
    ensure!(
        matches!(
            identity.firmware.as_slice(),
            [TasFirmwareIdentity::External { firmware_id, sha256, .. }]
                if firmware_id == "coleco.vision.bios"
                    && *sha256
                        == TasDigest(
                            zeff_coleco_core::save_state::inspect_current_native_tas_state_identity(
                                project.start_state(),
                            )?
                            .bios_sha256,
                        )
        ),
        "TAS project firmware differs from its ColecoVision start state"
    );
    ensure!(
        identity.persistent_state == TasExternalIdentity::Absent
            && identity.rtc_state == TasExternalIdentity::Absent
            && identity.sensor_state == TasExternalIdentity::Absent
            && identity.cheats == TasExternalIdentity::Absent,
        "TAS project declares unsupported external state"
    );
    let state_identity = zeff_coleco_core::save_state::inspect_current_native_tas_state_identity(
        project.start_state(),
    )?;
    ensure!(
        state_identity.expansion_hardware == zeff_coleco_core::ExpansionHardware::Absent
            && TasDigest(state_identity.cartridge_sha256) == identity.effective_media_sha256
            && TasDigest::from_bytes(project.start_state()) == identity.start_state_sha256,
        "ColecoVision start state identity differs from the project"
    );
    Ok(())
}

pub(crate) fn validate_direct_coleco_tas_branch_scope(
    project: &TasProject,
    branch_id: &str,
) -> Result<()> {
    validate_direct_coleco_tas_project_identity(project)?;
    ensure!(
        project.replay_start() == &Default::default(),
        "direct ColecoVision TAS execution does not support replay start metadata"
    );
    let branch = project
        .branch(branch_id)
        .with_context(|| format!("unknown TAS branch {branch_id:?}"))?;
    ensure!(
        branch.events().is_empty(),
        "direct ColecoVision TAS execution does not support replay events"
    );
    for span in branch.input_spans() {
        let input = span.input;
        ensure!(
            input
                .players
                .iter()
                .all(|player| *player == Default::default())
                && input.zapper == Default::default()
                && input.tilt_x_bits == 0
                && input.tilt_y_bits == 0
                && matches!(input.camera, TasCameraInput::None),
            "direct ColecoVision TAS execution supports semantic standard controllers only"
        );
    }
    Ok(())
}

pub(crate) fn validate_direct_coleco_tas_project_witness(
    project: &TasProject,
    branch_id: &str,
    witness: super::TasProjectRuntimeWitness<'_>,
) -> Result<()> {
    validate_direct_coleco_tas_branch_scope(project, branch_id)?;
    let identity = project.identity();
    ensure!(
        witness.source_media_sha256 == identity.source_media_sha256
            && witness.effective_media_sha256 == identity.effective_media_sha256
            && witness.determinism_abi == identity.determinism_abi
            && witness.state_format_compatibility_id == identity.state_format_compatibility_id
            && witness.sync_config_sha256 == identity.sync_config_sha256,
        "worker ColecoVision identity does not match the TAS project"
    );
    ensure!(
        TasDigest::from_bytes(witness.current_state_bytes) == witness.current_state_sha256,
        "worker current-state witness digest is inconsistent"
    );
    let state_identity = zeff_coleco_core::save_state::inspect_current_native_tas_state_identity(
        witness.current_state_bytes,
    )?;
    ensure!(
        state_identity.expansion_hardware == zeff_coleco_core::ExpansionHardware::Absent
            && TasDigest(state_identity.cartridge_sha256) == identity.effective_media_sha256
            && matches!(
                identity.firmware.as_slice(),
                [TasFirmwareIdentity::External { sha256, .. }]
                    if *sha256 == TasDigest(state_identity.bios_sha256)
            ),
        "worker ColecoVision state identity does not match the TAS project"
    );
    Ok(())
}

pub(crate) fn validate_direct_coleco_tas_runtime(
    backend: &EmuBackend,
    cheats_present: bool,
) -> Result<()> {
    validate_direct_coleco_tas_execution_runtime(backend, cheats_present)?;
    let provenance = backend
        .coleco()
        .and_then(crate::emu_backend::ColecoBackend::tas_load_provenance)
        .context("ColecoVision backend omitted load provenance")?;
    ensure!(
        provenance.current_controllers == [zeff_coleco_core::StandardController::default(); 2],
        "direct ColecoVision TAS acquisition requires neutral standard controllers"
    );
    Ok(())
}

pub(crate) fn validate_direct_coleco_tas_execution_runtime(
    backend: &EmuBackend,
    cheats_present: bool,
) -> Result<()> {
    ensure!(
        backend.system() == ActiveSystem::Coleco,
        "TAS execution profile requires a ColecoVision backend"
    );
    let metadata = backend.replay_metadata();
    let expected_core_family = format!("{:?}", zeff_emu_common::system::CoreFamily::ColecoVision);
    ensure!(
        metadata.system.as_deref() == Some(ActiveSystem::Coleco.code())
            && metadata.core_family.as_deref() == Some(expected_core_family.as_str()),
        "ColecoVision backend identity metadata is incompatible"
    );
    let effective_media_sha256 = metadata
        .rom_sha256
        .context("ColecoVision backend omitted its effective media identity")?;
    let coleco = backend
        .coleco()
        .context("ColecoVision backend became unavailable")?;
    let provenance = coleco
        .tas_load_provenance()
        .context("ColecoVision backend omitted load provenance")?;
    ensure!(
        (provenance.load.direct_col_file
            || provenance.load.tas_source_media_sha256 != provenance.load.raw_source_media_sha256)
            && provenance.load.raw_source_media_sha256 == effective_media_sha256
            && provenance.load.raw_source_media_len > 1
            && provenance.load.raw_source_media_len
                <= zeff_coleco_core::constants::MAX_CARTRIDGE_SIZE,
        "ColecoVision TAS execution requires one bounded standard .col cartridge"
    );
    ensure!(
        !provenance.load.any_mod_enabled && !provenance.load.any_mod_applied,
        "direct ColecoVision TAS execution requires mods to be disabled"
    );
    ensure!(
        provenance.load.initial_input.is_none(),
        "direct ColecoVision TAS execution requires neutral initial controllers"
    );
    let default_rate = zeff_coleco_core::constants::DEFAULT_SAMPLE_RATE;
    ensure!(
        provenance.load.configured_sample_rate.is_none()
            && provenance.load.initial_sample_rate == default_rate
            && provenance.current_sample_rate == default_rate,
        "direct ColecoVision TAS execution requires the default sample rate"
    );
    ensure!(
        backend.save_ram_kind() == SaveRamKind::None,
        "direct ColecoVision TAS execution requires absent persistent state"
    );
    ensure!(
        !cheats_present && metadata.cheat_sha256.is_none(),
        "ColecoVision TAS execution enabled cheats"
    );
    ensure!(
        matches!(
            metadata.firmware.as_slice(),
            [ReplayFirmwareManifest::External {
                firmware_id,
                sha256,
                ..
            }] if firmware_id == "coleco.vision.bios" && *sha256 == coleco.emu.bios_hash()
        ),
        "direct ColecoVision TAS execution requires one exact BIOS identity"
    );
    ensure!(
        coleco.emu.expansion_hardware() == zeff_coleco_core::ExpansionHardware::Absent
            && coleco.emu.cartridge_hash() == effective_media_sha256,
        "ColecoVision core cartridge identity differs from the loaded media"
    );
    Ok(())
}

pub(crate) fn validate_direct_coleco_tas_state(
    backend: &mut EmuBackend,
    state: &[u8],
) -> Result<zeff_coleco_core::save_state::CurrentNativeTasStateProjection> {
    let coleco = match backend {
        EmuBackend::Coleco(coleco) => coleco,
        _ => anyhow::bail!("TAS state requires a ColecoVision backend"),
    };
    let projection = zeff_coleco_core::save_state::validate_and_load_current_native_tas_state(
        &mut coleco.emu,
        state,
    )?;
    ensure!(
        projection.framebuffer.len() == ActiveSystem::Coleco.framebuffer_len()
            && projection.framebuffer.as_ref() == coleco.emu.framebuffer(),
        "ColecoVision TAS state did not restore its exact framebuffer"
    );
    Ok(projection)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    use zeff_emu_common::replay::ReplayStartMetadata;

    use super::*;
    use crate::emu_backend::loader::{BackendLoadConfig, load_backend_from_bounded_direct_source};
    use crate::tas_project::{
        TasColecoControllerInput, TasColecoKeypadKey, TasInitialBranch, TasInputFrame, TasInputSpan,
    };

    static TEST_BIOS: [u8; zeff_coleco_core::constants::BIOS_SIZE] =
        [0; zeff_coleco_core::constants::BIOS_SIZE];

    fn direct_backend() -> EmuBackend {
        let rom = test_rom();
        load_backend_from_bounded_direct_source(
            ActiveSystem::Coleco,
            &PathBuf::from("game.col"),
            rom,
            BackendLoadConfig {
                coleco_bios_override: Some(&TEST_BIOS),
                ..BackendLoadConfig::default()
            },
        )
        .unwrap()
        .backend
    }

    fn test_rom() -> Vec<u8> {
        let mut rom = vec![0; 8 * 1024];
        rom[..2].copy_from_slice(&[0xAA, 0x55]);
        rom
    }

    #[test]
    fn direct_runtime_and_current_state_are_exactly_witnessed() {
        let mut backend = direct_backend();
        assert!(validate_direct_coleco_tas_runtime(&backend, false).is_ok());
        assert_eq!(
            backend.coleco().unwrap().emu.expansion_hardware(),
            zeff_coleco_core::ExpansionHardware::Absent
        );
        let state = backend.encode_state_bytes().unwrap();
        backend.step_frame();

        let projection = validate_direct_coleco_tas_state(&mut backend, &state).unwrap();

        assert_eq!(projection.replay_state_bytes, state);
        assert_eq!(projection.frame_count, 0);
        assert_eq!(backend.frame_count(), 0);
        assert_eq!(projection.framebuffer.as_ref(), backend.framebuffer());
    }

    #[test]
    fn runtime_rejects_host_configuration_and_firmware_deviations() {
        let mut sample_rate = direct_backend();
        sample_rate.set_sample_rate(44_100);
        assert!(validate_direct_coleco_tas_runtime(&sample_rate, false).is_err());

        let mut input = direct_backend();
        input.set_input(1, 0);
        assert!(validate_direct_coleco_tas_runtime(&input, false).is_err());

        let mut firmware = direct_backend();
        firmware.set_firmware_manifests(Vec::new());
        assert!(validate_direct_coleco_tas_runtime(&firmware, false).is_err());

        assert!(validate_direct_coleco_tas_runtime(&direct_backend(), true).is_err());
    }

    #[test]
    fn identity_and_scope_bind_semantic_keypad_input() {
        let backend = direct_backend();
        let state = backend.encode_state_bytes().unwrap();
        let identity = direct_coleco_tas_identity(&backend, &test_rom(), &state).unwrap();
        let project = TasProject::new(
            "coleco-test".to_owned(),
            identity,
            state,
            ReplayStartMetadata::default(),
            TasInitialBranch {
                id: "main".to_owned(),
                name: "Main".to_owned(),
                frame_count: 1,
                input_spans: vec![TasInputSpan {
                    start: 0,
                    length: 1,
                    input: TasInputFrame {
                        coleco: [
                            TasColecoControllerInput {
                                left_button: true,
                                keypad: TasColecoKeypadKey::Star,
                                ..TasColecoControllerInput::default()
                            },
                            TasColecoControllerInput {
                                right: true,
                                keypad: TasColecoKeypadKey::Nine,
                                ..TasColecoControllerInput::default()
                            },
                        ],
                        ..TasInputFrame::default()
                    },
                }],
                events: Vec::new(),
            },
            BTreeMap::new(),
        )
        .unwrap();

        validate_direct_coleco_tas_project_identity(&project).unwrap();
        validate_direct_coleco_tas_branch_scope(&project, "main").unwrap();
    }
}
