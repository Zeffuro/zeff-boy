use anyhow::{Result, bail};

use super::ReplayEvent;
use super::metadata::{
    validate_game_boy_link_coordinator_events, validate_uncoordinated_game_boy_link_start,
};
use super::{
    MetadataCursor, ReplayGameBoyLinkCoordinatorState, ReplayGameBoyLinkState, read_bool,
    write_optional_u64, write_u32,
};

const MAGIC: &[u8; 4] = b"ZRST";
const VERSION: u32 = 1;
const REPLAY_METADATA_VERSION: u32 = 3;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ReplayStartMetadata {
    pub game_boy_link_state: Option<ReplayGameBoyLinkState>,
    pub game_boy_link_tick: Option<u64>,
    pub wonder_swan_link_tick: Option<u64>,
    pub game_boy_link_coordinator_state: Option<ReplayGameBoyLinkCoordinatorState>,
}

pub fn encode_replay_start_metadata(metadata: &ReplayStartMetadata) -> Result<Vec<u8>> {
    validate(metadata)?;
    let mut out = Vec::new();
    out.extend_from_slice(MAGIC);
    write_u32(&mut out, VERSION);
    match metadata.game_boy_link_state {
        Some(state) => {
            out.push(1);
            state.encode(&mut out, REPLAY_METADATA_VERSION);
        }
        None => out.push(0),
    }
    write_optional_u64(&mut out, metadata.game_boy_link_tick);
    write_optional_u64(&mut out, metadata.wonder_swan_link_tick);
    match metadata.game_boy_link_coordinator_state {
        Some(state) => {
            out.push(1);
            state.encode(&mut out);
        }
        None => out.push(0),
    }
    Ok(out)
}

pub fn decode_replay_start_metadata(bytes: &[u8]) -> Result<ReplayStartMetadata> {
    let mut cursor = MetadataCursor::new(bytes);
    if cursor.read_exact(4)? != MAGIC {
        bail!("invalid replay start metadata magic");
    }
    let version = cursor.read_u32()?;
    if version != VERSION {
        bail!("unsupported replay start metadata version: {version}");
    }
    let game_boy_link_state = if read_bool(&mut cursor, "GB link state present flag")? {
        Some(ReplayGameBoyLinkState::decode(
            &mut cursor,
            REPLAY_METADATA_VERSION,
        )?)
    } else {
        None
    };
    let game_boy_link_tick = read_optional_u64(&mut cursor, "GB link tick")?;
    let wonder_swan_link_tick = read_optional_u64(&mut cursor, "WS link tick")?;
    let game_boy_link_coordinator_state =
        if read_bool(&mut cursor, "GB link coordinator present flag")? {
            Some(ReplayGameBoyLinkCoordinatorState::decode(&mut cursor)?)
        } else {
            None
        };
    cursor.finish()?;
    let metadata = ReplayStartMetadata {
        game_boy_link_state,
        game_boy_link_tick,
        wonder_swan_link_tick,
        game_boy_link_coordinator_state,
    };
    validate(&metadata)?;
    Ok(metadata)
}

pub fn validate_replay_start_events(
    metadata: &ReplayStartMetadata,
    events: &[ReplayEvent],
) -> Result<()> {
    validate(metadata)?;
    match (
        metadata.game_boy_link_coordinator_state,
        metadata.game_boy_link_state,
    ) {
        (Some(coordinator), _) => validate_game_boy_link_coordinator_events(coordinator, events),
        (None, Some(state)) => validate_uncoordinated_game_boy_link_start(state, events),
        (None, None) => Ok(()),
    }
}

fn read_optional_u64(cursor: &mut MetadataCursor<'_>, name: &str) -> Result<Option<u64>> {
    match cursor.read_u8()? {
        0 => Ok(None),
        1 => Ok(Some(cursor.read_u64()?)),
        value => bail!("invalid replay start metadata {name} tag: {value}"),
    }
}

fn validate(metadata: &ReplayStartMetadata) -> Result<()> {
    if let Some(state) = metadata.game_boy_link_state {
        state.validate()?;
    }
    if let Some(coordinator) = metadata.game_boy_link_coordinator_state {
        let state = metadata.game_boy_link_state.ok_or_else(|| {
            anyhow::anyhow!("GB replay coordinator is missing its link start state")
        })?;
        if metadata.game_boy_link_tick.is_none() {
            bail!("GB replay coordinator is missing its link start tick");
        }
        coordinator.validate_against(state)?;
    }
    Ok(())
}
