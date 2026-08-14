use anyhow::{Result, bail};

use super::{
    MetadataCursor, ReplayEvent, ReplayGameBoyLinkState, read_bool, write_optional_hash,
    write_optional_string, write_optional_u64, write_string, write_u32,
};

const METADATA_VERSION: u32 = 1;

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
    }

    pub(super) fn encode(&self) -> Vec<u8> {
        let mut out = Vec::new();
        write_u32(&mut out, METADATA_VERSION);
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
            event.encode(&mut out);
        }
        write_optional_hash(&mut out, self.cheat_sha256);
        write_optional_hash(&mut out, self.final_state_sha256);
        write_optional_game_boy_link_state(&mut out, self.game_boy_link_start_state);
        write_optional_u64(&mut out, self.game_boy_link_start_tick);
        out
    }

    pub(super) fn decode(bytes: &[u8]) -> Result<Self> {
        let mut cursor = MetadataCursor::new(bytes);
        let version = cursor.read_u32()?;
        if version != METADATA_VERSION {
            bail!("unsupported replay metadata version: {version}");
        }

        let system = cursor.read_optional_string()?;
        let core_family = cursor.read_optional_string()?;
        let rom_sha256 = cursor.read_optional_hash()?;
        let firmware_count = cursor.read_u32()? as usize;
        let mut firmware = Vec::with_capacity(firmware_count);
        for _ in 0..firmware_count {
            firmware.push(ReplayFirmwareManifest::decode(&mut cursor)?);
        }
        let event_count = cursor.read_u32()? as usize;
        let mut events = Vec::with_capacity(event_count);
        for _ in 0..event_count {
            events.push(ReplayEvent::decode(&mut cursor)?);
        }
        events.sort_by_key(ReplayEvent::sort_key);
        let cheat_sha256 = read_optional_hash_if_present(&mut cursor)?;
        let final_state_sha256 = read_optional_hash_if_present(&mut cursor)?;
        let game_boy_link_start_state = read_optional_game_boy_link_state_if_present(&mut cursor)?;
        let game_boy_link_start_tick = read_optional_u64_if_present(&mut cursor)?;
        cursor.finish()?;

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
        })
    }
}

fn read_optional_hash_if_present(cursor: &mut MetadataCursor<'_>) -> Result<Option<[u8; 32]>> {
    if cursor.is_finished() {
        Ok(None)
    } else {
        cursor.read_optional_hash()
    }
}

fn write_optional_game_boy_link_state(out: &mut Vec<u8>, state: Option<ReplayGameBoyLinkState>) {
    match state {
        Some(state) => {
            out.push(1);
            state.encode(out);
        }
        None => out.push(0),
    }
}

fn read_optional_game_boy_link_state_if_present(
    cursor: &mut MetadataCursor<'_>,
) -> Result<Option<ReplayGameBoyLinkState>> {
    if cursor.is_finished() {
        return Ok(None);
    }
    if !read_bool(cursor, "GB link start state present flag")? {
        return Ok(None);
    }
    Ok(Some(ReplayGameBoyLinkState::decode(cursor)?))
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
