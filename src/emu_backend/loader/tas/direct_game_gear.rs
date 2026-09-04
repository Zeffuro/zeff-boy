use anyhow::{Context, Result, ensure};
use zeff_emu_common::save_ram::SaveRamKind;

use super::{ActiveSystem, EmuBackend};
use crate::tas_project::{
    TasCameraInput, TasDeviceIdentity, TasDigest, TasExternalIdentity, TasProject,
    TasProjectIdentity,
};

const CONTROLLER_CONFIGURATION: &[u8] =
    b"zeff-tas-device-config-v1\0game-gear-built-in-pad-start\0";
const CATALOG_SYNC_CONFIGURATION: &[u8] = b"zeff-tas-sync-config-v1\0game-gear-direct-cartridge\0hardware=game-gear-ntsc-export\0mapper=core-auto-sega-only\0board-catalog=exact-absent\0controller=built-in-pad-start\0mods=disabled\0external-state=absent\0initial-input=neutral\0sample-rate=48000\0boot-rom=absent\0link-peer=absent\0";
const CATALOG_BATTERY_SYNC_CONFIGURATION: &[u8] = b"zeff-tas-sync-config-v1\0game-gear-direct-cartridge\0hardware=game-gear-ntsc-export\0mapper=core-auto-sega-only\0board-catalog=exact-battery-backed-8kib\0controller=built-in-pad-start\0mods=disabled\0external-state=project-owned-sram\0initial-input=neutral\0sample-rate=48000\0boot-rom=absent\0link-peer=absent\0";
const CONFIRMED_NO_SAVE_SYNC_CONFIGURATION: &[u8] = b"zeff-tas-sync-config-v1\0game-gear-direct-cartridge\0hardware=game-gear-ntsc-export\0mapper=core-auto-sega-only\0board=user-confirmed-no-cartridge-save-memory\0controller=built-in-pad-start\0mods=disabled\0external-state=absent\0initial-input=neutral\0sample-rate=48000\0boot-rom=absent\0link-peer=absent\0";
const ZIP_SYNC_CONFIGURATION: &[u8] = b"zeff-tas-sync-config-v1\0game-gear-zip-member\0";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum DirectGameGearTasBoardChoice {
    CataloguedAbsent,
    CataloguedBattery8KiB,
    ConfirmedNoCartridgeSaveMemory,
}

pub(crate) fn direct_game_gear_tas_sync_config_sha256() -> TasDigest {
    direct_game_gear_tas_sync_config_sha256_for_board(
        DirectGameGearTasBoardChoice::CataloguedAbsent,
    )
}

pub(crate) fn direct_game_gear_tas_sync_config_sha256_for_board(
    board_choice: DirectGameGearTasBoardChoice,
) -> TasDigest {
    TasDigest::from_bytes(match board_choice {
        DirectGameGearTasBoardChoice::CataloguedAbsent => CATALOG_SYNC_CONFIGURATION,
        DirectGameGearTasBoardChoice::CataloguedBattery8KiB => CATALOG_BATTERY_SYNC_CONFIGURATION,
        DirectGameGearTasBoardChoice::ConfirmedNoCartridgeSaveMemory => {
            CONFIRMED_NO_SAVE_SYNC_CONFIGURATION
        }
    })
}

pub(crate) fn zip_game_gear_tas_sync_config_sha256(
    board_choice: DirectGameGearTasBoardChoice,
    member_name: &str,
) -> TasDigest {
    let board = direct_game_gear_tas_sync_config_sha256_for_board(board_choice);
    let mut bytes = Vec::with_capacity(ZIP_SYNC_CONFIGURATION.len() + 32 + member_name.len());
    bytes.extend_from_slice(ZIP_SYNC_CONFIGURATION);
    bytes.extend_from_slice(&board.0);
    bytes.extend_from_slice(member_name.as_bytes());
    TasDigest::from_bytes(&bytes)
}

pub(crate) fn zip_game_gear_tas_board_choice(
    identity: &TasProjectIdentity,
    member_name: &str,
) -> Result<DirectGameGearTasBoardChoice> {
    [
        DirectGameGearTasBoardChoice::CataloguedAbsent,
        DirectGameGearTasBoardChoice::CataloguedBattery8KiB,
        DirectGameGearTasBoardChoice::ConfirmedNoCartridgeSaveMemory,
    ]
    .into_iter()
    .find(|choice| {
        identity.sync_config_sha256 == zip_game_gear_tas_sync_config_sha256(*choice, member_name)
    })
    .context("Game Gear ZIP member has an unsupported board selection")
}

pub(crate) fn direct_game_gear_tas_board_choice(
    identity: &TasProjectIdentity,
) -> Result<DirectGameGearTasBoardChoice> {
    [
        DirectGameGearTasBoardChoice::CataloguedAbsent,
        DirectGameGearTasBoardChoice::CataloguedBattery8KiB,
        DirectGameGearTasBoardChoice::ConfirmedNoCartridgeSaveMemory,
    ]
    .into_iter()
    .find(|choice| {
        identity.sync_config_sha256 == direct_game_gear_tas_sync_config_sha256_for_board(*choice)
    })
    .context("Game Gear TAS project has an unsupported board selection")
}

fn devices() -> Vec<TasDeviceIdentity> {
    vec![TasDeviceIdentity {
        port: "p1".to_owned(),
        device: "game-gear-built-in-pad-start".to_owned(),
        configuration_sha256: TasDigest::from_bytes(CONTROLLER_CONFIGURATION),
    }]
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

pub(crate) fn direct_game_gear_tas_identity(
    backend: &EmuBackend,
    source_bytes: &[u8],
    start_state: &[u8],
    board_choice: DirectGameGearTasBoardChoice,
) -> Result<TasProjectIdentity> {
    let identity = game_gear_tas_identity(
        backend,
        TasDigest::from_bytes(source_bytes),
        direct_game_gear_tas_sync_config_sha256_for_board(board_choice),
        start_state,
        board_choice,
    )?;
    ensure!(
        identity.source_media_sha256 == identity.effective_media_sha256,
        "direct Game Gear loader changed media bytes"
    );
    Ok(identity)
}

pub(crate) fn zip_game_gear_tas_identity(
    backend: &EmuBackend,
    archive_sha256: [u8; 32],
    member_name: &str,
    start_state: &[u8],
    board_choice: DirectGameGearTasBoardChoice,
) -> Result<TasProjectIdentity> {
    game_gear_tas_identity(
        backend,
        TasDigest(archive_sha256),
        zip_game_gear_tas_sync_config_sha256(board_choice, member_name),
        start_state,
        board_choice,
    )
}

fn game_gear_tas_identity(
    backend: &EmuBackend,
    source_media_sha256: TasDigest,
    sync_config_sha256: TasDigest,
    start_state: &[u8],
    board_choice: DirectGameGearTasBoardChoice,
) -> Result<TasProjectIdentity> {
    validate_direct_game_gear_tas_private_runtime(backend, false)?;
    let metadata = backend.replay_metadata();
    let effective_media_sha256 = TasDigest(
        metadata
            .rom_sha256
            .context("Game Gear backend omitted its effective media identity")?,
    );
    ensure!(
        backend.encode_state_bytes()?.as_slice() == start_state,
        "Game Gear TAS start state differs from the loaded baseline"
    );
    ensure!(
        TasDigest(current_state_rom_sha256(start_state)?) == effective_media_sha256,
        "Game Gear TAS start state identity differs from the loaded core"
    );
    Ok(TasProjectIdentity {
        system: metadata
            .system
            .context("Game Gear backend omitted its system identity")?,
        core_family: metadata
            .core_family
            .context("Game Gear backend omitted its core-family identity")?,
        determinism_abi: zeff_sega8_core::save_state::GAME_GEAR_TAS_DETERMINISM_ABI_ID.to_owned(),
        source_media_sha256,
        effective_media_sha256,
        patches: Vec::new(),
        firmware: metadata
            .firmware
            .iter()
            .map(super::tas_firmware_identity)
            .collect(),
        devices: devices(),
        sync_config_sha256,
        persistent_state: game_gear_persistent_identity(backend, board_choice)?,
        rtc_state: TasExternalIdentity::Absent,
        sensor_state: TasExternalIdentity::Absent,
        cheats: TasExternalIdentity::Absent,
        state_format_compatibility_id:
            zeff_sega8_core::save_state::TAS_STATE_FORMAT_COMPATIBILITY_ID.to_owned(),
        start_state_sha256: TasDigest::from_bytes(start_state),
    })
}

pub(crate) fn validate_direct_game_gear_tas_project_identity(project: &TasProject) -> Result<()> {
    let identity = project.identity();
    ensure!(
        identity.system == ActiveSystem::GameGear.code()
            && identity.core_family == format!("{:?}", zeff_emu_common::system::CoreFamily::Sega8),
        "TAS project does not identify the native Game Gear core"
    );
    ensure!(
        identity.determinism_abi == zeff_sega8_core::save_state::GAME_GEAR_TAS_DETERMINISM_ABI_ID
            && identity.state_format_compatibility_id
                == zeff_sega8_core::save_state::TAS_STATE_FORMAT_COMPATIBILITY_ID,
        "TAS project uses an incompatible Game Gear determinism or state format"
    );
    let direct_media = identity.source_media_sha256 == identity.effective_media_sha256
        && direct_game_gear_tas_board_choice(identity).is_ok_and(|board| {
            matches!(
                (board, identity.persistent_state),
                (
                    DirectGameGearTasBoardChoice::CataloguedBattery8KiB,
                    TasExternalIdentity::ExternalSha256(_)
                ) | (
                    DirectGameGearTasBoardChoice::CataloguedAbsent
                        | DirectGameGearTasBoardChoice::ConfirmedNoCartridgeSaveMemory,
                    TasExternalIdentity::Absent
                )
            )
        });
    let zip_media = identity.source_media_sha256 != identity.effective_media_sha256
        && direct_game_gear_tas_board_choice(identity).is_err();
    ensure!(
        (direct_media || zip_media) && identity.patches.is_empty() && identity.devices == devices(),
        "TAS project media, devices, or sync configuration is incompatible"
    );
    ensure!(
        identity.rtc_state == TasExternalIdentity::Absent
            && identity.sensor_state == TasExternalIdentity::Absent
            && identity.cheats == TasExternalIdentity::Absent,
        "TAS project declares unsupported external state"
    );
    ensure!(
        TasDigest(current_state_rom_sha256(project.start_state())?)
            == identity.effective_media_sha256
            && TasDigest::from_bytes(project.start_state()) == identity.start_state_sha256,
        "Game Gear start state identity differs from the project"
    );
    Ok(())
}

pub(crate) fn validate_direct_game_gear_tas_branch_scope(
    project: &TasProject,
    branch_id: &str,
) -> Result<()> {
    validate_direct_game_gear_tas_project_identity(project)?;
    ensure!(
        project.replay_start() == &Default::default(),
        "direct Game Gear TAS execution does not support replay start metadata"
    );
    let branch = project
        .branch(branch_id)
        .with_context(|| format!("unknown TAS branch {branch_id:?}"))?;
    ensure!(
        branch.events().is_empty(),
        "direct Game Gear TAS execution does not support replay events"
    );
    for span in branch.input_spans() {
        let input = span.input;
        ensure!(
            input.players[0].buttons & !0x0B == 0
                && input.players[0].dpad & !0x0F == 0
                && input.players[1..]
                    .iter()
                    .all(|player| *player == Default::default())
                && input.coleco == [crate::tas_project::TasColecoControllerInput::default(); 2]
                && input.zapper == Default::default()
                && input.tilt_x_bits == 0
                && input.tilt_y_bits == 0
                && matches!(input.camera, TasCameraInput::None),
            "direct Game Gear TAS execution supports the built-in pad and Start only"
        );
    }
    Ok(())
}

pub(crate) fn validate_direct_game_gear_tas_project_witness(
    project: &TasProject,
    branch_id: &str,
    witness: super::TasProjectRuntimeWitness<'_>,
) -> Result<()> {
    validate_direct_game_gear_tas_branch_scope(project, branch_id)?;
    let identity = project.identity();
    ensure!(
        witness.source_media_sha256 == identity.source_media_sha256
            && witness.effective_media_sha256 == identity.effective_media_sha256
            && witness.determinism_abi == identity.determinism_abi
            && witness.state_format_compatibility_id == identity.state_format_compatibility_id
            && witness.sync_config_sha256 == identity.sync_config_sha256,
        "worker Game Gear identity does not match the TAS project"
    );
    ensure!(
        TasDigest::from_bytes(witness.current_state_bytes) == witness.current_state_sha256
            && TasDigest(current_state_rom_sha256(witness.current_state_bytes)?)
                == identity.effective_media_sha256,
        "worker Game Gear state identity does not match the TAS project"
    );
    Ok(())
}

pub(crate) fn validate_direct_game_gear_tas_runtime(
    backend: &EmuBackend,
    cheats_present: bool,
) -> Result<()> {
    crate::emu_backend::sega8::validate_direct_game_gear_tas_runtime(backend, cheats_present)
}

pub(crate) fn validate_direct_game_gear_tas_private_runtime(
    backend: &EmuBackend,
    cheats_present: bool,
) -> Result<()> {
    crate::emu_backend::sega8::validate_direct_game_gear_tas_private_runtime(
        backend,
        cheats_present,
    )
}

pub(crate) fn validate_direct_game_gear_tas_private_execution_runtime(
    backend: &EmuBackend,
    cheats_present: bool,
) -> Result<()> {
    crate::emu_backend::sega8::validate_direct_game_gear_tas_private_execution_runtime(
        backend,
        cheats_present,
    )
}

pub(crate) fn validate_direct_game_gear_tas_execution_runtime(
    backend: &EmuBackend,
    cheats_present: bool,
) -> Result<()> {
    crate::emu_backend::sega8::validate_direct_game_gear_tas_execution_runtime(
        backend,
        cheats_present,
    )
}

#[allow(dead_code)]
pub(crate) fn validate_direct_game_gear_tas_state(
    backend: &mut EmuBackend,
    state: &[u8],
) -> Result<zeff_sega8_core::save_state::CurrentNativeGameGearTasStateProjection> {
    validate_direct_game_gear_tas_state_inner(backend, state, false, false)
}

pub(crate) fn validate_direct_game_gear_tas_private_state(
    backend: &mut EmuBackend,
    state: &[u8],
) -> Result<zeff_sega8_core::save_state::CurrentNativeGameGearTasStateProjection> {
    validate_direct_game_gear_tas_state_inner(backend, state, true, true)
}

pub(crate) fn restore_direct_game_gear_tas_private_execution_state(
    backend: &mut EmuBackend,
    state: &[u8],
) -> Result<zeff_sega8_core::save_state::CurrentNativeGameGearTasStateProjection> {
    validate_direct_game_gear_tas_state_inner(backend, state, true, false)
}

fn validate_direct_game_gear_tas_state_inner(
    backend: &mut EmuBackend,
    state: &[u8],
    allow_project_storage: bool,
    require_neutral: bool,
) -> Result<zeff_sega8_core::save_state::CurrentNativeGameGearTasStateProjection> {
    if allow_project_storage && require_neutral {
        validate_direct_game_gear_tas_private_runtime(backend, false)?;
    } else if allow_project_storage {
        validate_direct_game_gear_tas_private_execution_runtime(backend, false)?;
    } else {
        validate_direct_game_gear_tas_execution_runtime(backend, false)?;
    }
    let sega8 = match backend {
        EmuBackend::Sega8(sega8) => sega8,
        _ => anyhow::bail!("TAS state requires a Game Gear backend"),
    };
    let inspection =
        zeff_sega8_core::save_state::inspect_current_native_game_gear_tas_state(&sega8.emu, state)?;
    ensure!(
        inspection.rom_sha256 == sega8.emu.rom_hash()
            && inspection.mapper_kind
                == zeff_sega8_core::hardware::cartridge::Sega8MapperKind::Sega
            && (inspection.save_ram_kind == SaveRamKind::None
                || (allow_project_storage
                    && inspection.save_ram_kind
                        == SaveRamKind::KnownBatteryBacked { size: 8 * 1024 }))
            && inspection.video_standard
                == zeff_sega8_core::hardware::timing::Sega8VideoStandard::Ntsc
            && inspection.console_region == zeff_sega8_core::hardware::region::Sega8Region::Export
            && !inspection.boot_rom_enabled
            && inspection.controller_raw[1] == 0xFF
            && inspection.controller_raw[0] | 0x3F == 0xFF
            && !inspection.serial.peer_present,
        "Game Gear TAS state is outside the direct execution profile"
    );
    let projection =
        zeff_sega8_core::save_state::validate_and_load_current_native_game_gear_tas_state(
            &mut sega8.emu,
            state,
        )?;
    ensure!(
        projection.framebuffer.len() == ActiveSystem::GameGear.framebuffer_len()
            && projection.framebuffer.as_ref() == sega8.emu.framebuffer(),
        "Game Gear TAS state did not restore its exact framebuffer"
    );
    if allow_project_storage && require_neutral {
        validate_direct_game_gear_tas_private_runtime(backend, false)?;
    } else if allow_project_storage {
        validate_direct_game_gear_tas_private_execution_runtime(backend, false)?;
    } else {
        validate_direct_game_gear_tas_execution_runtime(backend, false)?;
    }
    Ok(projection)
}

fn game_gear_persistent_identity(
    backend: &EmuBackend,
    board_choice: DirectGameGearTasBoardChoice,
) -> Result<TasExternalIdentity> {
    let sega8 = backend
        .sega8()
        .context("Game Gear backend became unavailable")?;
    match (board_choice, sega8.emu.dump_battery_sram()) {
        (DirectGameGearTasBoardChoice::CataloguedBattery8KiB, Some(bytes))
            if bytes.len() == 8 * 1024 =>
        {
            Ok(TasExternalIdentity::ExternalSha256(TasDigest::from_bytes(
                &bytes,
            )))
        }
        (
            DirectGameGearTasBoardChoice::CataloguedAbsent
            | DirectGameGearTasBoardChoice::ConfirmedNoCartridgeSaveMemory,
            None,
        ) => Ok(TasExternalIdentity::Absent),
        _ => anyhow::bail!("Game Gear board catalogue and persistent state disagree"),
    }
}
