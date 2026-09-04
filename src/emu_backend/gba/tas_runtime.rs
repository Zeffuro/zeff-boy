use anyhow::{Context, Result, ensure};
use zeff_emu_common::save_ram::SaveRamKind;
use zeff_gba_core::hardware::cartridge::{BackupKind, SensorKind};

use crate::emu_backend::{ActiveSystem, EmuBackend};
use crate::tas_project::{
    TasCameraInput, TasDeviceIdentity, TasDigest, TasExternalIdentity, TasFirmwareIdentity,
    TasProject, TasProjectIdentity,
};

use super::{GbaRtcPersistenceWitness, GbaTasPersistentLoadOutcome};

mod rtc;
mod tilt;

const CONTROLLER_CONFIGURATION: &[u8] = b"zeff-tas-device-config-v1\0gba-standard-keypad\0";
const SYNC_CONFIGURATION: &[u8] = b"zeff-tas-sync-config-v1\0gba-direct-cartridge\0startup=internal-post-boot\0persistence=absent\0rtc=absent\0link=absent\0sensors=absent\0mods=disabled\0initial-input=neutral\0sample-rate=48000\0firmware=hle:nintendo.gba.bios:zeff-gba-hle:1\0";
const BATTERY_SYNC_CONFIGURATION: &[u8] = b"zeff-tas-sync-config-v1\0gba-direct-cartridge\0startup=internal-post-boot\0persistence=project-owned-cartridge-save\0rtc=absent\0link=absent\0sensors=absent\0mods=disabled\0initial-input=neutral\0sample-rate=48000\0firmware=hle:nintendo.gba.bios:zeff-gba-hle:1\0";
const ZIP_SYNC_CONFIGURATION: &[u8] = b"zeff-tas-sync-config-v1\0gba-zip-member\0startup=internal-post-boot\0persistence=absent\0rtc=absent\0link=absent\0sensors=absent\0mods=disabled\0initial-input=neutral\0sample-rate=48000\0firmware=hle:nintendo.gba.bios:zeff-gba-hle:1\0member=";
const ZIP_BATTERY_SYNC_CONFIGURATION: &[u8] = b"zeff-tas-sync-config-v1\0gba-zip-member\0startup=internal-post-boot\0persistence=project-owned-cartridge-save\0rtc=absent\0link=absent\0sensors=absent\0mods=disabled\0initial-input=neutral\0sample-rate=48000\0firmware=hle:nintendo.gba.bios:zeff-gba-hle:1\0member=";

pub(crate) const DIRECT_GBA_SAMPLE_RATE: u32 = 48_000;
pub(crate) const MAX_DIRECT_GBA_ROM_BYTES: u64 = 32 * 1024 * 1024;

pub(crate) fn direct_gba_tas_sync_config_sha256() -> TasDigest {
    TasDigest::from_bytes(SYNC_CONFIGURATION)
}

pub(crate) fn direct_gba_battery_tas_sync_config_sha256() -> TasDigest {
    TasDigest::from_bytes(BATTERY_SYNC_CONFIGURATION)
}

pub(crate) fn zip_gba_tas_sync_config_sha256(member_name: &str) -> TasDigest {
    zip_gba_tas_sync_config_sha256_for_profile(member_name, false)
}

pub(crate) fn zip_gba_battery_tas_sync_config_sha256(member_name: &str) -> TasDigest {
    zip_gba_tas_sync_config_sha256_for_profile(member_name, true)
}

pub(crate) fn direct_gba_tilt_tas_sync_config_sha256() -> TasDigest {
    tilt::direct_sync_config()
}

pub(crate) fn zip_gba_tilt_tas_sync_config_sha256(member_name: &str) -> TasDigest {
    tilt::zip_sync_config(member_name)
}

pub(crate) fn direct_gba_rtc_tas_sync_config_sha256(backup_kind: BackupKind) -> TasDigest {
    rtc::direct_sync_config(backup_kind)
}

pub(crate) fn zip_gba_rtc_tas_sync_config_sha256(
    member_name: &str,
    backup_kind: BackupKind,
) -> TasDigest {
    rtc::zip_sync_config(member_name, backup_kind)
}

pub(crate) fn supported_gba_rtc_backup_kinds() -> [BackupKind; 5] {
    rtc::supported_backup_kinds()
}

pub(crate) fn gba_rtc_persistence_witness(
    backend: &EmuBackend,
) -> Result<GbaRtcPersistenceWitness> {
    let inspection = validate_direct_gba_tas_private_execution_runtime(backend, false)?;
    ensure!(inspection.rtc_present, "GBA RTC persistence is unavailable");
    let gba = backend.gba().context("GBA backend became unavailable")?;
    let backup_kind = validate_gba_backup_kind(&gba.emu)?;
    let rtc_bytes = gba
        .emu
        .dump_rtc_persistence_state()
        .context("GBA RTC persistence state is unavailable")?;
    let complete = gba
        .emu
        .dump_complete_rtc_persistence()
        .context("complete GBA RTC persistence is unavailable")?;
    ensure!(
        rtc_bytes.len() == 32 && complete.len() == backup_kind.size() + 40,
        "GBA RTC persistence layout is incompatible"
    );
    Ok(GbaRtcPersistenceWitness {
        backup_kind,
        persistent_state: gba_persistent_identity(&inspection)?,
        rtc_state: rtc::identity(Some(&rtc_bytes)),
        complete_byte_len: complete.len() as u64,
        complete_sha256: TasDigest::from_bytes(&complete),
    })
}

fn zip_gba_tas_sync_config_sha256_for_profile(member_name: &str, battery: bool) -> TasDigest {
    let configuration = if battery {
        ZIP_BATTERY_SYNC_CONFIGURATION
    } else {
        ZIP_SYNC_CONFIGURATION
    };
    let mut bytes = Vec::with_capacity(ZIP_SYNC_CONFIGURATION.len() + member_name.len());
    bytes.extend_from_slice(configuration);
    bytes.extend_from_slice(member_name.as_bytes());
    TasDigest::from_bytes(&bytes)
}

fn devices(sensor_kind: SensorKind) -> Vec<TasDeviceIdentity> {
    let mut devices = vec![TasDeviceIdentity {
        port: "p1".to_owned(),
        device: "gba-standard-keypad".to_owned(),
        configuration_sha256: TasDigest::from_bytes(CONTROLLER_CONFIGURATION),
    }];
    if sensor_kind == SensorKind::Tilt {
        devices.push(tilt::device());
    }
    devices.sort_by(|left, right| left.port.cmp(&right.port));
    devices
}

fn firmware() -> Vec<TasFirmwareIdentity> {
    vec![TasFirmwareIdentity::Hle {
        firmware_id: "nintendo.gba.bios".to_owned(),
        implementation: "zeff-gba-hle".to_owned(),
        compatibility_version: 1,
    }]
}

fn current_state_rom_sha256(state: &[u8], tilt: bool) -> Result<[u8; 32]> {
    ensure!(
        state.len() >= 44 && state[..8] == *b"ZBGBAST\0",
        "TAS requires a native GBA save state"
    );
    ensure!(
        u32::from_le_bytes(state[8..12].try_into().expect("length checked"))
            == if tilt { 11 } else { 10 },
        "TAS requires current native GBA state"
    );
    Ok(state[state.len() - 32..]
        .try_into()
        .expect("length checked"))
}

pub(crate) fn direct_gba_tas_identity(
    backend: &EmuBackend,
    source_bytes: &[u8],
    start_state: &[u8],
) -> Result<TasProjectIdentity> {
    let sync_config_sha256 = gba_tas_sync_config(backend, None)?;
    let identity = gba_tas_identity(
        backend,
        TasDigest::from_bytes(source_bytes),
        sync_config_sha256,
        start_state,
    )?;
    ensure!(
        identity.source_media_sha256 == identity.effective_media_sha256,
        "direct GBA loader changed media bytes"
    );
    Ok(identity)
}

pub(crate) fn zip_gba_tas_identity(
    backend: &EmuBackend,
    archive_sha256: [u8; 32],
    member_name: &str,
    start_state: &[u8],
) -> Result<TasProjectIdentity> {
    let sync_config_sha256 = gba_tas_sync_config(backend, Some(member_name))?;
    gba_tas_identity(
        backend,
        TasDigest(archive_sha256),
        sync_config_sha256,
        start_state,
    )
}

fn gba_tas_identity(
    backend: &EmuBackend,
    source_media_sha256: TasDigest,
    sync_config_sha256: TasDigest,
    start_state: &[u8],
) -> Result<TasProjectIdentity> {
    let inspection = validate_direct_gba_tas_private_runtime(backend, false)?;
    let tilt_profile = inspection.sensor_kind == SensorKind::Tilt;
    let metadata = backend.replay_metadata();
    let effective_media_sha256 = TasDigest(
        metadata
            .rom_sha256
            .context("GBA backend omitted its effective media identity")?,
    );
    ensure!(
        backend.encode_state_bytes()?.as_slice() == start_state,
        "GBA TAS start state differs from the loaded baseline"
    );
    ensure!(
        TasDigest(current_state_rom_sha256(start_state, tilt_profile)?) == effective_media_sha256,
        "GBA TAS start state identity differs from the loaded core"
    );
    let rtc_state = backend
        .gba()
        .and_then(|gba| gba.emu.dump_rtc_persistence_state());
    Ok(TasProjectIdentity {
        system: metadata
            .system
            .context("GBA backend omitted its system identity")?,
        core_family: metadata
            .core_family
            .context("GBA backend omitted its core-family identity")?,
        determinism_abi: if tilt_profile {
            zeff_gba_core::save_state::TILT_TAS_DETERMINISM_ABI_ID
        } else {
            zeff_gba_core::save_state::TAS_DETERMINISM_ABI_ID
        }
        .to_owned(),
        source_media_sha256,
        effective_media_sha256,
        patches: Vec::new(),
        firmware: firmware(),
        devices: devices(inspection.sensor_kind),
        sync_config_sha256,
        persistent_state: gba_persistent_identity(&inspection)?,
        rtc_state: rtc::identity(rtc_state.as_deref()),
        sensor_state: tilt::identity(inspection.tilt_state),
        cheats: TasExternalIdentity::Absent,
        state_format_compatibility_id: if tilt_profile {
            zeff_gba_core::save_state::TILT_TAS_STATE_FORMAT_COMPATIBILITY_ID
        } else {
            zeff_gba_core::save_state::TAS_STATE_FORMAT_COMPATIBILITY_ID
        }
        .to_owned(),
        start_state_sha256: TasDigest::from_bytes(start_state),
    })
}

pub(crate) fn validate_direct_gba_tas_project_identity(project: &TasProject) -> Result<()> {
    let identity = project.identity();
    ensure!(
        identity.system == ActiveSystem::GameBoyAdvance.code()
            && identity.core_family
                == format!("{:?}", zeff_emu_common::system::CoreFamily::GameBoyAdvance),
        "TAS project does not identify the native GBA core"
    );
    let tilt_profile = is_gba_tilt_tas_identity(identity);
    ensure!(
        tilt_profile
            || (identity.determinism_abi == zeff_gba_core::save_state::TAS_DETERMINISM_ABI_ID
                && identity.state_format_compatibility_id
                    == zeff_gba_core::save_state::TAS_STATE_FORMAT_COMPATIBILITY_ID),
        "TAS project uses an incompatible GBA determinism or state format"
    );
    let direct_media = tilt_profile
        && identity.source_media_sha256 == identity.effective_media_sha256
        && identity.persistent_state != TasExternalIdentity::Absent
        && identity.rtc_state == TasExternalIdentity::Absent
        && identity.sync_config_sha256 == direct_gba_tilt_tas_sync_config_sha256()
        || !tilt_profile
            && identity.source_media_sha256 == identity.effective_media_sha256
            && match identity.rtc_state {
                TasExternalIdentity::Absent => match identity.persistent_state {
                    TasExternalIdentity::Absent => {
                        identity.sync_config_sha256 == direct_gba_tas_sync_config_sha256()
                    }
                    TasExternalIdentity::ExternalSha256(_) => {
                        identity.sync_config_sha256 == direct_gba_battery_tas_sync_config_sha256()
                    }
                },
                TasExternalIdentity::ExternalSha256(_) => supported_gba_rtc_backup_kinds()
                    .into_iter()
                    .filter(|kind| {
                        (*kind == BackupKind::None)
                            == (identity.persistent_state == TasExternalIdentity::Absent)
                    })
                    .any(|kind| {
                        identity.sync_config_sha256 == direct_gba_rtc_tas_sync_config_sha256(kind)
                    }),
            };
    let zip_media = identity.source_media_sha256 != identity.effective_media_sha256
        && identity.sync_config_sha256 != direct_gba_tas_sync_config_sha256()
        && identity.sync_config_sha256 != direct_gba_battery_tas_sync_config_sha256()
        && identity.sync_config_sha256 != direct_gba_tilt_tas_sync_config_sha256();
    ensure!(
        (direct_media || zip_media)
            && identity.patches.is_empty()
            && identity.firmware == firmware()
            && identity.devices
                == devices(if tilt_profile {
                    SensorKind::Tilt
                } else {
                    SensorKind::None
                }),
        "TAS project media, firmware, devices, or sync configuration is incompatible"
    );
    ensure!(
        identity.sensor_state
            == if tilt_profile {
                tilt::identity(Some(zeff_gba_core::hardware::cartridge::TiltState {
                    host_x_bits: 0,
                    host_y_bits: 0,
                    x_latch: 0x0FFF,
                    y_latch: 0x0FFF,
                    latch_ready: false,
                }))
            } else {
                TasExternalIdentity::Absent
            }
            && identity.cheats == TasExternalIdentity::Absent,
        "TAS project declares unsupported external state"
    );
    ensure!(
        TasDigest(current_state_rom_sha256(
            project.start_state(),
            tilt_profile
        )?) == identity.effective_media_sha256
            && TasDigest::from_bytes(project.start_state()) == identity.start_state_sha256,
        "GBA start state identity differs from the project"
    );
    Ok(())
}

pub(crate) fn is_gba_tilt_tas_identity(identity: &TasProjectIdentity) -> bool {
    identity.determinism_abi == zeff_gba_core::save_state::TILT_TAS_DETERMINISM_ABI_ID
        && identity.state_format_compatibility_id
            == zeff_gba_core::save_state::TILT_TAS_STATE_FORMAT_COMPATIBILITY_ID
}

pub(crate) fn validate_direct_gba_tas_branch_scope(
    project: &TasProject,
    branch_id: &str,
) -> Result<()> {
    validate_direct_gba_tas_project_identity(project)?;
    let tilt_profile = is_gba_tilt_tas_identity(project.identity());
    ensure!(
        project.replay_start() == &Default::default(),
        "direct GBA TAS execution does not support replay start metadata"
    );
    let branch = project
        .branch(branch_id)
        .with_context(|| format!("unknown TAS branch {branch_id:?}"))?;
    ensure!(
        branch.events().is_empty(),
        "direct GBA TAS execution does not support replay events"
    );
    for span in branch.input_spans() {
        let input = span.input;
        ensure!(
            input.players[0].buttons & !0x3F == 0
                && input.players[0].dpad & !0x0F == 0
                && input.players[1..]
                    .iter()
                    .all(|player| *player == Default::default())
                && input.coleco == [crate::tas_project::TasColecoControllerInput::default(); 2]
                && input.zapper == Default::default()
                && (tilt_profile || (input.tilt_x_bits == 0 && input.tilt_y_bits == 0))
                && matches!(input.camera, TasCameraInput::None),
            "direct GBA TAS execution supports one standard keypad only"
        );
    }
    Ok(())
}

pub(crate) fn validate_direct_gba_tas_project_witness(
    project: &TasProject,
    branch_id: &str,
    witness: crate::emu_backend::loader::TasProjectRuntimeWitness<'_>,
) -> Result<()> {
    validate_direct_gba_tas_branch_scope(project, branch_id)?;
    let identity = project.identity();
    ensure!(
        witness.source_media_sha256 == identity.source_media_sha256
            && witness.effective_media_sha256 == identity.effective_media_sha256
            && witness.determinism_abi == identity.determinism_abi
            && witness.state_format_compatibility_id == identity.state_format_compatibility_id
            && witness.sync_config_sha256 == identity.sync_config_sha256,
        "worker GBA identity does not match the TAS project"
    );
    ensure!(
        TasDigest::from_bytes(witness.current_state_bytes) == witness.current_state_sha256
            && TasDigest(current_state_rom_sha256(
                witness.current_state_bytes,
                is_gba_tilt_tas_identity(identity),
            )?) == identity.effective_media_sha256,
        "worker GBA state identity does not match the TAS project"
    );
    Ok(())
}

pub(crate) fn validate_direct_gba_tas_runtime(
    backend: &EmuBackend,
    cheats_present: bool,
) -> Result<zeff_gba_core::save_state::CurrentNativeGbaTasStateInspection> {
    let inspection = validate_direct_gba_tas_execution_runtime(backend, cheats_present)?;
    let provenance = backend
        .gba_tas_load_provenance()
        .context("GBA backend omitted load provenance")?;
    ensure!(
        provenance.load.initial_input.buttons == 0
            && provenance.load.initial_input.dpad == 0
            && inspection.keypad.buttons == 0
            && inspection.keypad.dpad == 0
            && !inspection.executing_in_bios,
        "direct GBA TAS acquisition requires neutral post-boot keypad state"
    );
    validate_initial_rtc(&inspection)?;
    validate_initial_tilt(&inspection)?;
    Ok(inspection)
}

pub(crate) fn validate_direct_gba_tas_execution_runtime(
    backend: &EmuBackend,
    cheats_present: bool,
) -> Result<zeff_gba_core::save_state::CurrentNativeGbaTasStateInspection> {
    validate_direct_gba_tas_runtime_inner(backend, cheats_present, false)
}

pub(crate) fn validate_direct_gba_tas_private_runtime(
    backend: &EmuBackend,
    cheats_present: bool,
) -> Result<zeff_gba_core::save_state::CurrentNativeGbaTasStateInspection> {
    let inspection = validate_direct_gba_tas_private_execution_runtime(backend, cheats_present)?;
    let provenance = backend
        .gba_tas_load_provenance()
        .context("GBA backend omitted load provenance")?;
    ensure!(
        provenance.load.initial_input.buttons == 0
            && provenance.load.initial_input.dpad == 0
            && inspection.keypad.buttons == 0
            && inspection.keypad.dpad == 0
            && !inspection.executing_in_bios,
        "direct GBA TAS acquisition requires neutral post-boot keypad state"
    );
    validate_initial_rtc(&inspection)?;
    validate_initial_tilt(&inspection)?;
    Ok(inspection)
}

pub(crate) fn validate_direct_gba_tas_private_execution_runtime(
    backend: &EmuBackend,
    cheats_present: bool,
) -> Result<zeff_gba_core::save_state::CurrentNativeGbaTasStateInspection> {
    validate_direct_gba_tas_runtime_inner(backend, cheats_present, true)
}

fn validate_direct_gba_tas_runtime_inner(
    backend: &EmuBackend,
    cheats_present: bool,
    allow_project_owned_battery: bool,
) -> Result<zeff_gba_core::save_state::CurrentNativeGbaTasStateInspection> {
    ensure!(
        backend.system() == ActiveSystem::GameBoyAdvance,
        "TAS execution profile requires a GBA backend"
    );
    let metadata = backend.replay_metadata();
    let expected_core_family = format!("{:?}", zeff_emu_common::system::CoreFamily::GameBoyAdvance);
    ensure!(
        metadata.system.as_deref() == Some(ActiveSystem::GameBoyAdvance.code())
            && metadata.core_family.as_deref() == Some(expected_core_family.as_str()),
        "GBA backend identity metadata is incompatible"
    );
    let effective_media_sha256 = metadata
        .rom_sha256
        .context("GBA backend omitted its effective media identity")?;
    let provenance = backend
        .gba_tas_load_provenance()
        .context("GBA backend omitted load provenance")?;
    ensure!(
        provenance.load.direct_gba_file
            && provenance.load.raw_source_media_sha256 == effective_media_sha256
            && (0xC0..=MAX_DIRECT_GBA_ROM_BYTES as usize)
                .contains(&provenance.load.raw_source_media_len),
        "GBA TAS execution requires one directly loaded bounded .gba cartridge"
    );
    ensure!(
        !provenance.load.any_mod_enabled && !provenance.load.any_mod_applied,
        "direct GBA TAS execution requires mods to be disabled"
    );
    ensure!(
        provenance.load.persistent_load != GbaTasPersistentLoadOutcome::Unknown,
        "direct GBA TAS could not establish cartridge persistence"
    );
    ensure!(
        provenance.load.configured_sample_rate == Some(DIRECT_GBA_SAMPLE_RATE)
            && provenance.load.initial_sample_rate == DIRECT_GBA_SAMPLE_RATE
            && provenance.current_sample_rate == DIRECT_GBA_SAMPLE_RATE,
        "direct GBA TAS execution requires 48000 Hz audio"
    );
    ensure!(
        !provenance.load.external_bios_selected
            && !provenance.external_bios_present
            && !provenance.load.rtc_seeded_from_host,
        "direct GBA TAS execution requires internal post-boot startup without host RTC"
    );
    ensure!(
        !cheats_present && metadata.cheat_sha256.is_none(),
        "GBA TAS execution enabled cheats"
    );
    ensure!(
        metadata.firmware
            == crate::emu_backend::firmware::default_firmware_manifests_for_active_system(
                ActiveSystem::GameBoyAdvance,
            )
            && matches!(
                metadata.firmware.as_slice(),
                [zeff_emu_common::replay::ReplayFirmwareManifest::Hle {
                    firmware_id,
                    implementation,
                    compatibility_version: 1,
                }] if firmware_id == "nintendo.gba.bios" && implementation == "zeff-gba-hle"
            )
            && backend.media_slot_snapshot().is_none(),
        "direct GBA TAS execution requires its exact HLE BIOS and no removable media"
    );
    let gba = backend.gba().context("GBA backend became unavailable")?;
    let state = backend.encode_state_bytes()?;
    let inspection = inspect_gba_tas_state(&gba.emu, &state)?;
    ensure!(
        inspection.rom_sha256 == effective_media_sha256,
        "GBA state ROM identity differs from the loaded cartridge"
    );
    let backup_kind = validate_gba_backup_kind(&gba.emu)?;
    let battery_backed = gba_has_project_owned_battery_from_inspection(backup_kind, &inspection)?;
    ensure!(
        (allow_project_owned_battery || !battery_backed)
            && (battery_backed
                || provenance.load.persistent_load == GbaTasPersistentLoadOutcome::Skipped
                || (allow_project_owned_battery
                    && provenance.load.persistent_load == GbaTasPersistentLoadOutcome::Absent)),
        "direct GBA TAS execution requires absent persistent storage"
    );
    ensure!(
        inspection.rtc_present == inspection.rtc_state.is_some()
            && inspection.rtc_date_time == inspection.rtc_state.map(|state| state.date_time)
            && (allow_project_owned_battery || !inspection.rtc_present),
        "GBA state contains unsupported RTC hardware or state"
    );
    match inspection.sensor_kind {
        SensorKind::None => ensure!(
            inspection.tilt_state.is_none(),
            "GBA state contains unexpected tilt state"
        ),
        SensorKind::Tilt => ensure!(
            backup_kind == BackupKind::Eeprom
                && !inspection.rtc_present
                && inspection.tilt_state.is_some(),
            "GBA tilt TAS requires an EEPROM cartridge without RTC"
        ),
    }
    ensure!(
        !inspection.external_bios
            && inspection.startup == zeff_gba_core::save_state::GbaTasStartup::InternalPostBoot,
        "GBA state does not use internal post-boot startup"
    );
    ensure!(
        inspection.sample_rate == DIRECT_GBA_SAMPLE_RATE,
        "GBA state uses a non-48000 Hz audio rate"
    );
    Ok(inspection)
}

pub(crate) fn validate_direct_gba_tas_state(
    backend: &mut EmuBackend,
    state: &[u8],
) -> Result<zeff_gba_core::save_state::CurrentNativeGbaTasStateProjection> {
    validate_direct_gba_tas_private_runtime(backend, false)?;
    let projection = restore_direct_gba_tas_state_bytes(backend, state)?;
    validate_direct_gba_tas_private_runtime(backend, false)?;
    Ok(projection)
}

pub(crate) fn restore_direct_gba_tas_execution_state(
    backend: &mut EmuBackend,
    state: &[u8],
) -> Result<zeff_gba_core::save_state::CurrentNativeGbaTasStateProjection> {
    validate_direct_gba_tas_private_execution_runtime(backend, false)?;
    let projection = restore_direct_gba_tas_state_bytes(backend, state)?;
    validate_direct_gba_tas_private_execution_runtime(backend, false)?;
    Ok(projection)
}

fn restore_direct_gba_tas_state_bytes(
    backend: &mut EmuBackend,
    state: &[u8],
) -> Result<zeff_gba_core::save_state::CurrentNativeGbaTasStateProjection> {
    let EmuBackend::Gba(gba) = backend else {
        anyhow::bail!("TAS state requires a GBA backend");
    };
    let inspection = if gba.emu.sensor_kind() == SensorKind::Tilt {
        zeff_gba_core::save_state::restore_current_native_gba_tilt_tas_state(&mut gba.emu, state)?
    } else {
        zeff_gba_core::save_state::restore_current_native_gba_tas_state(&mut gba.emu, state)?
    };
    ensure!(
        inspection.projection.framebuffer.len() == ActiveSystem::GameBoyAdvance.framebuffer_len()
            && inspection.projection.framebuffer.as_ref() == gba.emu.framebuffer(),
        "GBA TAS state did not restore its exact framebuffer"
    );
    Ok(inspection.projection)
}

pub(crate) fn gba_tas_sync_config(
    backend: &EmuBackend,
    zip_member: Option<&str>,
) -> Result<TasDigest> {
    let gba = backend.gba().context("GBA backend became unavailable")?;
    let state = backend.encode_state_bytes()?;
    let inspection = inspect_gba_tas_state(&gba.emu, &state)?;
    let backup_kind = validate_gba_backup_kind(&gba.emu)?;
    Ok(
        match (
            zip_member,
            inspection.sensor_kind,
            inspection.rtc_present,
            backup_kind,
        ) {
            (Some(member), SensorKind::Tilt, false, BackupKind::Eeprom) => {
                zip_gba_tilt_tas_sync_config_sha256(member)
            }
            (None, SensorKind::Tilt, false, BackupKind::Eeprom) => {
                direct_gba_tilt_tas_sync_config_sha256()
            }
            (_, SensorKind::Tilt, _, _) => anyhow::bail!("GBA tilt TAS topology is unsupported"),
            (Some(member), SensorKind::None, true, kind) => {
                zip_gba_rtc_tas_sync_config_sha256(member, kind)
            }
            (None, SensorKind::None, true, kind) => direct_gba_rtc_tas_sync_config_sha256(kind),
            (Some(member), SensorKind::None, false, BackupKind::None) => {
                zip_gba_tas_sync_config_sha256(member)
            }
            (Some(member), SensorKind::None, false, _) => {
                zip_gba_battery_tas_sync_config_sha256(member)
            }
            (None, SensorKind::None, false, BackupKind::None) => {
                direct_gba_tas_sync_config_sha256()
            }
            (None, SensorKind::None, false, _) => direct_gba_battery_tas_sync_config_sha256(),
        },
    )
}

fn inspect_gba_tas_state(
    emu: &zeff_gba_core::emulator::Emulator,
    state: &[u8],
) -> Result<zeff_gba_core::save_state::CurrentNativeGbaTasStateInspection> {
    if emu.sensor_kind() == SensorKind::Tilt {
        zeff_gba_core::save_state::inspect_current_native_gba_tilt_tas_state(emu, state)
    } else {
        zeff_gba_core::save_state::inspect_current_native_gba_tas_state(emu, state)
    }
}

fn validate_initial_rtc(
    inspection: &zeff_gba_core::save_state::CurrentNativeGbaTasStateInspection,
) -> Result<()> {
    ensure!(
        inspection.rtc_state.is_none_or(rtc::is_initial_epoch),
        "direct GBA TAS acquisition requires the deterministic RTC epoch"
    );
    Ok(())
}

fn validate_initial_tilt(
    inspection: &zeff_gba_core::save_state::CurrentNativeGbaTasStateInspection,
) -> Result<()> {
    ensure!(
        inspection.tilt_state.is_none_or(tilt::is_initial),
        "direct GBA TAS acquisition requires neutral initial tilt state"
    );
    Ok(())
}

fn gba_has_project_owned_battery_from_inspection(
    backup_kind: BackupKind,
    inspection: &zeff_gba_core::save_state::CurrentNativeGbaTasStateInspection,
) -> Result<bool> {
    match backup_kind {
        BackupKind::None => {
            ensure!(
                inspection.save_ram_kind == SaveRamKind::None && inspection.battery_data.is_none(),
                "GBA state contains unexpected cartridge persistence"
            );
            Ok(false)
        }
        kind @ (BackupKind::Sram
        | BackupKind::Flash512
        | BackupKind::Flash1M
        | BackupKind::Eeprom) => {
            let bytes = inspection
                .battery_data
                .as_deref()
                .context("GBA state omitted cartridge persistence")?;
            ensure!(
                inspection.save_ram_kind == kind.save_ram_kind() && bytes.len() == kind.size(),
                "GBA state has incompatible cartridge persistence"
            );
            Ok(true)
        }
    }
}

pub(super) fn validate_gba_backup_kind(
    emu: &zeff_gba_core::emulator::Emulator,
) -> Result<BackupKind> {
    let mut recognized = None;
    for window in emu.cartridge_rom_bytes().windows(8) {
        let kind = if window.starts_with(b"FLASH1M_") {
            Some(BackupKind::Flash1M)
        } else if window.starts_with(b"FLASH512") || window.starts_with(b"FLASH_V") {
            Some(BackupKind::Flash512)
        } else if window.starts_with(b"SRAM_V") || window.starts_with(b"SRAM_F") {
            Some(BackupKind::Sram)
        } else if window.starts_with(b"EEPROM_V") {
            Some(BackupKind::Eeprom)
        } else {
            if window.starts_with(b"FLASH")
                || window.starts_with(b"SRAM_")
                || window.starts_with(b"EEPROM")
            {
                anyhow::bail!("direct GBA TAS does not support an unknown backup type");
            }
            None
        };
        if let Some(kind) = kind {
            if let Some(existing) = recognized {
                ensure!(
                    existing == kind,
                    "direct GBA TAS does not support ambiguous backup types"
                );
            } else {
                recognized = Some(kind);
            }
        }
    }
    let backup_kind = emu.backup_kind();
    ensure!(
        recognized.unwrap_or(BackupKind::None) == backup_kind,
        "GBA backup recognition differs from the loaded cartridge"
    );
    Ok(backup_kind)
}

fn gba_persistent_identity(
    inspection: &zeff_gba_core::save_state::CurrentNativeGbaTasStateInspection,
) -> Result<TasExternalIdentity> {
    match &inspection.battery_data {
        Some(bytes) => Ok(TasExternalIdentity::ExternalSha256(TasDigest::from_bytes(
            bytes,
        ))),
        None if inspection.save_ram_kind == SaveRamKind::None => Ok(TasExternalIdentity::Absent),
        _ => anyhow::bail!("GBA state has incompatible cartridge persistence"),
    }
}
