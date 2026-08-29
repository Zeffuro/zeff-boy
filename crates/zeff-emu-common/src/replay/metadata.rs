use anyhow::{Result, bail};

use super::{
    MetadataCursor, ReplayEvent, ReplayGameBoyLinkCoordinatorState, ReplayGameBoyLinkState,
    read_bool, write_optional_hash, write_optional_string, write_optional_u64, write_string,
    write_u32,
};

const METADATA_VERSION: u32 = 3;
const MIN_METADATA_VERSION: u32 = 1;
const MAX_REPLAY_FIRMWARE_MANIFESTS: usize = 4096;
const MAX_REPLAY_EVENTS: usize = 100_000;
const MAX_REPLAY_CHECKPOINTS: usize = 100_000;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReplayCheckpoint {
    pub frame: u64,
    pub state_sha256: [u8; 32],
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ReplayMetadata {
    pub system: Option<String>,
    pub core_family: Option<String>,
    pub rom_sha256: Option<[u8; 32]>,
    pub firmware: Vec<ReplayFirmwareManifest>,
    pub events: Vec<ReplayEvent>,
    pub cheat_sha256: Option<[u8; 32]>,
    pub final_state_sha256: Option<[u8; 32]>,
    pub game_boy_link_start_state: Option<ReplayGameBoyLinkState>,
    pub game_boy_link_start_tick: Option<u64>,
    pub wonder_swan_link_start_tick: Option<u64>,
    pub checkpoints: Vec<ReplayCheckpoint>,
    pub game_boy_link_coordinator_start_state: Option<ReplayGameBoyLinkCoordinatorState>,
}

pub fn firmware_manifests_match(
    left: &[ReplayFirmwareManifest],
    right: &[ReplayFirmwareManifest],
) -> bool {
    if left.len() != right.len() {
        return false;
    }
    let mut matched = vec![false; right.len()];
    left.iter().all(|expected| {
        let Some(index) = right
            .iter()
            .enumerate()
            .position(|(index, actual)| !matched[index] && actual == expected)
        else {
            return false;
        };
        matched[index] = true;
        true
    })
}

impl ReplayMetadata {
    pub fn is_empty(&self) -> bool {
        self.system.is_none()
            && self.core_family.is_none()
            && self.rom_sha256.is_none()
            && self.firmware.is_empty()
            && self.events.is_empty()
            && self.cheat_sha256.is_none()
            && self.final_state_sha256.is_none()
            && self.game_boy_link_start_state.is_none()
            && self.game_boy_link_start_tick.is_none()
            && self.wonder_swan_link_start_tick.is_none()
            && self.checkpoints.is_empty()
            && self.game_boy_link_coordinator_start_state.is_none()
    }

    pub(super) fn encode(&self) -> Result<Vec<u8>> {
        self.encode_with_version(METADATA_VERSION)
    }

    pub(super) fn encode_with_version(&self, version: u32) -> Result<Vec<u8>> {
        self.validate_encoding_limits()?;
        let mut out = Vec::new();
        write_u32(&mut out, version);
        write_optional_string(&mut out, self.system.as_deref());
        write_optional_string(&mut out, self.core_family.as_deref());
        write_optional_hash(&mut out, self.rom_sha256);
        write_u32(&mut out, self.firmware.len() as u32);
        for entry in &self.firmware {
            entry.encode(&mut out);
        }
        let mut events = self.events.clone();
        events.sort_by_key(ReplayEvent::sort_key);
        write_u32(&mut out, events.len() as u32);
        for event in &events {
            event.encode(&mut out, version);
        }
        write_optional_hash(&mut out, self.cheat_sha256);
        write_optional_hash(&mut out, self.final_state_sha256);
        write_optional_game_boy_link_state(&mut out, self.game_boy_link_start_state, version);
        write_optional_u64(&mut out, self.game_boy_link_start_tick);
        write_optional_u64(&mut out, self.wonder_swan_link_start_tick);
        let mut checkpoints = self.checkpoints.clone();
        checkpoints.sort_by_key(|checkpoint| checkpoint.frame);
        write_u32(&mut out, checkpoints.len() as u32);
        for checkpoint in &checkpoints {
            out.extend_from_slice(&checkpoint.frame.to_le_bytes());
            out.extend_from_slice(&checkpoint.state_sha256);
        }
        if version >= 3 {
            write_optional_game_boy_link_coordinator_state(
                &mut out,
                self.game_boy_link_coordinator_start_state,
            );
        }
        Ok(out)
    }

    fn validate_encoding_limits(&self) -> Result<()> {
        validate_count(
            "firmware manifests",
            self.firmware.len(),
            MAX_REPLAY_FIRMWARE_MANIFESTS,
        )?;
        validate_count("events", self.events.len(), MAX_REPLAY_EVENTS)?;
        validate_count(
            "checkpoints",
            self.checkpoints.len(),
            MAX_REPLAY_CHECKPOINTS,
        )?;
        for value in [self.system.as_deref(), self.core_family.as_deref()]
            .into_iter()
            .flatten()
        {
            validate_string_len(value)?;
        }
        for firmware in &self.firmware {
            match firmware {
                ReplayFirmwareManifest::External {
                    firmware_id,
                    variant,
                    ..
                } => {
                    validate_string_len(firmware_id)?;
                    if let Some(variant) = variant {
                        validate_string_len(variant)?;
                    }
                }
                ReplayFirmwareManifest::Hle {
                    firmware_id,
                    implementation,
                    ..
                }
                | ReplayFirmwareManifest::BuiltinOpenSource {
                    firmware_id,
                    implementation,
                    ..
                } => {
                    validate_string_len(firmware_id)?;
                    validate_string_len(implementation)?;
                }
                ReplayFirmwareManifest::Skipped { firmware_id, .. } => {
                    validate_string_len(firmware_id)?;
                }
            }
        }
        for event in &self.events {
            if let ReplayEvent::Media { event, .. } = event {
                validate_string_len(event.slot().as_ref())?;
                if let crate::media::MediaEvent::Insert { media_id, .. } = event {
                    validate_string_len(media_id.as_ref())?;
                }
            }
        }
        Ok(())
    }

    pub(super) fn decode(bytes: &[u8]) -> Result<Self> {
        let mut cursor = MetadataCursor::new(bytes);
        let version = cursor.read_u32()?;
        if !(MIN_METADATA_VERSION..=METADATA_VERSION).contains(&version) {
            bail!("unsupported replay metadata version: {version}");
        }

        let system = cursor.read_optional_string()?;
        let core_family = cursor.read_optional_string()?;
        let rom_sha256 = cursor.read_optional_hash()?;
        let firmware_count = cursor.read_u32()? as usize;
        if firmware_count > MAX_REPLAY_FIRMWARE_MANIFESTS {
            bail!("replay exceeds {MAX_REPLAY_FIRMWARE_MANIFESTS} firmware manifests");
        }
        if firmware_count > cursor.remaining() {
            bail!("replay firmware count exceeds the remaining metadata");
        }
        let mut firmware = Vec::with_capacity(firmware_count);
        for _ in 0..firmware_count {
            firmware.push(ReplayFirmwareManifest::decode(&mut cursor)?);
        }
        let event_count = cursor.read_u32()? as usize;
        if event_count > MAX_REPLAY_EVENTS {
            bail!("replay exceeds {MAX_REPLAY_EVENTS} events");
        }
        if event_count > cursor.remaining() {
            bail!("replay event count exceeds the remaining metadata");
        }
        let mut events = Vec::with_capacity(event_count);
        for _ in 0..event_count {
            events.push(ReplayEvent::decode(&mut cursor, version)?);
        }
        events.sort_by_key(ReplayEvent::sort_key);
        let cheat_sha256 = read_optional_hash_if_present(&mut cursor)?;
        let final_state_sha256 = read_optional_hash_if_present(&mut cursor)?;
        let game_boy_link_start_state =
            read_optional_game_boy_link_state_if_present(&mut cursor, version)?;
        let game_boy_link_start_tick = read_optional_u64_if_present(&mut cursor)?;
        let wonder_swan_link_start_tick = read_optional_u64_if_present(&mut cursor)?;
        let checkpoints = read_checkpoints_if_present(&mut cursor)?;
        let game_boy_link_coordinator_start_state = if version >= 3 {
            read_optional_game_boy_link_coordinator_state(&mut cursor)?
        } else {
            None
        };
        cursor.finish()?;

        if let Some(coordinator_state) = game_boy_link_coordinator_start_state {
            let link_state = game_boy_link_start_state.ok_or_else(|| {
                anyhow::anyhow!("GB master continuation is missing its core link start state")
            })?;
            if game_boy_link_start_tick.is_none() {
                bail!("GB master continuation is missing its link start tick");
            }
            coordinator_state.validate_against(link_state)?;
            validate_game_boy_link_coordinator_events(coordinator_state, &events)?;
        } else if version >= 3
            && let Some(link_state) = game_boy_link_start_state
        {
            validate_uncoordinated_game_boy_link_start(link_state, &events)?;
        }

        Ok(Self {
            system,
            core_family,
            rom_sha256,
            firmware,
            events,
            cheat_sha256,
            final_state_sha256,
            game_boy_link_start_state,
            game_boy_link_start_tick,
            wonder_swan_link_start_tick,
            checkpoints,
            game_boy_link_coordinator_start_state,
        })
    }
}

fn validate_count(name: &str, count: usize, maximum: usize) -> Result<()> {
    if count > maximum {
        bail!("replay exceeds {maximum} {name}");
    }
    u32::try_from(count).map_err(|_| anyhow::anyhow!("replay {name} count does not fit u32"))?;
    Ok(())
}

fn validate_string_len(value: &str) -> Result<()> {
    u32::try_from(value.len())
        .map(|_| ())
        .map_err(|_| anyhow::anyhow!("replay metadata string length does not fit u32"))
}

pub(super) fn validate_uncoordinated_game_boy_link_start(
    state: ReplayGameBoyLinkState,
    events: &[ReplayEvent],
) -> Result<()> {
    if !state.has_master_owned_transfer() {
        return Ok(());
    }
    let Some(action) = state.queued_master_action else {
        bail!("GB master start state has no coordinator ownership");
    };
    if state.pending_master_byte != Some(action.out_byte)
        || state.pending_master_response.is_some()
        || state.pending_master_completion_ready
        || state.serial_generation != action.serial_generation
        || action.clock_period_t_cycles == 0
        || action.clock_period_t_cycles > 4096
    {
        bail!("GB queued master start state is internally inconsistent");
    }
    let first_local_start = events.iter().find_map(|event| {
        let ReplayEvent::GameBoyLink {
            event:
                super::ReplayGameBoyLinkEvent::LocalMasterStart {
                    clock_period_t_cycles,
                    out_byte,
                    serial_generation,
                    ..
                },
            ..
        } = event
        else {
            return None;
        };
        Some(super::ReplayGameBoyLinkAction {
            out_byte: *out_byte,
            clock_period_t_cycles: *clock_period_t_cycles,
            serial_generation: *serial_generation,
        })
    });
    if first_local_start != Some(action) {
        bail!("GB queued master start state has no matching future local-start event");
    }
    Ok(())
}

pub(super) fn validate_game_boy_link_coordinator_events(
    coordinator: ReplayGameBoyLinkCoordinatorState,
    events: &[ReplayEvent],
) -> Result<()> {
    let matching_events: Vec<_> = events
        .iter()
        .enumerate()
        .filter(|event| {
            let (_, event) = event;
            matches!(
                event,
                ReplayEvent::GameBoyLink {
                    event:
                        super::ReplayGameBoyLinkEvent::LocalMasterStart { transfer_id, .. }
                        | super::ReplayGameBoyLinkEvent::RemoteMasterStart { transfer_id, .. }
                        | super::ReplayGameBoyLinkEvent::RemoteReply { transfer_id, .. },
                    ..
                } if *transfer_id == coordinator.transfer_id
            )
        })
        .collect();
    let reply_count = matching_events
        .iter()
        .filter(|(_, event)| {
            matches!(
                event,
                ReplayEvent::GameBoyLink {
                    event: super::ReplayGameBoyLinkEvent::RemoteReply { .. },
                    ..
                }
            )
        })
        .count();
    let first_link_ordinal = events.iter().position(|event| {
        matches!(
            event,
            ReplayEvent::GameBoyLink { .. }
                | ReplayEvent::GameBoyLinkState { .. }
                | ReplayEvent::GameBoyLinkStateAtTick { .. }
        )
    });

    match coordinator.owner {
        super::ReplayGameBoyLinkCoordinatorOwner::ReplayAwaitingReply
            if matching_events.len() != 1
                || reply_count != 1
                || matching_events.first().map(|(ordinal, _)| *ordinal) != first_link_ordinal =>
        {
            bail!(
                "GB replay-owned master continuation requires exactly one future event, a reply for transfer {}; found {} matching events and {reply_count} replies",
                coordinator.transfer_id,
                matching_events.len()
            )
        }
        super::ReplayGameBoyLinkCoordinatorOwner::CoreHasReply if !matching_events.is_empty() => {
            bail!(
                "GB core-owned master continuation repeats transfer {} in {} future events",
                coordinator.transfer_id,
                matching_events.len()
            )
        }
        _ => Ok(()),
    }
}

fn write_optional_game_boy_link_coordinator_state(
    out: &mut Vec<u8>,
    state: Option<ReplayGameBoyLinkCoordinatorState>,
) {
    match state {
        Some(state) => {
            out.push(1);
            state.encode(out);
        }
        None => out.push(0),
    }
}

fn read_optional_game_boy_link_coordinator_state(
    cursor: &mut MetadataCursor<'_>,
) -> Result<Option<ReplayGameBoyLinkCoordinatorState>> {
    if !read_bool(cursor, "GB master continuation present flag")? {
        return Ok(None);
    }
    Ok(Some(ReplayGameBoyLinkCoordinatorState::decode(cursor)?))
}

fn read_optional_hash_if_present(cursor: &mut MetadataCursor<'_>) -> Result<Option<[u8; 32]>> {
    if cursor.is_finished() {
        Ok(None)
    } else {
        cursor.read_optional_hash()
    }
}

fn write_optional_game_boy_link_state(
    out: &mut Vec<u8>,
    state: Option<ReplayGameBoyLinkState>,
    metadata_version: u32,
) {
    match state {
        Some(state) => {
            out.push(1);
            state.encode(out, metadata_version);
        }
        None => out.push(0),
    }
}

fn read_optional_game_boy_link_state_if_present(
    cursor: &mut MetadataCursor<'_>,
    metadata_version: u32,
) -> Result<Option<ReplayGameBoyLinkState>> {
    if cursor.is_finished() {
        return Ok(None);
    }
    if !read_bool(cursor, "GB link start state present flag")? {
        return Ok(None);
    }
    Ok(Some(ReplayGameBoyLinkState::decode(
        cursor,
        metadata_version,
    )?))
}

fn read_optional_u64_if_present(cursor: &mut MetadataCursor<'_>) -> Result<Option<u64>> {
    if cursor.is_finished() {
        return Ok(None);
    }
    match cursor.read_u8()? {
        0 => Ok(None),
        1 => Ok(Some(cursor.read_u64()?)),
        tag => bail!("invalid optional u64 tag: {tag}"),
    }
}

fn read_checkpoints_if_present(cursor: &mut MetadataCursor<'_>) -> Result<Vec<ReplayCheckpoint>> {
    if cursor.is_finished() {
        return Ok(Vec::new());
    }
    let count = cursor.read_u32()? as usize;
    if count > MAX_REPLAY_CHECKPOINTS {
        bail!("replay exceeds {MAX_REPLAY_CHECKPOINTS} checkpoints");
    }
    let required_bytes = count
        .checked_mul(40)
        .ok_or_else(|| anyhow::anyhow!("replay checkpoint size overflow"))?;
    if required_bytes > cursor.remaining() {
        bail!("replay checkpoint count exceeds the remaining metadata");
    }
    let mut checkpoints = Vec::with_capacity(count);
    for _ in 0..count {
        checkpoints.push(ReplayCheckpoint {
            frame: cursor.read_u64()?,
            state_sha256: cursor.read_hash()?,
        });
    }
    checkpoints.sort_by_key(|checkpoint| checkpoint.frame);
    Ok(checkpoints)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReplayFirmwareManifest {
    External {
        firmware_id: String,
        variant: Option<String>,
        sha256: [u8; 32],
    },
    Hle {
        firmware_id: String,
        implementation: String,
        compatibility_version: u32,
    },
    BuiltinOpenSource {
        firmware_id: String,
        implementation: String,
        compatibility_version: u32,
        sha256: [u8; 32],
    },
    Skipped {
        firmware_id: String,
        compatibility_version: u32,
    },
}

impl ReplayFirmwareManifest {
    fn encode(&self, out: &mut Vec<u8>) {
        match self {
            Self::External {
                firmware_id,
                variant,
                sha256,
            } => {
                out.push(0);
                write_string(out, firmware_id);
                write_optional_string(out, variant.as_deref());
                out.extend_from_slice(sha256);
            }
            Self::Hle {
                firmware_id,
                implementation,
                compatibility_version,
            } => {
                out.push(1);
                write_string(out, firmware_id);
                write_string(out, implementation);
                write_u32(out, *compatibility_version);
            }
            Self::BuiltinOpenSource {
                firmware_id,
                implementation,
                compatibility_version,
                sha256,
            } => {
                out.push(2);
                write_string(out, firmware_id);
                write_string(out, implementation);
                write_u32(out, *compatibility_version);
                out.extend_from_slice(sha256);
            }
            Self::Skipped {
                firmware_id,
                compatibility_version,
            } => {
                out.push(3);
                write_string(out, firmware_id);
                write_u32(out, *compatibility_version);
            }
        }
    }

    fn decode(cursor: &mut MetadataCursor<'_>) -> Result<Self> {
        match cursor.read_u8()? {
            0 => Ok(Self::External {
                firmware_id: cursor.read_string()?,
                variant: cursor.read_optional_string()?,
                sha256: cursor.read_hash()?,
            }),
            1 => Ok(Self::Hle {
                firmware_id: cursor.read_string()?,
                implementation: cursor.read_string()?,
                compatibility_version: cursor.read_u32()?,
            }),
            2 => Ok(Self::BuiltinOpenSource {
                firmware_id: cursor.read_string()?,
                implementation: cursor.read_string()?,
                compatibility_version: cursor.read_u32()?,
                sha256: cursor.read_hash()?,
            }),
            3 => Ok(Self::Skipped {
                firmware_id: cursor.read_string()?,
                compatibility_version: cursor.read_u32()?,
            }),
            tag => bail!("unknown replay firmware manifest tag: {tag}"),
        }
    }
}
