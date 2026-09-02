use anyhow::{Context, Result, ensure};
use zeff_emu_common::save_ram::SaveRamKind;
use zeff_pce_core::hardware::{
    PadButtons, PceArcadeCardMode, PceConsoleWiring, PceControllerMode, PceHardwareTopology,
    PceHuCardBoard, PceMemoryBaseMode, PsgRevision,
};

use super::{ActiveSystem, EmuBackend};
use crate::tas_project::{
    TasCameraInput, TasDeviceIdentity, TasDigest, TasExternalIdentity, TasProject,
    TasProjectIdentity,
};
use zeff_emu_common::time::FrameLifecycle;

const PCE_TWO_BUTTON_CONFIGURATION: &[u8] =
    b"zeff-tas-device-config-v1\0pce-two-button-controller\0";
const PCE_SYNC_CONFIGURATION: &[u8] = b"zeff-tas-sync-config-v1\0pce-direct-hucard\0wiring=pc-engine\0topology=base\0board=plain\0controller=two-button\0memory-base=disconnected\0arcade-card=disabled\0cd=absent\0supergrafx=absent\0mods=disabled\0persistence=absent\0initial-input=neutral\0sample-rate=48000\0overscan=full\0palette=raw-rgb\0firmware=absent\0";
const PCE_SF2_SYNC_CONFIGURATION: &[u8] = b"zeff-tas-sync-config-v1\0pce-direct-hucard\0wiring=pc-engine\0topology=base\0board=sf2-ce\0controller=two-button\0memory-base=disconnected\0arcade-card=disabled\0cd=absent\0supergrafx=absent\0mods=disabled\0persistence=absent\0initial-input=neutral\0sample-rate=48000\0overscan=full\0palette=raw-rgb\0firmware=absent\0";
const PCE_POPULOUS_SYNC_CONFIGURATION: &[u8] = b"zeff-tas-sync-config-v1\0pce-direct-hucard\0wiring=pc-engine\0topology=base\0board=populous\0controller=two-button\0memory-base=disconnected\0arcade-card=disabled\0cd=absent\0supergrafx=absent\0mods=disabled\0mapper-ram=32768-native-state-owned\0host-persistence=disabled\0initial-input=neutral\0sample-rate=48000\0overscan=full\0palette=raw-rgb\0firmware=absent\0";
const PCE_SUPERGRAFX_SYNC_CONFIGURATION: &[u8] = b"zeff-tas-sync-config-v1\0pce-direct-hucard\0wiring=pc-engine\0topology=supergrafx\0board=plain\0controller=two-button\0memory-base=disconnected\0arcade-card=disabled\0cd=absent\0mods=disabled\0persistence=absent\0initial-input=neutral\0sample-rate=48000\0overscan=full\0palette=raw-rgb\0firmware=absent\0";
const PCE_ZIP_SYNC_CONFIGURATION: &[u8] = b"zeff-tas-sync-config-v1\0pce-zip-member\0wiring=pc-engine\0topology=base\0board=plain\0controller=two-button\0memory-base=disconnected\0arcade-card=disabled\0cd=absent\0supergrafx=absent\0mods=disabled\0persistence=absent\0initial-input=neutral\0sample-rate=48000\0overscan=full\0palette=raw-rgb\0firmware=absent\0member=";
const PCE_SF2_ZIP_SYNC_CONFIGURATION: &[u8] = b"zeff-tas-sync-config-v1\0pce-zip-member\0wiring=pc-engine\0topology=base\0board=sf2-ce\0controller=two-button\0memory-base=disconnected\0arcade-card=disabled\0cd=absent\0supergrafx=absent\0mods=disabled\0persistence=absent\0initial-input=neutral\0sample-rate=48000\0overscan=full\0palette=raw-rgb\0firmware=absent\0member=";
const PCE_POPULOUS_ZIP_SYNC_CONFIGURATION: &[u8] = b"zeff-tas-sync-config-v1\0pce-zip-member\0wiring=pc-engine\0topology=base\0board=populous\0controller=two-button\0memory-base=disconnected\0arcade-card=disabled\0cd=absent\0supergrafx=absent\0mods=disabled\0mapper-ram=32768-native-state-owned\0host-persistence=disabled\0initial-input=neutral\0sample-rate=48000\0overscan=full\0palette=raw-rgb\0firmware=absent\0member=";
const PCE_SUPERGRAFX_ZIP_SYNC_CONFIGURATION: &[u8] = b"zeff-tas-sync-config-v1\0pce-zip-member\0wiring=pc-engine\0topology=supergrafx\0board=plain\0controller=two-button\0memory-base=disconnected\0arcade-card=disabled\0cd=absent\0mods=disabled\0persistence=absent\0initial-input=neutral\0sample-rate=48000\0overscan=full\0palette=raw-rgb\0firmware=absent\0member=";
const PCE_SIX_BUTTON_SYNC_CONFIGURATION: &[u8] = b"zeff-tas-sync-config-v1\0pce-direct-hucard\0wiring=pc-engine\0topology=base\0board=plain\0controller=six-button\0memory-base=disconnected\0arcade-card=disabled\0cd=absent\0supergrafx=absent\0mods=disabled\0persistence=absent\0initial-input=neutral\0sample-rate=48000\0overscan=full\0palette=raw-rgb\0firmware=absent\0";
const PCE_SIX_BUTTON_ZIP_SYNC_CONFIGURATION: &[u8] = b"zeff-tas-sync-config-v1\0pce-zip-member\0wiring=pc-engine\0topology=base\0board=plain\0controller=six-button\0memory-base=disconnected\0arcade-card=disabled\0cd=absent\0supergrafx=absent\0mods=disabled\0persistence=absent\0initial-input=neutral\0sample-rate=48000\0overscan=full\0palette=raw-rgb\0firmware=absent\0member=";
const PCE_SIX_BUTTON_CONFIGURATION: &[u8] =
    b"zeff-tas-device-config-v1\0pce-six-button-controller\0";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PceTasHardwareProfile {
    pub(crate) board: PceHuCardBoard,
    pub(crate) topology: PceHardwareTopology,
    pub(crate) controller_mode: PceControllerMode,
}

pub(crate) fn direct_pce_tas_sync_config_sha256_for_board(board: PceHuCardBoard) -> TasDigest {
    TasDigest::from_bytes(match board {
        PceHuCardBoard::Plain => PCE_SYNC_CONFIGURATION,
        PceHuCardBoard::Sf2Ce => PCE_SF2_SYNC_CONFIGURATION,
        PceHuCardBoard::Populous => PCE_POPULOUS_SYNC_CONFIGURATION,
        _ => unreachable!("unsupported direct PC Engine TAS board"),
    })
}

pub(crate) fn direct_pce_tas_sync_config_sha256_for_profile(
    profile: PceTasHardwareProfile,
) -> TasDigest {
    if profile.controller_mode == PceControllerMode::SixButton {
        ensure_six_button_profile(profile);
        return TasDigest::from_bytes(PCE_SIX_BUTTON_SYNC_CONFIGURATION);
    }
    if profile.topology == PceHardwareTopology::SuperGrafx && profile.board == PceHuCardBoard::Plain
    {
        return TasDigest::from_bytes(PCE_SUPERGRAFX_SYNC_CONFIGURATION);
    }
    ensure_base_profile(profile);
    direct_pce_tas_sync_config_sha256_for_board(profile.board)
}

pub(crate) fn zip_pce_tas_sync_config_sha256_for_board(
    board: PceHuCardBoard,
    member_name: &str,
) -> TasDigest {
    let configuration = match board {
        PceHuCardBoard::Plain => PCE_ZIP_SYNC_CONFIGURATION,
        PceHuCardBoard::Sf2Ce => PCE_SF2_ZIP_SYNC_CONFIGURATION,
        PceHuCardBoard::Populous => PCE_POPULOUS_ZIP_SYNC_CONFIGURATION,
        _ => unreachable!("unsupported direct PC Engine TAS board"),
    };
    let mut bytes = Vec::with_capacity(configuration.len() + member_name.len());
    bytes.extend_from_slice(configuration);
    bytes.extend_from_slice(member_name.as_bytes());
    TasDigest::from_bytes(&bytes)
}

pub(crate) fn zip_pce_tas_sync_config_sha256_for_profile(
    profile: PceTasHardwareProfile,
    member_name: &str,
) -> TasDigest {
    if profile.controller_mode == PceControllerMode::SixButton {
        ensure_six_button_profile(profile);
        let mut bytes =
            Vec::with_capacity(PCE_SIX_BUTTON_ZIP_SYNC_CONFIGURATION.len() + member_name.len());
        bytes.extend_from_slice(PCE_SIX_BUTTON_ZIP_SYNC_CONFIGURATION);
        bytes.extend_from_slice(member_name.as_bytes());
        return TasDigest::from_bytes(&bytes);
    }
    if profile.topology == PceHardwareTopology::SuperGrafx && profile.board == PceHuCardBoard::Plain
    {
        let mut bytes =
            Vec::with_capacity(PCE_SUPERGRAFX_ZIP_SYNC_CONFIGURATION.len() + member_name.len());
        bytes.extend_from_slice(PCE_SUPERGRAFX_ZIP_SYNC_CONFIGURATION);
        bytes.extend_from_slice(member_name.as_bytes());
        return TasDigest::from_bytes(&bytes);
    }
    ensure_base_profile(profile);
    zip_pce_tas_sync_config_sha256_for_board(profile.board, member_name)
}

fn ensure_base_profile(profile: PceTasHardwareProfile) {
    assert!(
        profile.topology == PceHardwareTopology::Base
            && matches!(
                profile.board,
                PceHuCardBoard::Plain | PceHuCardBoard::Sf2Ce | PceHuCardBoard::Populous
            ),
        "unsupported direct PC Engine TAS hardware profile"
    );
}

fn ensure_six_button_profile(profile: PceTasHardwareProfile) {
    assert!(
        profile.board == PceHuCardBoard::Plain
            && profile.topology == PceHardwareTopology::Base
            && profile.controller_mode == PceControllerMode::SixButton,
        "unsupported direct PC Engine six-button TAS hardware profile"
    );
}

fn direct_pce_tas_devices(profile: PceTasHardwareProfile) -> Vec<TasDeviceIdentity> {
    vec![TasDeviceIdentity {
        port: "p1".to_owned(),
        device: match profile.controller_mode {
            PceControllerMode::TwoButton => "pce-two-button-controller".to_owned(),
            PceControllerMode::SixButton => "pce-six-button-controller".to_owned(),
            _ => unreachable!("unsupported direct PC Engine TAS controller"),
        },
        configuration_sha256: TasDigest::from_bytes(match profile.controller_mode {
            PceControllerMode::TwoButton => PCE_TWO_BUTTON_CONFIGURATION,
            PceControllerMode::SixButton => PCE_SIX_BUTTON_CONFIGURATION,
            _ => unreachable!("unsupported direct PC Engine TAS controller"),
        }),
    }]
}

pub(crate) fn direct_pce_tas_identity(
    backend: &EmuBackend,
    source_bytes: &[u8],
    start_state: &[u8],
) -> Result<TasProjectIdentity> {
    let identity = pce_tas_identity(
        backend,
        TasDigest::from_bytes(source_bytes),
        direct_pce_tas_sync_config_sha256_for_profile(pce_tas_profile(backend)?),
        start_state,
    )?;
    ensure!(
        identity.source_media_sha256
            == TasDigest(
                backend
                    .pce()
                    .and_then(crate::emu_backend::PceBackend::tas_load_provenance)
                    .context("PC Engine backend omitted load provenance")?
                    .load
                    .raw_source_media_sha256,
            ),
        "PC Engine source HuCard identity changed during loading"
    );
    Ok(identity)
}

pub(crate) fn zip_pce_tas_identity(
    backend: &EmuBackend,
    archive_sha256: [u8; 32],
    member_name: &str,
    start_state: &[u8],
) -> Result<TasProjectIdentity> {
    pce_tas_identity(
        backend,
        TasDigest(archive_sha256),
        zip_pce_tas_sync_config_sha256_for_profile(pce_tas_profile(backend)?, member_name),
        start_state,
    )
}

fn pce_tas_identity(
    backend: &EmuBackend,
    source_media_sha256: TasDigest,
    sync_config_sha256: TasDigest,
    start_state: &[u8],
) -> Result<TasProjectIdentity> {
    let profile = pce_tas_profile(backend)?;
    let inspection =
        validate_direct_pce_tas_execution_runtime_for_profile(backend, false, profile)?;
    let metadata = backend.replay_metadata();
    let effective_media_sha256 = TasDigest(
        metadata
            .rom_sha256
            .context("PC Engine backend omitted its effective HuCard identity")?,
    );
    ensure!(
        effective_media_sha256 == TasDigest(inspection.normalized_rom_sha256),
        "PC Engine normalized HuCard identity changed during loading"
    );
    ensure!(
        backend.encode_state_bytes()?.as_slice() == start_state,
        "PC Engine TAS start state differs from the loaded baseline"
    );
    let state_inspection = backend
        .pce()
        .context("PC Engine backend became unavailable")?
        .inspect_current_native_tas_state(start_state)?;
    ensure!(
        state_inspection.normalized_rom_sha256 == inspection.normalized_rom_sha256,
        "PC Engine TAS start state identity differs from the loaded core"
    );
    ensure!(
        state_inspection.board == inspection.board,
        "PC Engine TAS start state board differs from the loaded core"
    );
    Ok(TasProjectIdentity {
        system: metadata
            .system
            .context("PC Engine backend omitted its system identity")?,
        core_family: metadata
            .core_family
            .context("PC Engine backend omitted its core-family identity")?,
        determinism_abi: zeff_pce_core::hardware::save_state::tas::TAS_DETERMINISM_ABI_ID
            .to_owned(),
        source_media_sha256,
        effective_media_sha256,
        patches: Vec::new(),
        firmware: Vec::new(),
        devices: direct_pce_tas_devices(profile),
        sync_config_sha256,
        persistent_state: TasExternalIdentity::Absent,
        rtc_state: TasExternalIdentity::Absent,
        sensor_state: TasExternalIdentity::Absent,
        cheats: TasExternalIdentity::Absent,
        state_format_compatibility_id:
            zeff_pce_core::hardware::save_state::tas::TAS_STATE_FORMAT_COMPATIBILITY_ID.to_owned(),
        start_state_sha256: TasDigest::from_bytes(start_state),
    })
}

pub(crate) fn validate_direct_pce_tas_project_identity(project: &TasProject) -> Result<()> {
    let identity = project.identity();
    ensure!(
        identity.system == ActiveSystem::Pce.code()
            && identity.core_family
                == format!("{:?}", zeff_emu_common::system::CoreFamily::PcEngine),
        "TAS project does not identify the native PC Engine core"
    );
    ensure!(
        identity.determinism_abi
            == zeff_pce_core::hardware::save_state::tas::TAS_DETERMINISM_ABI_ID
            && identity.state_format_compatibility_id
                == zeff_pce_core::hardware::save_state::tas::TAS_STATE_FORMAT_COMPATIBILITY_ID,
        "TAS project uses an incompatible PC Engine determinism or state format"
    );
    let state = inspect_project_start_state(project)?;
    let profile = PceTasHardwareProfile {
        board: state.board,
        topology: state.topology,
        controller_mode: project_controller_mode(&identity.devices)?,
    };
    let direct_profile = [
        PceTasHardwareProfile {
            board: PceHuCardBoard::Plain,
            topology: PceHardwareTopology::Base,
            controller_mode: PceControllerMode::TwoButton,
        },
        PceTasHardwareProfile {
            board: PceHuCardBoard::Sf2Ce,
            topology: PceHardwareTopology::Base,
            controller_mode: PceControllerMode::TwoButton,
        },
        PceTasHardwareProfile {
            board: PceHuCardBoard::Populous,
            topology: PceHardwareTopology::Base,
            controller_mode: PceControllerMode::TwoButton,
        },
        PceTasHardwareProfile {
            board: PceHuCardBoard::Plain,
            topology: PceHardwareTopology::SuperGrafx,
            controller_mode: PceControllerMode::TwoButton,
        },
        PceTasHardwareProfile {
            board: PceHuCardBoard::Plain,
            topology: PceHardwareTopology::Base,
            controller_mode: PceControllerMode::SixButton,
        },
    ]
    .into_iter()
    .find(|profile| {
        identity.sync_config_sha256 == direct_pce_tas_sync_config_sha256_for_profile(*profile)
    });
    let direct_media = direct_profile.is_some();
    let zip_media =
        identity.source_media_sha256 != identity.effective_media_sha256 && !direct_media;
    ensure!(
        identity.patches.is_empty()
            && identity.firmware.is_empty()
            && identity.devices == direct_pce_tas_devices(profile)
            && (direct_media || zip_media)
            && direct_profile.is_none_or(|candidate| {
                candidate.board == profile.board
                    && candidate.topology == profile.topology
                    && candidate.controller_mode == profile.controller_mode
            }),
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
        TasDigest(state.normalized_rom_sha256) == identity.effective_media_sha256
            && TasDigest::from_bytes(project.start_state()) == identity.start_state_sha256,
        "PC Engine start state identity differs from the project"
    );
    Ok(())
}

#[cfg(test)]
pub(crate) fn direct_pce_tas_project_board(project: &TasProject) -> Result<PceHuCardBoard> {
    Ok(direct_pce_tas_project_profile(project)?.board)
}

pub(crate) fn direct_pce_tas_project_profile(
    project: &TasProject,
) -> Result<PceTasHardwareProfile> {
    validate_direct_pce_tas_project_identity(project)?;
    let state = inspect_project_start_state(project)?;
    Ok(PceTasHardwareProfile {
        board: state.board,
        topology: state.topology,
        controller_mode: project_controller_mode(&project.identity().devices)?,
    })
}

pub(crate) fn validate_direct_pce_tas_branch_scope(
    project: &TasProject,
    branch_id: &str,
) -> Result<()> {
    validate_direct_pce_tas_project_identity(project)?;
    ensure!(
        project.replay_start() == &Default::default(),
        "direct PC Engine TAS execution does not support replay start metadata"
    );
    let branch = project
        .branch(branch_id)
        .with_context(|| format!("unknown TAS branch {branch_id:?}"))?;
    let profile = direct_pce_tas_project_profile(project)?;
    ensure!(
        branch.events().is_empty(),
        "direct PC Engine TAS execution does not support replay events"
    );
    for span in branch.input_spans() {
        let input = span.input;
        ensure!(
            input.players[0].buttons
                & if profile.controller_mode == PceControllerMode::SixButton {
                    !0xFF
                } else {
                    !0x0F
                }
                == 0
                && input.players[0].dpad & !0x0F == 0
                && input.players[1..]
                    .iter()
                    .all(|player| *player == Default::default())
                && input.coleco == [crate::tas_project::TasColecoControllerInput::default(); 2]
                && input.zapper == Default::default()
                && input.tilt_x_bits == 0
                && input.tilt_y_bits == 0
                && matches!(input.camera, TasCameraInput::None),
            "direct PC Engine TAS execution supports one configured controller only"
        );
    }
    Ok(())
}

pub(crate) fn validate_direct_pce_tas_project_witness(
    project: &TasProject,
    branch_id: &str,
    witness: super::TasProjectRuntimeWitness<'_>,
) -> Result<()> {
    validate_direct_pce_tas_branch_scope(project, branch_id)?;
    let identity = project.identity();
    ensure!(
        witness.source_media_sha256 == identity.source_media_sha256
            && witness.effective_media_sha256 == identity.effective_media_sha256
            && witness.determinism_abi == identity.determinism_abi
            && witness.state_format_compatibility_id == identity.state_format_compatibility_id
            && witness.sync_config_sha256 == identity.sync_config_sha256,
        "worker PC Engine identity does not match the TAS project"
    );
    ensure!(
        TasDigest::from_bytes(witness.current_state_bytes) == witness.current_state_sha256,
        "worker current-state witness digest is inconsistent"
    );
    let state = inspect_backend_state_identity(witness.current_state_bytes)?;
    ensure!(
        TasDigest(state.normalized_rom_sha256) == identity.effective_media_sha256
            && state.board == inspect_project_start_state(project)?.board
            && state.topology == inspect_project_start_state(project)?.topology,
        "worker PC Engine state identity does not match the TAS project"
    );
    Ok(())
}

pub(crate) fn validate_direct_pce_tas_runtime(
    backend: &EmuBackend,
    cheats_present: bool,
) -> Result<zeff_pce_core::hardware::save_state::tas::CurrentNativePceTasStateInspection> {
    let inspection = validate_direct_pce_tas_execution_runtime_for_profile(
        backend,
        cheats_present,
        PceTasHardwareProfile {
            controller_mode: PceControllerMode::TwoButton,
            ..pce_tas_profile(backend)?
        },
    )?;
    ensure!(
        inspection.controller_buttons == PadButtons::empty(),
        "direct PC Engine TAS acquisition requires neutral controller input"
    );
    Ok(inspection)
}

pub(crate) fn validate_direct_pce_tas_execution_runtime(
    backend: &EmuBackend,
    cheats_present: bool,
) -> Result<zeff_pce_core::hardware::save_state::tas::CurrentNativePceTasStateInspection> {
    validate_direct_pce_tas_execution_runtime_for_profile(
        backend,
        cheats_present,
        PceTasHardwareProfile {
            controller_mode: PceControllerMode::TwoButton,
            ..pce_tas_profile(backend)?
        },
    )
}

pub(crate) fn validate_direct_pce_six_button_tas_runtime(
    backend: &EmuBackend,
    cheats_present: bool,
) -> Result<zeff_pce_core::hardware::save_state::tas::CurrentNativePceTasStateInspection> {
    let inspection = validate_direct_pce_six_button_tas_execution_runtime(backend, cheats_present)?;
    ensure!(
        inspection.controller_buttons == PadButtons::empty()
            && inspection.controller_extra_buttons.is_empty(),
        "direct PC Engine TAS acquisition requires neutral controller input"
    );
    Ok(inspection)
}

pub(crate) fn validate_direct_pce_six_button_tas_execution_runtime(
    backend: &EmuBackend,
    cheats_present: bool,
) -> Result<zeff_pce_core::hardware::save_state::tas::CurrentNativePceTasStateInspection> {
    validate_direct_pce_tas_execution_runtime_for_profile(
        backend,
        cheats_present,
        PceTasHardwareProfile {
            controller_mode: PceControllerMode::SixButton,
            ..pce_tas_profile(backend)?
        },
    )
}

pub(crate) fn validate_direct_pce_tas_execution_runtime_for_profile(
    backend: &EmuBackend,
    cheats_present: bool,
    expected_profile: PceTasHardwareProfile,
) -> Result<zeff_pce_core::hardware::save_state::tas::CurrentNativePceTasStateInspection> {
    ensure!(
        backend.system() == ActiveSystem::Pce,
        "TAS execution profile requires a PC Engine backend"
    );
    let metadata = backend.replay_metadata();
    let expected_core_family = format!("{:?}", zeff_emu_common::system::CoreFamily::PcEngine);
    ensure!(
        metadata.system.as_deref() == Some(ActiveSystem::Pce.code())
            && metadata.core_family.as_deref() == Some(expected_core_family.as_str()),
        "PC Engine backend identity metadata is incompatible"
    );
    let effective_media_sha256 = metadata
        .rom_sha256
        .context("PC Engine backend omitted its effective media identity")?;
    let pce = backend
        .pce()
        .context("PC Engine backend became unavailable")?;
    let provenance = pce
        .tas_load_provenance()
        .context("PC Engine backend omitted load provenance")?;
    ensure!(
        provenance.load.direct_pce_file
            && (1..=super::direct_pce_loader::MAX_DIRECT_PCE_HUCARD_BYTES as usize)
                .contains(&provenance.load.raw_source_media_len),
        "PC Engine TAS execution requires one directly loaded bounded .pce HuCard"
    );
    ensure!(
        !provenance.load.any_mod_enabled && !provenance.load.any_mod_applied,
        "direct PC Engine TAS execution requires mods to be disabled"
    );
    ensure!(
        direct_pce_tas_host_persistence_absent(backend),
        "direct PC Engine TAS execution requires persistence loading to be disabled"
    );
    ensure!(
        provenance.load.initial_input.is_none(),
        "direct PC Engine TAS execution requires neutral initial input"
    );
    ensure!(
        provenance.load.configured_sample_rate == Some(48_000)
            && provenance.load.initial_sample_rate == 48_000
            && provenance.current_sample_rate == 48_000,
        "direct PC Engine TAS execution requires an exact 48000 Hz sample rate"
    );
    ensure!(
        provenance.load.selected_wiring == Some(PceConsoleWiring::PcEngine)
            && provenance.load.effective_wiring == PceConsoleWiring::PcEngine
            && provenance.load.selected_board == Some(provenance.load.effective_board)
            && provenance.load.selected_hardware
                == Some(match provenance.load.effective_topology {
                    PceHardwareTopology::Base =>
                        zeff_pce_core::hardware::PceCartridgeHardware::Base,
                    PceHardwareTopology::SuperGrafx => {
                        zeff_pce_core::hardware::PceCartridgeHardware::SuperGrafx
                    }
                })
            && matches!(
                provenance.load.effective_board,
                PceHuCardBoard::Plain | PceHuCardBoard::Sf2Ce | PceHuCardBoard::Populous
            )
            && (provenance.load.effective_topology == PceHardwareTopology::Base
                || (provenance.load.effective_topology == PceHardwareTopology::SuperGrafx
                    && provenance.load.effective_board == PceHuCardBoard::Plain))
            && provenance.load.effective_board == expected_profile.board
            && provenance.load.effective_topology == expected_profile.topology
            && provenance.load.selected_controller_mode == expected_profile.controller_mode
            && provenance.load.effective_controller_mode == expected_profile.controller_mode
            && provenance.load.selected_memory_base_mode == PceMemoryBaseMode::Disabled
            && provenance.load.effective_memory_base_mode == PceMemoryBaseMode::Disabled
            && provenance.load.selected_arcade_card_mode == PceArcadeCardMode::Disabled
            && provenance.load.effective_arcade_card_mode == PceArcadeCardMode::Disabled,
        "PC Engine TAS execution requires explicit supported hardware and one configured controller"
    );
    ensure!(
        !cheats_present && metadata.cheat_sha256.is_none(),
        "PC Engine TAS execution enabled cheats"
    );
    ensure!(
        metadata.firmware.is_empty() && backend.media_slot_snapshot().is_none(),
        "direct PC Engine HuCard TAS requires firmware and removable media to be absent"
    );
    ensure!(
        pce.tas_frame_counters_match(),
        "PC Engine backend and core frame counters differ"
    );
    ensure!(
        pce.tas_output_policy_is_exact() && pce.tas_presented_frame_is_current(),
        "direct PC Engine TAS execution requires exact Full/Raw RGB output"
    );
    let state = backend.encode_state_bytes()?;
    let inspection = pce.inspect_current_native_tas_state(&state)?;
    ensure!(
        inspection.normalized_rom_sha256 == effective_media_sha256
            && inspection.board == provenance.load.effective_board
            && inspection.topology == provenance.load.effective_topology
            && inspection.wiring == PceConsoleWiring::PcEngine
            && inspection.psg_revision
                == match provenance.load.effective_topology {
                    PceHardwareTopology::Base => PsgRevision::HuC6280,
                    PceHardwareTopology::SuperGrafx => PsgRevision::HuC6280A,
                },
        "PC Engine core media or hardware identity is incompatible"
    );
    if expected_profile.controller_mode == PceControllerMode::SixButton {
        ensure_six_button_profile(expected_profile);
    }
    Ok(inspection)
}

fn pce_tas_profile(backend: &EmuBackend) -> Result<PceTasHardwareProfile> {
    let board = backend
        .pce()
        .context("PC Engine backend became unavailable")?
        .tas_load_provenance()
        .context("PC Engine backend omitted load provenance")?
        .load
        .effective_board;
    ensure!(
        matches!(
            board,
            PceHuCardBoard::Plain | PceHuCardBoard::Sf2Ce | PceHuCardBoard::Populous
        ),
        "PC Engine TAS execution requires a supported HuCard board"
    );
    let topology = backend
        .pce()
        .context("PC Engine backend became unavailable")?
        .hardware_topology();
    let controller_mode = backend
        .pce()
        .context("PC Engine backend became unavailable")?
        .tas_load_provenance()
        .context("PC Engine backend omitted load provenance")?
        .load
        .effective_controller_mode;
    ensure!(
        topology == PceHardwareTopology::Base
            || (topology == PceHardwareTopology::SuperGrafx && board == PceHuCardBoard::Plain),
        "PC Engine TAS execution requires a supported HuCard topology"
    );
    ensure!(
        controller_mode == PceControllerMode::TwoButton
            || (controller_mode == PceControllerMode::SixButton
                && board == PceHuCardBoard::Plain
                && topology == PceHardwareTopology::Base),
        "PC Engine TAS execution requires a supported controller topology"
    );
    Ok(PceTasHardwareProfile {
        board,
        topology,
        controller_mode,
    })
}

pub(crate) fn direct_pce_tas_host_persistence_absent(backend: &EmuBackend) -> bool {
    let Some(provenance) = backend
        .pce()
        .and_then(crate::emu_backend::PceBackend::tas_load_provenance)
    else {
        return false;
    };
    if provenance.load.persistent_load
        != crate::emu_backend::pce::PceTasPersistentLoadOutcome::Skipped
    {
        return false;
    }
    match provenance.load.effective_board {
        PceHuCardBoard::Plain | PceHuCardBoard::Sf2Ce => {
            backend.save_ram_kind() == SaveRamKind::None
        }
        PceHuCardBoard::Populous => {
            backend.save_ram_kind()
                == SaveRamKind::mapper_ram_unknown(zeff_pce_core::hardware::POPULOUS_HUCARD_RAM_LEN)
        }
        PceHuCardBoard::SystemCardV1V2 | PceHuCardBoard::SystemCardV3 => false,
    }
}

pub(crate) fn validate_direct_pce_tas_state(
    backend: &mut EmuBackend,
    state: &[u8],
) -> Result<crate::emu_backend::pce::PceTasStateProjection> {
    let profile = pce_tas_profile(backend)?;
    validate_direct_pce_tas_execution_runtime_for_profile(backend, false, profile)?;
    let pce = match backend {
        EmuBackend::Pce(pce) => pce,
        _ => anyhow::bail!("TAS state requires a PC Engine backend"),
    };
    let projection = pce.validate_and_load_current_native_tas_state(state)?;
    ensure!(
        projection.framebuffer.as_ref() == pce.tas_core_framebuffer()
            && pce.tas_presented_frame_is_current()
            && projection.frame_count == pce.frame_count()
            && pce.tas_frame_counters_match(),
        "PC Engine TAS state did not restore exact frame and output state"
    );
    validate_direct_pce_tas_execution_runtime_for_profile(backend, false, profile)?;
    Ok(projection)
}

fn inspect_project_start_state(
    project: &TasProject,
) -> Result<zeff_pce_core::hardware::save_state::tas::CurrentNativePceTasStateIdentity> {
    inspect_backend_state_identity(project.start_state())
}

fn inspect_backend_state_identity(
    state: &[u8],
) -> Result<zeff_pce_core::hardware::save_state::tas::CurrentNativePceTasStateIdentity> {
    crate::emu_backend::pce::inspect_pce_tas_state_identity(state)
}

fn project_controller_mode(devices: &[TasDeviceIdentity]) -> Result<PceControllerMode> {
    ensure!(
        devices.len() == 1 && devices[0].port == "p1",
        "TAS project has an invalid PC Engine controller topology"
    );
    match (devices[0].device.as_str(), devices[0].configuration_sha256) {
        ("pce-two-button-controller", digest)
            if digest == TasDigest::from_bytes(PCE_TWO_BUTTON_CONFIGURATION) =>
        {
            Ok(PceControllerMode::TwoButton)
        }
        ("pce-six-button-controller", digest)
            if digest == TasDigest::from_bytes(PCE_SIX_BUTTON_CONFIGURATION) =>
        {
            Ok(PceControllerMode::SixButton)
        }
        _ => anyhow::bail!("TAS project has an unsupported PC Engine controller"),
    }
}

#[cfg(test)]
mod tests;
