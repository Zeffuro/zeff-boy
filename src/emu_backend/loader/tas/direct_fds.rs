use anyhow::{Context, Result, bail, ensure};
use zeff_emu_common::media::MediaEvent;
use zeff_emu_common::replay::{ReplayEvent, ReplayFirmwareManifest};

use super::{ActiveSystem, EmuBackend, TasProjectRuntimeWitness, tas_firmware_identity};
use crate::emu_thread::TasExecutionProfile;
use crate::tas_project::{
    TasCameraInput, TasDeviceIdentity, TasDigest, TasExternalIdentity, TasFirmwareIdentity,
    TasProject, TasProjectIdentity,
};

const FDS_CONTROLLER_CONFIGURATION: &[u8] =
    b"zeff-tas-device-config-v1\0fds-two-standard-controllers\0";
const FDS_SYNC_CONFIGURATION: &[u8] = b"zeff-tas-sync-config-v1\0fds-direct-disk\0sides=1..2\0drive=inserted-side0-writable\0controllers=two-standard\0mods=disabled\0initial-input=neutral\0sample-rate=48000\0mutable-disk=project-native-state\0host-persistence=disabled\0";
const FDS_ZIP_SYNC_CONFIGURATION: &[u8] = b"zeff-tas-sync-config-v1\0fds-zip-member\0sides=1..2\0drive=inserted-side0-writable\0controllers=two-standard\0mods=disabled\0initial-input=neutral\0sample-rate=48000\0mutable-disk=project-native-state\0host-persistence=disabled\0member=";
const FDS_THREE_SIDE_SYNC_CONFIGURATION: &[u8] = b"zeff-tas-sync-config-v1\0fds-direct-disk\0sides=3\0drive=inserted-side0-writable\0controllers=two-standard\0mods=disabled\0initial-input=neutral\0sample-rate=48000\0mutable-disk=project-native-state\0host-persistence=disabled\0";
const FDS_THREE_SIDE_ZIP_SYNC_CONFIGURATION: &[u8] = b"zeff-tas-sync-config-v1\0fds-zip-member\0sides=3\0drive=inserted-side0-writable\0controllers=two-standard\0mods=disabled\0initial-input=neutral\0sample-rate=48000\0mutable-disk=project-native-state\0host-persistence=disabled\0member=";
const FDS_FOUR_SIDE_SYNC_CONFIGURATION: &[u8] = b"zeff-tas-sync-config-v1\0fds-direct-disk\0sides=4\0drive=inserted-side0-writable\0controllers=two-standard\0mods=disabled\0initial-input=neutral\0sample-rate=48000\0mutable-disk=project-native-state\0host-persistence=disabled\0";
const FDS_FOUR_SIDE_ZIP_SYNC_CONFIGURATION: &[u8] = b"zeff-tas-sync-config-v1\0fds-zip-member\0sides=4\0drive=inserted-side0-writable\0controllers=two-standard\0mods=disabled\0initial-input=neutral\0sample-rate=48000\0mutable-disk=project-native-state\0host-persistence=disabled\0member=";
const FDS_MANY_SIDE_SYNC_CONFIGURATION_PREFIX: &[u8] =
    b"zeff-tas-sync-config-v1\0fds-direct-disk\0sides=";
const FDS_MANY_SIDE_ZIP_SYNC_CONFIGURATION_PREFIX: &[u8] =
    b"zeff-tas-sync-config-v1\0fds-zip-member\0sides=";
const FDS_MANY_SIDE_SYNC_CONFIGURATION_SUFFIX: &[u8] = b"\0drive=inserted-side0-writable\0controllers=two-standard\0mods=disabled\0initial-input=neutral\0sample-rate=48000\0mutable-disk=project-native-state\0host-persistence=disabled\0";
const FDS_MANY_SIDE_ZIP_SYNC_CONFIGURATION_SUFFIX: &[u8] = b"\0drive=inserted-side0-writable\0controllers=two-standard\0mods=disabled\0initial-input=neutral\0sample-rate=48000\0mutable-disk=project-native-state\0host-persistence=disabled\0member=";
const FDS_ZIP_ASSET_MAGIC: &[u8; 8] = b"ZFDSZIP1";

pub(crate) const MAX_FDS_SIDE_COUNT: u8 = u8::MAX;
pub(crate) const MAX_FDS_IMAGE_BYTES: u64 =
    zeff_nes_core::hardware::cartridge::mappers::FDS_HEADER_SIZE as u64
        + MAX_FDS_SIDE_COUNT as u64
            * zeff_nes_core::hardware::cartridge::mappers::FDS_SIDE_SIZE as u64;

pub(crate) fn direct_fds_tas_sync_config_sha256(side_count: usize) -> Result<TasDigest> {
    match side_count {
        1 | 2 => Ok(TasDigest::from_bytes(FDS_SYNC_CONFIGURATION)),
        3 => Ok(TasDigest::from_bytes(FDS_THREE_SIDE_SYNC_CONFIGURATION)),
        4 => Ok(TasDigest::from_bytes(FDS_FOUR_SIDE_SYNC_CONFIGURATION)),
        5..=255 => Ok(many_side_sync_config_sha256(
            FDS_MANY_SIDE_SYNC_CONFIGURATION_PREFIX,
            FDS_MANY_SIDE_SYNC_CONFIGURATION_SUFFIX,
            side_count,
            None,
        )),
        _ => bail!("FDS TAS execution supports one through 255 disk sides"),
    }
}

pub(crate) fn zip_fds_tas_sync_config_sha256(
    member_name: &str,
    side_count: usize,
) -> Result<TasDigest> {
    let prefix = match side_count {
        1 | 2 => FDS_ZIP_SYNC_CONFIGURATION,
        3 => FDS_THREE_SIDE_ZIP_SYNC_CONFIGURATION,
        4 => FDS_FOUR_SIDE_ZIP_SYNC_CONFIGURATION,
        5..=255 => {
            return Ok(many_side_sync_config_sha256(
                FDS_MANY_SIDE_ZIP_SYNC_CONFIGURATION_PREFIX,
                FDS_MANY_SIDE_ZIP_SYNC_CONFIGURATION_SUFFIX,
                side_count,
                Some(member_name),
            ));
        }
        _ => bail!("FDS TAS execution supports one through 255 disk sides"),
    };
    let mut bytes = Vec::with_capacity(prefix.len() + member_name.len());
    bytes.extend_from_slice(prefix);
    bytes.extend_from_slice(member_name.as_bytes());
    Ok(TasDigest::from_bytes(&bytes))
}

pub(crate) fn fds_tas_side_count_supported(side_count: usize) -> bool {
    (1..=usize::from(MAX_FDS_SIDE_COUNT)).contains(&side_count)
}

fn is_direct_fds_tas_sync_config(digest: TasDigest) -> bool {
    digest == TasDigest::from_bytes(FDS_SYNC_CONFIGURATION)
        || digest == TasDigest::from_bytes(FDS_THREE_SIDE_SYNC_CONFIGURATION)
        || digest == TasDigest::from_bytes(FDS_FOUR_SIDE_SYNC_CONFIGURATION)
        || (5..=usize::from(MAX_FDS_SIDE_COUNT)).any(|side_count| {
            many_side_sync_config_sha256(
                FDS_MANY_SIDE_SYNC_CONFIGURATION_PREFIX,
                FDS_MANY_SIDE_SYNC_CONFIGURATION_SUFFIX,
                side_count,
                None,
            ) == digest
        })
}

fn many_side_sync_config_sha256(
    prefix: &[u8],
    suffix: &[u8],
    side_count: usize,
    member_name: Option<&str>,
) -> TasDigest {
    let side_count = side_count.to_string();
    let member_name = member_name.unwrap_or_default();
    let mut bytes =
        Vec::with_capacity(prefix.len() + side_count.len() + suffix.len() + member_name.len());
    bytes.extend_from_slice(prefix);
    bytes.extend_from_slice(side_count.as_bytes());
    bytes.extend_from_slice(suffix);
    bytes.extend_from_slice(member_name.as_bytes());
    TasDigest::from_bytes(&bytes)
}

fn fds_tas_devices() -> Vec<TasDeviceIdentity> {
    ["p1", "p2"]
        .into_iter()
        .map(|port| TasDeviceIdentity {
            port: port.to_owned(),
            device: "nes-standard-controller".to_owned(),
            configuration_sha256: TasDigest::from_bytes(FDS_CONTROLLER_CONFIGURATION),
        })
        .collect()
}

fn fds_tas_firmware(backend: &EmuBackend) -> Result<Vec<TasFirmwareIdentity>> {
    let metadata = backend.replay_metadata();
    ensure!(
        matches!(
            metadata.firmware.as_slice(),
            [ReplayFirmwareManifest::External { firmware_id, .. }]
                if firmware_id == "nintendo.fds.bios"
        ),
        "FDS TAS execution requires exactly one external FDS BIOS"
    );
    Ok(metadata
        .firmware
        .iter()
        .map(tas_firmware_identity)
        .collect())
}

pub(crate) fn fds_tas_identity(
    backend: &EmuBackend,
    source_media_sha256: TasDigest,
    sync_config_sha256: TasDigest,
    start_state: &[u8],
) -> Result<TasProjectIdentity> {
    validate_fds_tas_private_runtime(backend, false)?;
    let metadata = backend.replay_metadata();
    ensure!(
        backend.encode_state_bytes()?.as_slice() == start_state,
        "FDS TAS start state differs from the loaded baseline"
    );
    Ok(TasProjectIdentity {
        system: metadata
            .system
            .context("FDS backend omitted its system identity")?,
        core_family: metadata
            .core_family
            .context("FDS backend omitted its core-family identity")?,
        determinism_abi: zeff_nes_core::save_state::TAS_DETERMINISM_ABI_ID.to_owned(),
        source_media_sha256,
        effective_media_sha256: TasDigest(
            metadata
                .rom_sha256
                .context("FDS backend omitted its effective media identity")?,
        ),
        patches: Vec::new(),
        firmware: fds_tas_firmware(backend)?,
        devices: fds_tas_devices(),
        sync_config_sha256,
        persistent_state: TasExternalIdentity::Absent,
        rtc_state: TasExternalIdentity::Absent,
        sensor_state: TasExternalIdentity::Absent,
        cheats: TasExternalIdentity::Absent,
        state_format_compatibility_id: zeff_nes_core::save_state::TAS_STATE_FORMAT_COMPATIBILITY_ID
            .to_owned(),
        start_state_sha256: TasDigest::from_bytes(start_state),
    })
}

pub(crate) fn validate_fds_tas_project_identity(project: &TasProject) -> Result<()> {
    let identity = project.identity();
    ensure!(
        identity.system == ActiveSystem::Nes.code()
            && identity.core_family == format!("{:?}", zeff_emu_common::system::CoreFamily::Nes),
        "TAS project does not identify the native NES core"
    );
    ensure!(
        identity.determinism_abi == zeff_nes_core::save_state::TAS_DETERMINISM_ABI_ID
            && identity.state_format_compatibility_id
                == zeff_nes_core::save_state::TAS_STATE_FORMAT_COMPATIBILITY_ID,
        "TAS project uses an incompatible FDS determinism or state format"
    );
    let direct = is_direct_fds_tas_sync_config(identity.sync_config_sha256);
    let (disk_asset, zip_member) = if direct {
        (
            project
                .assets()
                .get(&identity.source_media_sha256)
                .map(Vec::as_slice),
            None,
        )
    } else {
        let asset = project.assets().values().next().map(Vec::as_slice);
        let (member, disk) = asset
            .map(decode_zip_fds_asset)
            .transpose()?
            .context("FDS ZIP TAS project omitted its owned disk image")?;
        (Some(disk), Some(member))
    };
    let disk_asset = disk_asset.context("FDS TAS project omitted its owned disk image")?;
    let image = zeff_nes_core::hardware::cartridge::mappers::FdsImage::parse(disk_asset)
        .context("FDS TAS project disk image is invalid")?;
    ensure!(
        fds_tas_side_count_supported(image.side_count())
            && fds_image_digest(&image) == identity.effective_media_sha256
            && project.assets().len() == 1,
        "FDS TAS project disk asset or side topology is incompatible"
    );
    let expected_sync_config = match zip_member {
        Some(member) => zip_fds_tas_sync_config_sha256(member, image.side_count())?,
        None => direct_fds_tas_sync_config_sha256(image.side_count())?,
    };
    ensure!(
        expected_sync_config == identity.sync_config_sha256
            && (direct || TasDigest::from_bytes(disk_asset) == identity.effective_media_sha256),
        "ZIP FDS TAS requires a headerless selected member"
    );
    ensure!(
        matches!(
            identity.firmware.as_slice(),
            [TasFirmwareIdentity::External { firmware_id, .. }]
                if firmware_id == "nintendo.fds.bios"
        ) && identity.devices == fds_tas_devices()
            && identity.patches.is_empty(),
        "FDS TAS firmware, devices, or media transforms are incompatible"
    );
    ensure!(
        identity.persistent_state == TasExternalIdentity::Absent
            && identity.rtc_state == TasExternalIdentity::Absent
            && identity.sensor_state == TasExternalIdentity::Absent
            && identity.cheats == TasExternalIdentity::Absent
            && TasDigest::from_bytes(project.start_state()) == identity.start_state_sha256,
        "FDS TAS project declares unsupported external state"
    );
    super::validate_current_nes_start_state(project.start_state())
}

pub(crate) fn validate_fds_tas_branch_scope(project: &TasProject, branch_id: &str) -> Result<()> {
    validate_fds_tas_project_identity(project)?;
    ensure!(
        project.replay_start() == &Default::default(),
        "FDS TAS execution does not support replay start metadata"
    );
    let branch = project
        .branch(branch_id)
        .with_context(|| format!("unknown TAS branch {branch_id:?}"))?;
    let image = zeff_nes_core::hardware::cartridge::mappers::FdsImage::parse(
        fds_project_disk_bytes(project)?,
    )?;
    let side_count = image.side_count();
    let source_media_id = image.media_object_id();
    let mut previous_frame = None;
    let mut inserted = true;
    for event in branch.events() {
        let (frame, inserted_after) = match event {
            ReplayEvent::FdsDiskSide { frame, side } => {
                ensure!(
                    inserted && usize::from(*side) < side_count,
                    "FDS TAS disk-side event selects an unavailable side"
                );
                (*frame, inserted)
            }
            ReplayEvent::Media {
                frame,
                sequence,
                event: MediaEvent::SetWriteProtected { slot, .. },
            } => {
                ensure!(
                    inserted
                        && *sequence == 0
                        && slot.as_ref()
                            == zeff_nes_core::hardware::cartridge::mappers::FDS_DRIVE_SLOT_ID,
                    "FDS TAS write-protect event has an incompatible slot or sequence"
                );
                (*frame, inserted)
            }
            ReplayEvent::Media {
                frame,
                sequence,
                event: MediaEvent::Eject { slot },
            } => {
                ensure!(
                    inserted
                        && *sequence == 0
                        && slot.as_ref()
                            == zeff_nes_core::hardware::cartridge::mappers::FDS_DRIVE_SLOT_ID,
                    "FDS TAS eject event has an incompatible drive state, slot, or sequence"
                );
                (*frame, false)
            }
            ReplayEvent::Media {
                frame,
                sequence,
                event:
                    MediaEvent::Insert {
                        slot,
                        media_id,
                        side,
                        ..
                    },
            } => {
                ensure!(
                    !inserted
                        && *sequence == 0
                        && slot.as_ref()
                            == zeff_nes_core::hardware::cartridge::mappers::FDS_DRIVE_SLOT_ID
                        && media_id == &source_media_id
                        && side.is_some_and(|side| usize::from(side) < side_count),
                    "FDS TAS insert event does not restore the project-owned disk"
                );
                (*frame, true)
            }
            _ => bail!("FDS TAS execution contains an unsupported drive event"),
        };
        ensure!(
            frame < branch.frame_count() && previous_frame.is_none_or(|previous| previous < frame),
            "FDS TAS drive event is outside the linked execution profile"
        );
        previous_frame = Some(frame);
        inserted = inserted_after;
    }
    for span in branch.input_spans() {
        let input = span.input;
        ensure!(
            input.players[2..]
                .iter()
                .all(|player| *player == Default::default())
                && input.coleco == [crate::tas_project::TasColecoControllerInput::default(); 2]
                && input.zapper == Default::default()
                && input.tilt_x_bits == 0
                && input.tilt_y_bits == 0
                && matches!(input.camera, TasCameraInput::None),
            "FDS TAS execution supports two standard controllers only"
        );
    }
    Ok(())
}

pub(crate) fn validate_fds_tas_private_runtime(
    backend: &EmuBackend,
    cheats_present: bool,
) -> Result<()> {
    validate_fds_tas_runtime(backend, cheats_present, true)
}

pub(crate) fn validate_fds_tas_execution_runtime(
    backend: &EmuBackend,
    cheats_present: bool,
) -> Result<()> {
    validate_fds_tas_runtime(backend, cheats_present, false)
}

fn validate_fds_tas_runtime(
    backend: &EmuBackend,
    cheats_present: bool,
    require_inserted_writable: bool,
) -> Result<()> {
    let nes = backend
        .nes()
        .context("FDS TAS execution requires a NES backend")?;
    let metadata = backend.replay_metadata();
    let snapshot = nes
        .media_slot_snapshot()
        .context("FDS TAS execution requires an inserted disk")?;
    ensure!(
        backend.system() == ActiveSystem::Nes
            && nes.has_standard_console_hardware()
            && backend.nes_has_standard_controller_topology() == Some(true)
            && !nes.host_persistence_enabled()
            && snapshot.source_media_id.is_some()
            && ((!snapshot.inserted()
                && !require_inserted_writable
                && snapshot.state.side.is_none()
                && !snapshot.state.write_protected)
                || (snapshot.inserted()
                    && snapshot
                        .state
                        .side
                        .is_some_and(|side| side < snapshot.side_count)
                    && (!require_inserted_writable || !snapshot.state.write_protected)))
            && fds_tas_side_count_supported(usize::from(snapshot.side_count)),
        "FDS TAS runtime topology is incompatible"
    );
    ensure!(
        !cheats_present && metadata.cheat_sha256.is_none(),
        "FDS TAS execution does not support cheats"
    );
    fds_tas_firmware(backend)?;
    Ok(())
}

pub(crate) fn validate_fds_tas_project_witness(
    project: &TasProject,
    branch_id: &str,
    witness: TasProjectRuntimeWitness<'_>,
) -> Result<()> {
    validate_fds_tas_branch_scope(project, branch_id)?;
    let identity = project.identity();
    ensure!(
        witness.profile == TasExecutionProfile::DirectFdsDisk
            && witness.source_media_sha256 == identity.source_media_sha256
            && witness.effective_media_sha256 == identity.effective_media_sha256
            && witness.current_state_sha256 == TasDigest::from_bytes(witness.current_state_bytes)
            && witness.determinism_abi == identity.determinism_abi
            && witness.state_format_compatibility_id == identity.state_format_compatibility_id
            && witness.sync_config_sha256 == identity.sync_config_sha256,
        "worker FDS witness does not match the TAS project"
    );
    super::validate_current_nes_start_state(witness.current_state_bytes)
}

pub(crate) fn fds_project_disk_bytes(project: &TasProject) -> Result<&[u8]> {
    validate_fds_tas_project_identity(project)?;
    let identity = project.identity();
    if is_direct_fds_tas_sync_config(identity.sync_config_sha256) {
        return project
            .assets()
            .get(&identity.source_media_sha256)
            .map(Vec::as_slice)
            .context("FDS TAS project omitted its owned disk image");
    }
    let asset = project
        .assets()
        .values()
        .next()
        .context("FDS ZIP TAS project omitted its owned disk image")?;
    Ok(decode_zip_fds_asset(asset)?.1)
}

pub(crate) fn encode_zip_fds_asset(member_name: &str, disk: &[u8]) -> Result<Vec<u8>> {
    let member_len = u32::try_from(member_name.len())?;
    let mut asset =
        Vec::with_capacity(FDS_ZIP_ASSET_MAGIC.len() + 4 + member_name.len() + disk.len());
    asset.extend_from_slice(FDS_ZIP_ASSET_MAGIC);
    asset.extend_from_slice(&member_len.to_le_bytes());
    asset.extend_from_slice(member_name.as_bytes());
    asset.extend_from_slice(disk);
    Ok(asset)
}

fn decode_zip_fds_asset(asset: &[u8]) -> Result<(&str, &[u8])> {
    ensure!(
        asset.len() >= FDS_ZIP_ASSET_MAGIC.len() + 4
            && &asset[..FDS_ZIP_ASSET_MAGIC.len()] == FDS_ZIP_ASSET_MAGIC,
        "FDS ZIP TAS asset has an invalid envelope"
    );
    let length_start = FDS_ZIP_ASSET_MAGIC.len();
    let member_len = u32::from_le_bytes(
        asset[length_start..length_start + 4]
            .try_into()
            .expect("length checked"),
    ) as usize;
    let member_start = length_start + 4;
    let disk_start = member_start
        .checked_add(member_len)
        .context("FDS ZIP member length overflow")?;
    ensure!(disk_start < asset.len(), "FDS ZIP TAS asset is truncated");
    let member = std::str::from_utf8(&asset[member_start..disk_start])?;
    ensure!(!member.is_empty(), "FDS ZIP TAS member identity is empty");
    Ok((member, &asset[disk_start..]))
}

fn fds_image_digest(image: &zeff_nes_core::hardware::cartridge::mappers::FdsImage) -> TasDigest {
    let mut bytes = Vec::with_capacity(image.side_data_len());
    image.sides().for_each(|side| bytes.extend_from_slice(side));
    TasDigest::from_bytes(&bytes)
}
