use anyhow::{Context, Result, ensure};
use zeff_emu_common::save_ram::SaveRamKind;
use zeff_ws_core::hardware::cartridge::{MinimumSystem, RomOrientation, SaveKind};

use super::{ActiveSystem, EmuBackend};
use crate::tas_project::{
    TasCameraInput, TasDeviceIdentity, TasDigest, TasExternalIdentity, TasProject,
    TasProjectIdentity,
};

const WS_HORIZONTAL_KEYPAD_CONFIGURATION: &[u8] =
    b"zeff-tas-device-config-v1\0ws-standard-keypad\0orientation=horizontal\0";
const WS_VERTICAL_KEYPAD_CONFIGURATION: &[u8] =
    b"zeff-tas-device-config-v1\0ws-standard-keypad\0orientation=vertical\0";
const WS_MONO_HORIZONTAL_SYNC_CONFIGURATION: &[u8] = b"zeff-tas-sync-config-v1\0ws-direct-cartridge\0system=ws\0orientation=horizontal\0internal-post-boot\0persistence=absent\0rtc=absent\0uart=disconnected\0mods=disabled\0initial-input=neutral\0sample-rate=48000\0firmware=absent\0";
const WS_MONO_VERTICAL_SYNC_CONFIGURATION: &[u8] = b"zeff-tas-sync-config-v1\0ws-direct-cartridge\0system=ws\0orientation=vertical\0internal-post-boot\0persistence=absent\0rtc=absent\0uart=disconnected\0mods=disabled\0initial-input=neutral\0sample-rate=48000\0firmware=absent\0";
const WS_COLOR_HORIZONTAL_SYNC_CONFIGURATION: &[u8] = b"zeff-tas-sync-config-v1\0ws-direct-cartridge\0system=wsc\0orientation=horizontal\0internal-post-boot\0persistence=absent\0rtc=absent\0uart=disconnected\0mods=disabled\0initial-input=neutral\0sample-rate=48000\0firmware=absent\0";
const WS_COLOR_VERTICAL_SYNC_CONFIGURATION: &[u8] = b"zeff-tas-sync-config-v1\0ws-direct-cartridge\0system=wsc\0orientation=vertical\0internal-post-boot\0persistence=absent\0rtc=absent\0uart=disconnected\0mods=disabled\0initial-input=neutral\0sample-rate=48000\0firmware=absent\0";
const WS_MONO_HORIZONTAL_BATTERY_SYNC_CONFIGURATION: &[u8] = b"zeff-tas-sync-config-v1\0ws-direct-cartridge\0system=ws\0orientation=horizontal\0internal-post-boot\0persistence=project-owned-cartridge-save\0rtc=absent\0uart=disconnected\0mods=disabled\0initial-input=neutral\0sample-rate=48000\0firmware=absent\0";
const WS_MONO_VERTICAL_BATTERY_SYNC_CONFIGURATION: &[u8] = b"zeff-tas-sync-config-v1\0ws-direct-cartridge\0system=ws\0orientation=vertical\0internal-post-boot\0persistence=project-owned-cartridge-save\0rtc=absent\0uart=disconnected\0mods=disabled\0initial-input=neutral\0sample-rate=48000\0firmware=absent\0";
const WS_COLOR_HORIZONTAL_BATTERY_SYNC_CONFIGURATION: &[u8] = b"zeff-tas-sync-config-v1\0ws-direct-cartridge\0system=wsc\0orientation=horizontal\0internal-post-boot\0persistence=project-owned-cartridge-save\0rtc=absent\0uart=disconnected\0mods=disabled\0initial-input=neutral\0sample-rate=48000\0firmware=absent\0";
const WS_COLOR_VERTICAL_BATTERY_SYNC_CONFIGURATION: &[u8] = b"zeff-tas-sync-config-v1\0ws-direct-cartridge\0system=wsc\0orientation=vertical\0internal-post-boot\0persistence=project-owned-cartridge-save\0rtc=absent\0uart=disconnected\0mods=disabled\0initial-input=neutral\0sample-rate=48000\0firmware=absent\0";
const WS_ZIP_SYNC_CONFIGURATION: &[u8] = b"zeff-tas-sync-config-v1\0ws-zip-member\0";
const WS_RTC_SYNC_CONFIGURATION: &[u8] = b"zeff-tas-sync-config-v1\0ws-direct-cartridge\0rtc=deterministic-cycle-clock\0epoch=2000-01-01T00:00:00\0";

pub(crate) fn direct_ws_tas_sync_config_sha256(
    system: MinimumSystem,
    orientation: RomOrientation,
) -> Result<TasDigest> {
    let bytes = match (system, orientation) {
        (MinimumSystem::WonderSwan, RomOrientation::Horizontal) => {
            WS_MONO_HORIZONTAL_SYNC_CONFIGURATION
        }
        (MinimumSystem::WonderSwan, RomOrientation::Vertical) => {
            WS_MONO_VERTICAL_SYNC_CONFIGURATION
        }
        (MinimumSystem::WonderSwanColor, RomOrientation::Horizontal) => {
            WS_COLOR_HORIZONTAL_SYNC_CONFIGURATION
        }
        (MinimumSystem::WonderSwanColor, RomOrientation::Vertical) => {
            WS_COLOR_VERTICAL_SYNC_CONFIGURATION
        }
        (MinimumSystem::Unknown(_), _) => anyhow::bail!("unsupported WonderSwan system"),
    };
    Ok(TasDigest::from_bytes(bytes))
}

pub(crate) fn direct_ws_battery_tas_sync_config_sha256(
    system: MinimumSystem,
    orientation: RomOrientation,
) -> Result<TasDigest> {
    let bytes = match (system, orientation) {
        (MinimumSystem::WonderSwan, RomOrientation::Horizontal) => {
            WS_MONO_HORIZONTAL_BATTERY_SYNC_CONFIGURATION
        }
        (MinimumSystem::WonderSwan, RomOrientation::Vertical) => {
            WS_MONO_VERTICAL_BATTERY_SYNC_CONFIGURATION
        }
        (MinimumSystem::WonderSwanColor, RomOrientation::Horizontal) => {
            WS_COLOR_HORIZONTAL_BATTERY_SYNC_CONFIGURATION
        }
        (MinimumSystem::WonderSwanColor, RomOrientation::Vertical) => {
            WS_COLOR_VERTICAL_BATTERY_SYNC_CONFIGURATION
        }
        (MinimumSystem::Unknown(_), _) => anyhow::bail!("unsupported WonderSwan system"),
    };
    Ok(TasDigest::from_bytes(bytes))
}

pub(crate) fn direct_ws_rtc_tas_sync_config_sha256(
    system: MinimumSystem,
    orientation: RomOrientation,
    save_kind: SaveKind,
) -> Result<TasDigest> {
    ensure!(
        save_kind == SaveKind::None || is_supported_direct_ws_save_kind(save_kind),
        "unsupported WonderSwan RTC persistence"
    );
    let mut bytes = Vec::from(WS_RTC_SYNC_CONFIGURATION);
    let system_bytes: &[u8] = match system {
        MinimumSystem::WonderSwan => b"system=ws\0",
        MinimumSystem::WonderSwanColor => b"system=wsc\0",
        MinimumSystem::Unknown(_) => anyhow::bail!("unsupported WonderSwan system"),
    };
    bytes.extend_from_slice(system_bytes);
    let orientation_bytes: &[u8] = match orientation {
        RomOrientation::Horizontal => b"orientation=horizontal\0",
        RomOrientation::Vertical => b"orientation=vertical\0",
    };
    bytes.extend_from_slice(orientation_bytes);
    bytes.extend_from_slice(b"save-kind=");
    bytes.push(ws_save_kind_byte(save_kind));
    bytes.extend_from_slice(b"\0uart=disconnected\0mods=disabled\0initial-input=neutral\0sample-rate=48000\0firmware=absent\0");
    Ok(TasDigest::from_bytes(&bytes))
}

fn ws_save_kind_byte(save_kind: SaveKind) -> u8 {
    match save_kind {
        SaveKind::None => 0x00,
        SaveKind::Sram32KId1 => 0x01,
        SaveKind::Sram32K => 0x02,
        SaveKind::Sram128K => 0x03,
        SaveKind::Sram256K => 0x04,
        SaveKind::Sram512K => 0x05,
        SaveKind::Eeprom128 => 0x10,
        SaveKind::Eeprom1K => 0x20,
        SaveKind::Eeprom2K => 0x50,
        SaveKind::Unknown(value) => value,
    }
}

pub(crate) fn zip_ws_tas_sync_config_sha256(
    system: MinimumSystem,
    orientation: RomOrientation,
    battery: bool,
    member_name: &str,
) -> Result<TasDigest> {
    let base = if battery {
        direct_ws_battery_tas_sync_config_sha256(system, orientation)?
    } else {
        direct_ws_tas_sync_config_sha256(system, orientation)?
    };
    let mut bytes = Vec::with_capacity(WS_ZIP_SYNC_CONFIGURATION.len() + 32 + member_name.len());
    bytes.extend_from_slice(WS_ZIP_SYNC_CONFIGURATION);
    bytes.extend_from_slice(&base.0);
    bytes.extend_from_slice(member_name.as_bytes());
    Ok(TasDigest::from_bytes(&bytes))
}

pub(crate) fn zip_ws_rtc_tas_sync_config_sha256(
    system: MinimumSystem,
    orientation: RomOrientation,
    save_kind: SaveKind,
    member_name: &str,
) -> Result<TasDigest> {
    let base = direct_ws_rtc_tas_sync_config_sha256(system, orientation, save_kind)?;
    let mut bytes = Vec::with_capacity(WS_ZIP_SYNC_CONFIGURATION.len() + 32 + member_name.len());
    bytes.extend_from_slice(WS_ZIP_SYNC_CONFIGURATION);
    bytes.extend_from_slice(&base.0);
    bytes.extend_from_slice(member_name.as_bytes());
    Ok(TasDigest::from_bytes(&bytes))
}

pub(crate) fn direct_ws_tas_orientation(project: &TasProject) -> Result<RomOrientation> {
    let [device] = project.identity().devices.as_slice() else {
        anyhow::bail!("direct WonderSwan TAS requires one keypad")
    };
    match device.device.as_str() {
        "ws-standard-keypad-horizontal" => Ok(RomOrientation::Horizontal),
        "ws-standard-keypad-vertical" => Ok(RomOrientation::Vertical),
        _ => anyhow::bail!("direct WonderSwan TAS declares an unsupported keypad"),
    }
}

fn direct_ws_tas_devices(orientation: RomOrientation) -> Vec<TasDeviceIdentity> {
    let (device, configuration) = match orientation {
        RomOrientation::Horizontal => (
            "ws-standard-keypad-horizontal",
            WS_HORIZONTAL_KEYPAD_CONFIGURATION,
        ),
        RomOrientation::Vertical => (
            "ws-standard-keypad-vertical",
            WS_VERTICAL_KEYPAD_CONFIGURATION,
        ),
    };
    vec![TasDeviceIdentity {
        port: "p1".to_owned(),
        device: device.to_owned(),
        configuration_sha256: TasDigest::from_bytes(configuration),
    }]
}

fn current_state_rom_sha256(state: &[u8]) -> Result<[u8; 32]> {
    ensure!(
        state.len() >= 41 && state[..8] == zeff_ws_core::save_state::SAVE_STATE_MAGIC,
        "TAS requires a native WonderSwan save state"
    );
    ensure!(
        state[8] == zeff_ws_core::save_state::SAVE_STATE_FORMAT_VERSION,
        "TAS requires current native WonderSwan state"
    );
    Ok(state[9..41].try_into().expect("length checked"))
}

pub(crate) fn direct_ws_tas_identity(
    backend: &EmuBackend,
    source_bytes: &[u8],
    start_state: &[u8],
) -> Result<TasProjectIdentity> {
    let inspection = validate_direct_ws_tas_private_runtime(backend, false)?;
    let identity = ws_tas_identity(
        backend,
        TasDigest::from_bytes(source_bytes),
        direct_ws_tas_sync_config_for_inspection(&inspection)?,
        start_state,
    )?;
    ensure!(
        identity.source_media_sha256 == identity.effective_media_sha256,
        "direct WonderSwan loader changed media bytes"
    );
    Ok(identity)
}

pub(crate) fn zip_ws_tas_identity(
    backend: &EmuBackend,
    archive_sha256: [u8; 32],
    member_name: &str,
    start_state: &[u8],
) -> Result<TasProjectIdentity> {
    let inspection = validate_direct_ws_tas_private_runtime(backend, false)?;
    let sync = zip_ws_tas_sync_config_sha256(
        inspection.minimum_system,
        inspection.orientation,
        inspection.save_kind != SaveKind::None,
        member_name,
    )?;
    let sync = if inspection.rtc_present {
        zip_ws_rtc_tas_sync_config_sha256(
            inspection.minimum_system,
            inspection.orientation,
            inspection.save_kind,
            member_name,
        )?
    } else {
        sync
    };
    ws_tas_identity(backend, TasDigest(archive_sha256), sync, start_state)
}

fn ws_tas_identity(
    backend: &EmuBackend,
    source_media_sha256: TasDigest,
    sync_config_sha256: TasDigest,
    start_state: &[u8],
) -> Result<TasProjectIdentity> {
    let inspection = validate_direct_ws_tas_private_runtime(backend, false)?;
    let metadata = backend.replay_metadata();
    let effective_media_sha256 = TasDigest(
        metadata
            .rom_sha256
            .context("WonderSwan backend omitted its effective media identity")?,
    );
    ensure!(
        backend.encode_state_bytes()?.as_slice() == start_state,
        "WonderSwan TAS start state differs from the loaded baseline"
    );
    ensure!(
        TasDigest(current_state_rom_sha256(start_state)?) == effective_media_sha256,
        "WonderSwan TAS start state identity differs from the loaded core"
    );
    let identity = TasProjectIdentity {
        system: metadata
            .system
            .context("WonderSwan backend omitted its system identity")?,
        core_family: metadata
            .core_family
            .context("WonderSwan backend omitted its core-family identity")?,
        determinism_abi: zeff_ws_core::save_state::TAS_DETERMINISM_ABI_ID.to_owned(),
        source_media_sha256,
        effective_media_sha256,
        patches: Vec::new(),
        firmware: Vec::new(),
        devices: direct_ws_tas_devices(inspection.orientation),
        sync_config_sha256,
        persistent_state: crate::emu_backend::ws::ws_tas_persistent_identity(&inspection)?,
        rtc_state: crate::emu_backend::ws::ws_tas_rtc_identity(&inspection),
        sensor_state: TasExternalIdentity::Absent,
        cheats: TasExternalIdentity::Absent,
        state_format_compatibility_id: zeff_ws_core::save_state::TAS_STATE_FORMAT_COMPATIBILITY_ID
            .to_owned(),
        start_state_sha256: TasDigest::from_bytes(start_state),
    };
    if inspection.rtc_present {
        let witness: crate::emu_backend::ws::WsRtcPersistenceWitness =
            crate::emu_backend::ws::ws_rtc_persistence_witness(backend)?;
        let complete = backend
            .ws()
            .and_then(crate::emu_backend::ws::WsBackend::tas_rtc_battery_bytes)
            .context("complete WonderSwan RTC persistence is unavailable")?;
        ensure!(
            witness.save_kind == inspection.save_kind
                && witness.persistent_state == identity.persistent_state
                && witness.rtc_state == identity.rtc_state
                && witness.complete_byte_len == complete.len() as u64
                && witness.complete_sha256 == TasDigest::from_bytes(&complete),
            "WonderSwan RTC persistence witness differs from the TAS identity"
        );
    }
    Ok(identity)
}

pub(crate) fn validate_direct_ws_tas_project_identity(project: &TasProject) -> Result<()> {
    let identity = project.identity();
    ensure!(
        identity.system == ActiveSystem::WonderSwan.code()
            && identity.core_family
                == format!("{:?}", zeff_emu_common::system::CoreFamily::WonderSwan),
        "TAS project does not identify the native WonderSwan core"
    );
    ensure!(
        identity.determinism_abi == zeff_ws_core::save_state::TAS_DETERMINISM_ABI_ID
            && identity.state_format_compatibility_id
                == zeff_ws_core::save_state::TAS_STATE_FORMAT_COMPATIBILITY_ID,
        "TAS project uses an incompatible WonderSwan determinism or state format"
    );
    let orientation = direct_ws_tas_orientation(project)?;
    let save_kinds: &[SaveKind] = match identity.persistent_state {
        TasExternalIdentity::Absent => &[SaveKind::None],
        TasExternalIdentity::ExternalSha256(_) => &[
            SaveKind::Sram32KId1,
            SaveKind::Sram32K,
            SaveKind::Sram128K,
            SaveKind::Sram256K,
            SaveKind::Sram512K,
            SaveKind::Eeprom128,
            SaveKind::Eeprom1K,
            SaveKind::Eeprom2K,
        ],
    };
    let direct_profile_matches = [MinimumSystem::WonderSwan, MinimumSystem::WonderSwanColor]
        .into_iter()
        .any(|system| {
            save_kinds.iter().copied().any(|save_kind| {
                let expected = if identity.rtc_state == TasExternalIdentity::Absent {
                    if save_kind == SaveKind::None {
                        direct_ws_tas_sync_config_sha256(system, orientation)
                    } else {
                        direct_ws_battery_tas_sync_config_sha256(system, orientation)
                    }
                } else {
                    direct_ws_rtc_tas_sync_config_sha256(system, orientation, save_kind)
                };
                expected.is_ok_and(|sync| sync == identity.sync_config_sha256)
            })
        });
    ensure!(
        ((identity.source_media_sha256 == identity.effective_media_sha256
            && direct_profile_matches)
            || (identity.source_media_sha256 != identity.effective_media_sha256
                && !direct_profile_matches))
            && identity.patches.is_empty()
            && identity.firmware.is_empty()
            && identity.devices == direct_ws_tas_devices(orientation),
        "TAS project media, firmware, devices, or sync configuration is incompatible"
    );
    ensure!(
        identity.sensor_state == TasExternalIdentity::Absent
            && identity.cheats == TasExternalIdentity::Absent,
        "TAS project declares unsupported external state"
    );
    ensure!(
        TasDigest(current_state_rom_sha256(project.start_state())?)
            == identity.effective_media_sha256
            && TasDigest::from_bytes(project.start_state()) == identity.start_state_sha256,
        "WonderSwan start state identity differs from the project"
    );
    Ok(())
}

pub(crate) fn validate_direct_ws_tas_branch_scope(
    project: &TasProject,
    branch_id: &str,
) -> Result<()> {
    validate_direct_ws_tas_project_identity(project)?;
    ensure!(
        project.replay_start() == &Default::default(),
        "direct WonderSwan TAS execution does not support replay start metadata"
    );
    let branch = project
        .branch(branch_id)
        .with_context(|| format!("unknown TAS branch {branch_id:?}"))?;
    ensure!(
        branch.events().is_empty(),
        "direct WonderSwan TAS execution does not support replay events"
    );
    for span in branch.input_spans() {
        let input = span.input;
        ensure!(
            input.players[0].buttons & !0xFB == 0
                && input.players[0].dpad & !0x0F == 0
                && input.players[1..]
                    .iter()
                    .all(|player| *player == Default::default())
                && input.coleco == [crate::tas_project::TasColecoControllerInput::default(); 2]
                && input.zapper == Default::default()
                && input.tilt_x_bits == 0
                && input.tilt_y_bits == 0
                && matches!(input.camera, TasCameraInput::None),
            "direct WonderSwan TAS execution supports one standard keypad only"
        );
    }
    Ok(())
}

pub(crate) fn validate_direct_ws_tas_project_witness(
    project: &TasProject,
    branch_id: &str,
    witness: super::TasProjectRuntimeWitness<'_>,
) -> Result<()> {
    validate_direct_ws_tas_branch_scope(project, branch_id)?;
    let identity = project.identity();
    ensure!(
        witness.source_media_sha256 == identity.source_media_sha256
            && witness.effective_media_sha256 == identity.effective_media_sha256
            && witness.determinism_abi == identity.determinism_abi
            && witness.state_format_compatibility_id == identity.state_format_compatibility_id
            && witness.sync_config_sha256 == identity.sync_config_sha256,
        "worker WonderSwan identity does not match the TAS project"
    );
    ensure!(
        TasDigest::from_bytes(witness.current_state_bytes) == witness.current_state_sha256
            && TasDigest(current_state_rom_sha256(witness.current_state_bytes)?)
                == identity.effective_media_sha256,
        "worker WonderSwan state identity does not match the TAS project"
    );
    Ok(())
}

pub(crate) fn validate_direct_ws_tas_runtime(
    backend: &EmuBackend,
    cheats_present: bool,
) -> Result<zeff_ws_core::save_state::CurrentNativeWonderSwanTasStateInspection> {
    let inspection = validate_direct_ws_tas_execution_runtime(backend, cheats_present)?;
    validate_direct_ws_tas_neutral_acquisition(backend, &inspection)?;
    Ok(inspection)
}

pub(crate) fn validate_direct_ws_tas_private_runtime(
    backend: &EmuBackend,
    cheats_present: bool,
) -> Result<zeff_ws_core::save_state::CurrentNativeWonderSwanTasStateInspection> {
    let inspection = validate_direct_ws_tas_execution_runtime_inner(backend, cheats_present, true)?;
    validate_direct_ws_tas_neutral_acquisition(backend, &inspection)?;
    Ok(inspection)
}

pub(crate) fn validate_direct_ws_tas_private_execution_runtime(
    backend: &EmuBackend,
    cheats_present: bool,
) -> Result<zeff_ws_core::save_state::CurrentNativeWonderSwanTasStateInspection> {
    validate_direct_ws_tas_execution_runtime_inner(backend, cheats_present, true)
}

fn validate_direct_ws_tas_neutral_acquisition(
    backend: &EmuBackend,
    inspection: &zeff_ws_core::save_state::CurrentNativeWonderSwanTasStateInspection,
) -> Result<()> {
    let provenance = backend
        .ws()
        .and_then(crate::emu_backend::WsBackend::tas_load_provenance)
        .context("WonderSwan backend omitted load provenance")?;
    ensure!(
        provenance.load.initial_input.is_none()
            && inspection.keypad.x_buttons == 0
            && inspection.keypad.y_buttons == 0
            && inspection.keypad.ab_start == 0,
        "direct WonderSwan TAS acquisition requires neutral keypad input"
    );
    ensure!(
        !inspection.rtc_present
            || (inspection.rtc.command == 0
                && inspection.rtc.payload == zeff_ws_core::save_state::TAS_RTC_EPOCH_BCD
                && inspection.rtc.payload_index == 0
                && inspection.rtc.payload_len == 0
                && inspection.rtc.ready_delay_reads == 0
                && !inspection.rtc.invalid_command
                && inspection.rtc.subsecond_cycles == 0),
        "direct WonderSwan TAS acquisition requires the deterministic RTC epoch"
    );
    Ok(())
}

pub(crate) fn validate_direct_ws_tas_execution_runtime(
    backend: &EmuBackend,
    cheats_present: bool,
) -> Result<zeff_ws_core::save_state::CurrentNativeWonderSwanTasStateInspection> {
    validate_direct_ws_tas_execution_runtime_inner(backend, cheats_present, false)
}

pub(crate) fn validate_direct_ws_tas_linked_runtime(
    backend: &EmuBackend,
    cheats_present: bool,
) -> Result<zeff_ws_core::save_state::CurrentNativeWonderSwanTasStateInspection> {
    validate_direct_ws_tas_private_execution_runtime(backend, cheats_present)
}

fn validate_direct_ws_tas_execution_runtime_inner(
    backend: &EmuBackend,
    cheats_present: bool,
    allow_project_storage: bool,
) -> Result<zeff_ws_core::save_state::CurrentNativeWonderSwanTasStateInspection> {
    ensure!(
        backend.system() == ActiveSystem::WonderSwan,
        "TAS execution profile requires a WonderSwan backend"
    );
    let metadata = backend.replay_metadata();
    let expected_core_family = format!("{:?}", zeff_emu_common::system::CoreFamily::WonderSwan);
    ensure!(
        metadata.system.as_deref() == Some(ActiveSystem::WonderSwan.code())
            && metadata.core_family.as_deref() == Some(expected_core_family.as_str()),
        "WonderSwan backend identity metadata is incompatible"
    );
    let effective_media_sha256 = metadata
        .rom_sha256
        .context("WonderSwan backend omitted its effective media identity")?;
    let ws = backend
        .ws()
        .context("WonderSwan backend became unavailable")?;
    let provenance = ws
        .tas_load_provenance()
        .context("WonderSwan backend omitted load provenance")?;
    ensure!(
        (provenance.load.direct_ws_file
            || provenance.load.tas_source_media_sha256 != provenance.load.raw_source_media_sha256)
            && provenance.load.raw_source_media_sha256 == effective_media_sha256
            && (1..=super::direct_ws_loader::MAX_DIRECT_WS_ROM_BYTES as usize)
                .contains(&provenance.load.raw_source_media_len),
        "WonderSwan TAS execution requires one bounded .ws or .wsc cartridge"
    );
    ensure!(
        !provenance.load.any_mod_enabled && !provenance.load.any_mod_applied,
        "direct WonderSwan TAS execution requires mods to be disabled"
    );
    let persistent_load_matches = if allow_project_storage {
        provenance.load.persistent_load
            != crate::emu_backend::ws::WsTasPersistentLoadOutcome::Unknown
    } else {
        provenance.load.persistent_load
            == crate::emu_backend::ws::WsTasPersistentLoadOutcome::Absent
            && backend.save_ram_kind() == SaveRamKind::None
    };
    ensure!(
        persistent_load_matches,
        "direct WonderSwan TAS execution requires owned or absent persistent storage"
    );
    let default_rate = zeff_ws_core::emulator::DEFAULT_SAMPLE_RATE;
    ensure!(
        provenance.load.configured_sample_rate.is_none()
            && provenance.load.initial_sample_rate == default_rate
            && provenance.current_sample_rate == default_rate,
        "direct WonderSwan TAS execution requires the default sample rate"
    );
    ensure!(
        !cheats_present && metadata.cheat_sha256.is_none(),
        "WonderSwan TAS execution enabled cheats"
    );
    ensure!(
        metadata.firmware.is_empty(),
        "direct WonderSwan TAS execution requires firmware to be absent"
    );
    ensure!(
        backend.media_slot_snapshot().is_none(),
        "direct WonderSwan TAS execution does not support removable media"
    );
    let state = backend.encode_state_bytes()?;
    let inspection =
        zeff_ws_core::save_state::inspect_current_native_wonder_swan_tas_state(&ws.emu, &state)?;
    validate_machine_facts(
        &inspection,
        provenance.load,
        provenance.current_orientation,
        effective_media_sha256,
        allow_project_storage,
    )?;
    Ok(inspection)
}

fn validate_machine_facts(
    inspection: &zeff_ws_core::save_state::CurrentNativeWonderSwanTasStateInspection,
    load: &crate::emu_backend::ws::WsTasLoadProvenance,
    current_orientation: RomOrientation,
    effective_media_sha256: [u8; 32],
    allow_project_storage: bool,
) -> Result<()> {
    let persistent_storage_matches = if allow_project_storage {
        match inspection.save_kind {
            SaveKind::None => {
                inspection.save_ram_kind == SaveRamKind::None && inspection.cartridge_save_len == 0
            }
            save_kind if is_supported_direct_ws_save_kind(save_kind) => {
                inspection.save_ram_kind == save_kind.save_ram_kind()
                    && inspection.cartridge_save_len == save_kind.size()
            }
            _ => false,
        }
    } else {
        inspection.save_kind == SaveKind::None
            && inspection.save_ram_kind == SaveRamKind::None
            && inspection.cartridge_save_len == 0
    };
    ensure!(
        inspection.rom_sha256 == effective_media_sha256
            && inspection.rom_len == load.raw_source_media_len
            && inspection.rom_footer.checksum_valid
            && inspection.rom_footer.rom_size.declared_bytes == Some(inspection.rom_len)
            && load.source_system == Some(inspection.minimum_system)
            && inspection.color_hardware
                == (inspection.minimum_system == MinimumSystem::WonderSwanColor)
            && inspection.orientation == inspection.rom_footer.orientation()
            && inspection.orientation == current_orientation
            && persistent_storage_matches
            && (allow_project_storage || !inspection.rtc_present)
            && inspection.uart.is_disconnected()
            && inspection.startup
                == zeff_ws_core::save_state::WonderSwanTasStartup::InternalPostBoot,
        "WonderSwan core media, system, orientation, persistence, RTC, or link state is incompatible"
    );
    Ok(())
}

pub(crate) fn validate_direct_ws_tas_state(
    backend: &mut EmuBackend,
    state: &[u8],
) -> Result<zeff_ws_core::save_state::CurrentNativeWonderSwanTasStateProjection> {
    validate_direct_ws_tas_execution_runtime(backend, false)?;
    let ws = match backend {
        EmuBackend::Ws(ws) => ws,
        _ => anyhow::bail!("TAS state requires a WonderSwan backend"),
    };
    let inspection =
        zeff_ws_core::save_state::inspect_current_native_wonder_swan_tas_state(&ws.emu, state)?;
    let provenance = ws
        .tas_load_provenance()
        .context("WonderSwan backend omitted load provenance")?;
    validate_machine_facts(
        &inspection,
        provenance.load,
        provenance.current_orientation,
        ws.emu.rom_hash(),
        false,
    )?;
    let projection =
        zeff_ws_core::save_state::validate_and_load_current_native_wonder_swan_tas_state(
            &mut ws.emu,
            state,
        )?;
    ensure!(
        projection.framebuffer.len() == ActiveSystem::WonderSwan.framebuffer_len()
            && projection.framebuffer.as_ref() == ws.emu.framebuffer(),
        "WonderSwan TAS state did not restore its exact framebuffer"
    );
    validate_direct_ws_tas_execution_runtime(backend, false)?;
    Ok(projection)
}

pub(crate) fn validate_direct_ws_tas_private_state(
    backend: &mut EmuBackend,
    state: &[u8],
) -> Result<zeff_ws_core::save_state::CurrentNativeWonderSwanTasStateProjection> {
    validate_direct_ws_tas_private_execution_runtime(backend, false)?;
    let ws = match backend {
        EmuBackend::Ws(ws) => ws,
        _ => anyhow::bail!("TAS state requires a WonderSwan backend"),
    };
    let inspection =
        zeff_ws_core::save_state::inspect_current_native_wonder_swan_tas_state(&ws.emu, state)?;
    let provenance = ws
        .tas_load_provenance()
        .context("WonderSwan backend omitted load provenance")?;
    validate_machine_facts(
        &inspection,
        provenance.load,
        provenance.current_orientation,
        ws.emu.rom_hash(),
        true,
    )?;
    let projection =
        zeff_ws_core::save_state::validate_and_load_current_native_wonder_swan_tas_state(
            &mut ws.emu,
            state,
        )?;
    ensure!(
        projection.framebuffer.len() == ActiveSystem::WonderSwan.framebuffer_len()
            && projection.framebuffer.as_ref() == ws.emu.framebuffer(),
        "WonderSwan TAS state did not restore its exact framebuffer"
    );
    validate_direct_ws_tas_private_execution_runtime(backend, false)?;
    Ok(projection)
}

pub(crate) fn direct_ws_tas_sync_config_for_inspection(
    inspection: &zeff_ws_core::save_state::CurrentNativeWonderSwanTasStateInspection,
) -> Result<TasDigest> {
    if inspection.rtc_present {
        direct_ws_rtc_tas_sync_config_sha256(
            inspection.minimum_system,
            inspection.orientation,
            inspection.save_kind,
        )
    } else if inspection.save_kind == SaveKind::None {
        direct_ws_tas_sync_config_sha256(inspection.minimum_system, inspection.orientation)
    } else {
        direct_ws_battery_tas_sync_config_sha256(inspection.minimum_system, inspection.orientation)
    }
}

pub(crate) fn is_supported_direct_ws_save_kind(save_kind: SaveKind) -> bool {
    matches!(
        save_kind,
        SaveKind::Sram32KId1
            | SaveKind::Sram32K
            | SaveKind::Sram128K
            | SaveKind::Sram256K
            | SaveKind::Sram512K
            | SaveKind::Eeprom128
            | SaveKind::Eeprom1K
            | SaveKind::Eeprom2K
    )
}
