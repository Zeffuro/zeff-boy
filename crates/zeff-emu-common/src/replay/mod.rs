/// File format (`.zrpl`):
/// ```text
/// [4 bytes]  magic: "ZRPL"
/// [4 bytes]  version: 1 (u32 LE)
/// [4 bytes]  metadata_length (u32 LE)
/// [N bytes]  replay metadata
/// [4 bytes]  save_state_length (u32 LE)
/// [N bytes]  save state data
/// [4 bytes]  frame_count (u32 LE)
/// [remaining] frames: repeated frame records
///     fixed input record, 18 bytes:
///         p1_buttons: u8, p1_dpad: u8, p2_buttons: u8, p2_dpad: u8,
///         zapper_flags: u8, zapper_x: u16 LE, zapper_y: u16 LE,
///         tilt_x: f32 LE, tilt_y: f32 LE, reserved: u8
///     camera_frame_length: u32 LE
///         0 = no host camera update for this frame
///         0xFFFF_FFFF = repeat previous host camera frame
///         otherwise exactly that many camera bytes follow
/// ```
use std::fs::File;
use std::io::{BufWriter, Read, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

const MAGIC: &[u8; 4] = b"ZRPL";
const VERSION: u32 = 1;
const METADATA_VERSION: u32 = 1;
const FRAME_FIXED_BYTES: usize = 18;
const CAMERA_REPEAT_SENTINEL: u32 = u32::MAX;
const MAX_REPLAY_CAMERA_FRAME_BYTES: usize = 1024 * 1024;
pub const POCKET_CAMERA_FRAME_BYTES: usize = 128 * 112;
// Pre-metadata v1 recordings stored `[save_state_len][save_state][2-byte input frames]`.
// Keep this narrow so corrupt metadata-first files are not silently accepted as legacy.
const LEGACY_GB_SAVE_STATE_MAGIC: &[u8; 8] = b"ZBSTATE\0";
const LEGACY_NES_SAVE_STATE_MAGIC: &[u8; 8] = b"ZBNSTATE";

#[derive(Clone, Debug, Default)]
pub struct ReplayJoypadFrame {
    pub buttons: u8,
    pub dpad: u8,
    pub buttons_p2: u8,
    pub dpad_p2: u8,
    pub zapper: ReplayZapperFrame,
    pub host_tilt: (f32, f32),
    pub camera_frame: Option<Vec<u8>>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ReplayZapperFrame {
    pub enabled: bool,
    pub trigger: bool,
    pub hit: bool,
    pub screen_pos: Option<(u16, u16)>,
}

impl PartialEq for ReplayJoypadFrame {
    fn eq(&self, other: &Self) -> bool {
        self.buttons == other.buttons
            && self.dpad == other.dpad
            && self.buttons_p2 == other.buttons_p2
            && self.dpad_p2 == other.dpad_p2
            && self.zapper == other.zapper
            && self.host_tilt.0.to_bits() == other.host_tilt.0.to_bits()
            && self.host_tilt.1.to_bits() == other.host_tilt.1.to_bits()
            && self.camera_frame == other.camera_frame
    }
}

impl Eq for ReplayJoypadFrame {}

impl ReplayJoypadFrame {
    pub fn p1(buttons: u8, dpad: u8) -> Self {
        Self {
            buttons,
            dpad,
            buttons_p2: 0,
            dpad_p2: 0,
            zapper: ReplayZapperFrame::default(),
            host_tilt: (0.0, 0.0),
            camera_frame: None,
        }
    }

    pub fn uses_host_tilt_input(&self) -> bool {
        self.host_tilt.0 != 0.0 || self.host_tilt.1 != 0.0
    }

    pub fn uses_host_camera_input(&self) -> bool {
        self.camera_frame.is_some()
    }

    pub fn uses_zapper_input(&self) -> bool {
        self.zapper.enabled
            || self.zapper.trigger
            || self.zapper.hit
            || self.zapper.screen_pos.is_some()
    }
}

impl ReplayZapperFrame {
    fn flags(self) -> u8 {
        u8::from(self.enabled)
            | (u8::from(self.trigger) << 1)
            | (u8::from(self.hit) << 2)
            | (u8::from(self.screen_pos.is_some()) << 3)
    }

    fn from_parts(flags: u8, x: u16, y: u16) -> Result<Self> {
        if flags & !0x0F != 0 {
            bail!("invalid replay zapper flags: {flags:#04X}");
        }
        Ok(Self {
            enabled: flags & 0x01 != 0,
            trigger: flags & 0x02 != 0,
            hit: flags & 0x04 != 0,
            screen_pos: (flags & 0x08 != 0).then_some((x, y)),
        })
    }
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

    fn encode(&self) -> Vec<u8> {
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

    fn decode(bytes: &[u8]) -> Result<Self> {
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReplayEvent {
    FdsDiskSide {
        frame: u64,
        side: u8,
    },
    GameBoyLink {
        frame: u64,
        tick: u64,
        event: ReplayGameBoyLinkEvent,
    },
    GameBoyLinkState {
        frame: u64,
        state: ReplayGameBoyLinkState,
    },
    WonderSwanLink {
        frame: u64,
        session_cycle: u64,
        event: ReplayWonderSwanLinkEvent,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReplayGameBoyLinkEvent {
    LocalMasterStart {
        transfer_id: u64,
        clock_period_t_cycles: u64,
        out_byte: u8,
        serial_generation: u64,
    },
    RemoteMasterStart {
        transfer_id: u64,
        clock_period_t_cycles: u64,
        out_byte: u8,
        serial_generation: u64,
        local_reply: Option<ReplayGameBoyLinkReply>,
    },
    RemoteReply {
        transfer_id: u64,
        out_byte: u8,
        passive: bool,
        serial_generation: u64,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReplayGameBoyLinkReply {
    pub out_byte: u8,
    pub passive: bool,
    pub serial_generation: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReplayGameBoyLinkAction {
    pub out_byte: u8,
    pub clock_period_t_cycles: u64,
    pub serial_generation: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReplayGameBoyLinkState {
    pub peer_present: bool,
    pub pending_master_byte: Option<u8>,
    pub pending_master_response: Option<u8>,
    pub pending_master_completion_ready: bool,
    pub queued_master_action: Option<ReplayGameBoyLinkAction>,
    pub serial_generation: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReplayWonderSwanLinkEvent {
    RemoteByte {
        generation: u64,
        baud_bps: u32,
        byte: u8,
    },
}

impl ReplayEvent {
    pub fn frame(&self) -> u64 {
        match self {
            Self::FdsDiskSide { frame, .. } => *frame,
            Self::GameBoyLink { frame, .. } => *frame,
            Self::GameBoyLinkState { frame, .. } => *frame,
            Self::WonderSwanLink { frame, .. } => *frame,
        }
    }

    fn sort_key(&self) -> (u64, u64, u8) {
        match self {
            Self::FdsDiskSide { frame, .. } => (*frame, 0, 0),
            Self::GameBoyLinkState { frame, .. } => (*frame, 0, 1),
            Self::GameBoyLink { frame, tick, event } => {
                (*frame, *tick, 2 + event.sort_discriminant())
            }
            Self::WonderSwanLink {
                frame,
                session_cycle,
                ..
            } => (*frame, *session_cycle, 5),
        }
    }

    fn is_frame_boundary_event(&self) -> bool {
        matches!(
            self,
            Self::FdsDiskSide { .. } | Self::GameBoyLinkState { .. }
        )
    }

    fn encode(&self, out: &mut Vec<u8>) {
        match self {
            Self::FdsDiskSide { frame, side } => {
                out.push(0);
                write_u64(out, *frame);
                out.push(*side);
            }
            Self::GameBoyLink { frame, tick, event } => {
                out.push(1);
                write_u64(out, *frame);
                write_u64(out, *tick);
                event.encode(out);
            }
            Self::GameBoyLinkState { frame, state } => {
                out.push(3);
                write_u64(out, *frame);
                state.encode(out);
            }
            Self::WonderSwanLink {
                frame,
                session_cycle,
                event,
            } => {
                out.push(2);
                write_u64(out, *frame);
                write_u64(out, *session_cycle);
                event.encode(out);
            }
        }
    }

    fn decode(cursor: &mut MetadataCursor<'_>) -> Result<Self> {
        match cursor.read_u8()? {
            0 => Ok(Self::FdsDiskSide {
                frame: cursor.read_u64()?,
                side: cursor.read_u8()?,
            }),
            1 => Ok(Self::GameBoyLink {
                frame: cursor.read_u64()?,
                tick: cursor.read_u64()?,
                event: ReplayGameBoyLinkEvent::decode(cursor)?,
            }),
            3 => Ok(Self::GameBoyLinkState {
                frame: cursor.read_u64()?,
                state: ReplayGameBoyLinkState::decode(cursor)?,
            }),
            2 => Ok(Self::WonderSwanLink {
                frame: cursor.read_u64()?,
                session_cycle: cursor.read_u64()?,
                event: ReplayWonderSwanLinkEvent::decode(cursor)?,
            }),
            tag => bail!("unknown replay event tag: {tag}"),
        }
    }
}

impl ReplayGameBoyLinkEvent {
    fn sort_discriminant(&self) -> u8 {
        match self {
            Self::LocalMasterStart { .. } => 0,
            Self::RemoteMasterStart { .. } => 1,
            Self::RemoteReply { .. } => 2,
        }
    }

    fn encode(&self, out: &mut Vec<u8>) {
        match self {
            Self::LocalMasterStart {
                transfer_id,
                clock_period_t_cycles,
                out_byte,
                serial_generation,
            } => {
                out.push(3);
                write_u64(out, *transfer_id);
                write_u64(out, *clock_period_t_cycles);
                out.push(*out_byte);
                write_u64(out, *serial_generation);
            }
            Self::RemoteMasterStart {
                transfer_id,
                clock_period_t_cycles,
                out_byte,
                serial_generation,
                local_reply,
            } => match local_reply {
                Some(reply) => {
                    out.push(2);
                    write_u64(out, *transfer_id);
                    write_u64(out, *clock_period_t_cycles);
                    out.push(*out_byte);
                    write_u64(out, *serial_generation);
                    reply.encode(out);
                }
                None => {
                    out.push(0);
                    write_u64(out, *transfer_id);
                    write_u64(out, *clock_period_t_cycles);
                    out.push(*out_byte);
                    write_u64(out, *serial_generation);
                }
            },
            Self::RemoteReply {
                transfer_id,
                out_byte,
                passive,
                serial_generation,
            } => {
                out.push(1);
                write_u64(out, *transfer_id);
                out.push(*out_byte);
                out.push(u8::from(*passive));
                write_u64(out, *serial_generation);
            }
        }
    }

    fn decode(cursor: &mut MetadataCursor<'_>) -> Result<Self> {
        match cursor.read_u8()? {
            0 => Ok(Self::RemoteMasterStart {
                transfer_id: cursor.read_u64()?,
                clock_period_t_cycles: cursor.read_u64()?,
                out_byte: cursor.read_u8()?,
                serial_generation: cursor.read_u64()?,
                local_reply: None,
            }),
            1 => Ok(Self::RemoteReply {
                transfer_id: cursor.read_u64()?,
                out_byte: cursor.read_u8()?,
                passive: read_bool(cursor, "GB link reply passive flag")?,
                serial_generation: cursor.read_u64()?,
            }),
            2 => Ok(Self::RemoteMasterStart {
                transfer_id: cursor.read_u64()?,
                clock_period_t_cycles: cursor.read_u64()?,
                out_byte: cursor.read_u8()?,
                serial_generation: cursor.read_u64()?,
                local_reply: Some(ReplayGameBoyLinkReply::decode(cursor)?),
            }),
            3 => Ok(Self::LocalMasterStart {
                transfer_id: cursor.read_u64()?,
                clock_period_t_cycles: cursor.read_u64()?,
                out_byte: cursor.read_u8()?,
                serial_generation: cursor.read_u64()?,
            }),
            tag => bail!("unknown GB replay link event tag: {tag}"),
        }
    }
}

impl ReplayGameBoyLinkReply {
    fn encode(&self, out: &mut Vec<u8>) {
        out.push(self.out_byte);
        out.push(u8::from(self.passive));
        write_u64(out, self.serial_generation);
    }

    fn decode(cursor: &mut MetadataCursor<'_>) -> Result<Self> {
        Ok(Self {
            out_byte: cursor.read_u8()?,
            passive: read_bool(cursor, "GB remote-master local reply passive flag")?,
            serial_generation: cursor.read_u64()?,
        })
    }
}

impl ReplayGameBoyLinkAction {
    fn encode(self, out: &mut Vec<u8>) {
        out.push(self.out_byte);
        write_u64(out, self.clock_period_t_cycles);
        write_u64(out, self.serial_generation);
    }

    fn decode(cursor: &mut MetadataCursor<'_>) -> Result<Self> {
        Ok(Self {
            out_byte: cursor.read_u8()?,
            clock_period_t_cycles: cursor.read_u64()?,
            serial_generation: cursor.read_u64()?,
        })
    }
}

impl ReplayGameBoyLinkState {
    pub fn is_idle(self) -> bool {
        !self.peer_present
            && self.pending_master_byte.is_none()
            && self.pending_master_response.is_none()
            && !self.pending_master_completion_ready
            && self.queued_master_action.is_none()
    }

    fn encode(self, out: &mut Vec<u8>) {
        out.push(u8::from(self.peer_present));
        write_optional_u8(out, self.pending_master_byte);
        write_optional_u8(out, self.pending_master_response);
        out.push(u8::from(self.pending_master_completion_ready));
        match self.queued_master_action {
            Some(action) => {
                out.push(1);
                action.encode(out);
            }
            None => out.push(0),
        }
        write_u64(out, self.serial_generation);
    }

    fn decode(cursor: &mut MetadataCursor<'_>) -> Result<Self> {
        Ok(Self {
            peer_present: read_bool(cursor, "GB link start peer-present flag")?,
            pending_master_byte: read_optional_u8(cursor, "GB link start pending master byte")?,
            pending_master_response: read_optional_u8(
                cursor,
                "GB link start pending master response",
            )?,
            pending_master_completion_ready: read_bool(
                cursor,
                "GB link start pending master completion flag",
            )?,
            queued_master_action: if read_bool(cursor, "GB link start queued action flag")? {
                Some(ReplayGameBoyLinkAction::decode(cursor)?)
            } else {
                None
            },
            serial_generation: cursor.read_u64()?,
        })
    }
}

impl ReplayWonderSwanLinkEvent {
    fn encode(&self, out: &mut Vec<u8>) {
        match self {
            Self::RemoteByte {
                generation,
                baud_bps,
                byte,
            } => {
                out.push(0);
                write_u64(out, *generation);
                write_u32(out, *baud_bps);
                out.push(*byte);
            }
        }
    }

    fn decode(cursor: &mut MetadataCursor<'_>) -> Result<Self> {
        match cursor.read_u8()? {
            0 => Ok(Self::RemoteByte {
                generation: cursor.read_u64()?,
                baud_bps: cursor.read_u32()?,
                byte: cursor.read_u8()?,
            }),
            tag => bail!("unknown WonderSwan replay link event tag: {tag}"),
        }
    }
}

pub struct ReplayRecorder {
    path: PathBuf,
    save_state: Vec<u8>,
    frames: Vec<ReplayJoypadFrame>,
    metadata: ReplayMetadata,
}

impl ReplayRecorder {
    pub fn new(path: PathBuf, save_state: Vec<u8>) -> Self {
        Self {
            path,
            save_state,
            frames: Vec::with_capacity(3600),
            metadata: ReplayMetadata::default(),
        }
    }

    pub fn new_with_metadata(path: PathBuf, save_state: Vec<u8>, metadata: ReplayMetadata) -> Self {
        Self {
            path,
            save_state,
            frames: Vec::with_capacity(3600),
            metadata,
        }
    }

    pub fn record_frame(&mut self, buttons_pressed: u8, dpad_pressed: u8) {
        self.record_joypad_frame(ReplayJoypadFrame::p1(buttons_pressed, dpad_pressed));
    }

    pub fn record_joypad_frame(&mut self, frame: ReplayJoypadFrame) {
        self.frames.push(frame);
    }

    pub fn record_event(&mut self, event: ReplayEvent) {
        self.metadata.events.push(event);
    }

    pub fn set_final_state_sha256(&mut self, hash: [u8; 32]) {
        self.metadata.final_state_sha256 = Some(hash);
    }

    pub fn finish(mut self) -> Result<PathBuf> {
        pad_frames_to_metadata_events(&mut self.frames, &self.metadata);

        let raw_file = File::create(&self.path)
            .with_context(|| format!("failed to create replay file: {}", self.path.display()))?;
        let mut file = BufWriter::new(raw_file);

        file.write_all(MAGIC)?;
        file.write_all(&VERSION.to_le_bytes())?;
        let metadata = self.metadata.encode();
        file.write_all(&(metadata.len() as u32).to_le_bytes())?;
        file.write_all(&metadata)?;
        file.write_all(&(self.save_state.len() as u32).to_le_bytes())?;
        file.write_all(&self.save_state)?;
        file.write_all(&(self.frames.len() as u32).to_le_bytes())?;
        let mut previous_camera_frame: Option<&[u8]> = None;
        for frame in &self.frames {
            file.write_all(&[
                frame.buttons,
                frame.dpad,
                frame.buttons_p2,
                frame.dpad_p2,
                frame.zapper.flags(),
            ])?;
            let (x, y) = frame.zapper.screen_pos.unwrap_or((0, 0));
            file.write_all(&x.to_le_bytes())?;
            file.write_all(&y.to_le_bytes())?;
            file.write_all(&frame.host_tilt.0.to_le_bytes())?;
            file.write_all(&frame.host_tilt.1.to_le_bytes())?;
            file.write_all(&[0])?;
            match frame.camera_frame.as_deref() {
                None => {
                    file.write_all(&0u32.to_le_bytes())?;
                }
                Some(camera_frame) if Some(camera_frame) == previous_camera_frame => {
                    file.write_all(&CAMERA_REPEAT_SENTINEL.to_le_bytes())?;
                }
                Some(camera_frame) => {
                    let camera_len = u32::try_from(camera_frame.len())
                        .context("replay camera frame is too large")?;
                    if camera_len == CAMERA_REPEAT_SENTINEL {
                        bail!("replay camera frame is too large");
                    }
                    file.write_all(&camera_len.to_le_bytes())?;
                    file.write_all(camera_frame)?;
                    previous_camera_frame = Some(camera_frame);
                }
            }
        }

        file.flush()?;
        file.get_ref().sync_all()?;
        log::info!(
            "Wrote replay: {} frames to {}",
            self.frames.len(),
            self.path.display()
        );
        Ok(self.path)
    }

    pub fn frame_count(&self) -> usize {
        self.frames.len()
    }
}

pub struct ReplayPlayer {
    save_state: Vec<u8>,
    frames: Vec<ReplayJoypadFrame>,
    cursor: usize,
    metadata: ReplayMetadata,
    event_cursor: usize,
}

fn pad_frames_to_metadata_events(frames: &mut Vec<ReplayJoypadFrame>, metadata: &ReplayMetadata) {
    let Some(required_frames) = metadata
        .events
        .iter()
        .filter_map(|event| usize::try_from(event.frame().saturating_add(1)).ok())
        .max()
    else {
        return;
    };
    if frames.len() >= required_frames {
        return;
    }

    let pad_frame = frames.last().cloned().unwrap_or_default();
    frames.resize(required_frames, pad_frame);
}

impl ReplayPlayer {
    pub fn load(path: &Path) -> Result<Self> {
        let mut file = File::open(path)
            .with_context(|| format!("failed to open replay file: {}", path.display()))?;

        let mut magic = [0u8; 4];
        file.read_exact(&mut magic)?;
        if &magic != MAGIC {
            bail!("not a valid replay file");
        }

        let mut version_buf = [0u8; 4];
        file.read_exact(&mut version_buf)?;
        let version = u32::from_le_bytes(version_buf);
        if version != VERSION {
            bail!("unsupported replay version: {version}");
        }

        let mut first_len_buf = [0u8; 4];
        file.read_exact(&mut first_len_buf)?;
        let first_len = u32::from_le_bytes(first_len_buf) as usize;
        let mut first_block = vec![0u8; first_len];
        file.read_exact(&mut first_block)?;

        if legacy_v1_save_state_block(&first_block) {
            let mut input_data = Vec::new();
            file.read_to_end(&mut input_data)?;
            let frames = decode_legacy_v1_input_frames(&input_data)?;

            log::info!(
                "Loaded legacy replay: {} frames from {}",
                frames.len(),
                path.display()
            );

            return Ok(Self {
                save_state: first_block,
                frames,
                cursor: 0,
                metadata: ReplayMetadata::default(),
                event_cursor: 0,
            });
        }

        let metadata = ReplayMetadata::decode(&first_block).context("invalid replay metadata")?;

        let mut state_len_buf = [0u8; 4];
        file.read_exact(&mut state_len_buf)?;
        let state_len = u32::from_le_bytes(state_len_buf) as usize;

        let mut save_state = vec![0u8; state_len];
        file.read_exact(&mut save_state)?;

        let mut input_data = Vec::new();
        file.read_to_end(&mut input_data)?;
        let mut frames = decode_input_frames(&input_data)?;
        pad_frames_to_metadata_events(&mut frames, &metadata);

        log::info!(
            "Loaded replay: {} frames from {}",
            frames.len(),
            path.display()
        );

        Ok(Self {
            save_state,
            frames,
            cursor: 0,
            metadata,
            event_cursor: 0,
        })
    }

    pub fn save_state(&self) -> &[u8] {
        &self.save_state
    }

    pub fn metadata(&self) -> &ReplayMetadata {
        &self.metadata
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    pub fn take_events_at_cursor(&mut self) -> Vec<ReplayEvent> {
        let cursor = u64::try_from(self.cursor).unwrap_or(u64::MAX);
        let mut events = Vec::new();
        while let Some(event) = self.metadata.events.get(self.event_cursor) {
            if event.frame() > cursor {
                break;
            }
            if event.frame() == cursor && event.is_frame_boundary_event() {
                events.push(event.clone());
            }
            self.event_cursor += 1;
        }
        events
    }

    pub fn frames_until_next_event(&self, max_frames: usize) -> usize {
        let cursor = u64::try_from(self.cursor).unwrap_or(u64::MAX);
        let Some(next_event_frame) = self
            .metadata
            .events
            .iter()
            .skip(self.event_cursor)
            .find(|event| event.is_frame_boundary_event())
            .map(ReplayEvent::frame)
        else {
            return max_frames;
        };
        if next_event_frame <= cursor {
            return 0;
        }
        max_frames.min((next_event_frame - cursor) as usize)
    }

    pub fn next_frame(&mut self) -> Option<(u8, u8)> {
        self.next_joypad_frame()
            .map(|frame| (frame.buttons, frame.dpad))
    }

    pub fn next_joypad_frame(&mut self) -> Option<ReplayJoypadFrame> {
        if self.cursor < self.frames.len() {
            let frame = self.frames[self.cursor].clone();
            self.cursor += 1;
            Some(frame)
        } else {
            None
        }
    }

    pub fn peek_frames(&self, offset: usize, count: usize) -> Vec<(u8, u8)> {
        self.peek_joypad_frames(offset, count)
            .into_iter()
            .map(|frame| (frame.buttons, frame.dpad))
            .collect()
    }

    pub fn peek_joypad_frames(&self, offset: usize, count: usize) -> Vec<ReplayJoypadFrame> {
        let start = self.cursor.saturating_add(offset).min(self.frames.len());
        let end = start.saturating_add(count).min(self.frames.len());
        self.frames[start..end].to_vec()
    }

    pub fn advance_frames(&mut self, count: usize) {
        self.cursor = self.cursor.saturating_add(count).min(self.frames.len());
    }

    pub fn remaining(&self) -> usize {
        self.frames.len().saturating_sub(self.cursor)
    }

    pub fn total_frames(&self) -> usize {
        self.frames.len()
    }

    pub fn is_finished(&self) -> bool {
        self.cursor >= self.frames.len()
    }

    pub fn uses_host_tilt_input(&self) -> bool {
        self.frames
            .iter()
            .any(ReplayJoypadFrame::uses_host_tilt_input)
    }

    pub fn uses_host_camera_input(&self) -> bool {
        self.frames
            .iter()
            .any(ReplayJoypadFrame::uses_host_camera_input)
    }

    pub fn uses_zapper_input(&self) -> bool {
        self.frames.iter().any(ReplayJoypadFrame::uses_zapper_input)
    }

    pub fn uses_game_boy_link_events(&self) -> bool {
        self.metadata.events.iter().any(|event| {
            matches!(
                event,
                ReplayEvent::GameBoyLink { .. } | ReplayEvent::GameBoyLinkState { .. }
            )
        })
    }

    pub fn uses_wonder_swan_link_events(&self) -> bool {
        self.metadata
            .events
            .iter()
            .any(|event| matches!(event, ReplayEvent::WonderSwanLink { .. }))
    }

    pub fn game_boy_link_events(
        &self,
    ) -> impl Iterator<Item = (u64, u64, ReplayGameBoyLinkEvent)> + '_ {
        self.metadata.events.iter().filter_map(|event| {
            if let ReplayEvent::GameBoyLink { frame, tick, event } = event {
                Some((*frame, *tick, *event))
            } else {
                None
            }
        })
    }

    pub fn wonder_swan_link_events(
        &self,
    ) -> impl Iterator<Item = (u64, u64, ReplayWonderSwanLinkEvent)> + '_ {
        self.metadata.events.iter().filter_map(|event| {
            if let ReplayEvent::WonderSwanLink {
                frame,
                session_cycle,
                event,
            } = event
            {
                Some((*frame, *session_cycle, *event))
            } else {
                None
            }
        })
    }

    pub fn host_camera_frame_lengths(&self) -> impl Iterator<Item = (usize, usize)> + '_ {
        self.frames.iter().enumerate().filter_map(|(index, frame)| {
            frame
                .camera_frame
                .as_ref()
                .map(|bytes| (index, bytes.len()))
        })
    }
}

fn write_u32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn decode_input_frames(input_data: &[u8]) -> Result<Vec<ReplayJoypadFrame>> {
    if input_data.len() < 4 {
        bail!("replay input stream is missing frame count");
    }
    let frame_count =
        u32::from_le_bytes([input_data[0], input_data[1], input_data[2], input_data[3]]) as usize;

    let mut frames = Vec::with_capacity(frame_count);
    let mut offset = 4usize;
    let mut previous_camera_frame: Option<Vec<u8>> = None;
    for frame_index in 0..frame_count {
        let chunk = read_replay_input_exact(input_data, &mut offset, FRAME_FIXED_BYTES)
            .with_context(|| format!("truncated replay input frame {frame_index}"))?;
        if chunk[17] != 0 {
            bail!("invalid replay frame reserved byte: {:#04X}", chunk[17]);
        }
        let camera_len_bytes = read_replay_input_exact(input_data, &mut offset, 4)
            .with_context(|| format!("truncated replay camera length at frame {frame_index}"))?;
        let camera_len = u32::from_le_bytes([
            camera_len_bytes[0],
            camera_len_bytes[1],
            camera_len_bytes[2],
            camera_len_bytes[3],
        ]);
        let camera_frame = match camera_len {
            0 => None,
            CAMERA_REPEAT_SENTINEL => Some(previous_camera_frame.clone().ok_or_else(|| {
                anyhow::anyhow!("replay camera frame repeats before any camera frame")
            })?),
            len => {
                let len = len as usize;
                if len > MAX_REPLAY_CAMERA_FRAME_BYTES {
                    bail!("replay camera frame is too large: {len} bytes");
                }
                let bytes = read_replay_input_exact(input_data, &mut offset, len)
                    .with_context(|| format!("truncated replay camera frame {frame_index}"))?
                    .to_vec();
                previous_camera_frame = Some(bytes.clone());
                Some(bytes)
            }
        };
        frames.push(ReplayJoypadFrame {
            buttons: chunk[0],
            dpad: chunk[1],
            buttons_p2: chunk[2],
            dpad_p2: chunk[3],
            zapper: ReplayZapperFrame::from_parts(
                chunk[4],
                u16::from_le_bytes([chunk[5], chunk[6]]),
                u16::from_le_bytes([chunk[7], chunk[8]]),
            )?,
            host_tilt: (
                f32::from_le_bytes([chunk[9], chunk[10], chunk[11], chunk[12]]),
                f32::from_le_bytes([chunk[13], chunk[14], chunk[15], chunk[16]]),
            ),
            camera_frame,
        });
    }
    if offset != input_data.len() {
        bail!(
            "replay input stream has trailing bytes: expected {offset} bytes, got {}",
            input_data.len()
        );
    }
    Ok(frames)
}

fn legacy_v1_save_state_block(bytes: &[u8]) -> bool {
    bytes.starts_with(LEGACY_GB_SAVE_STATE_MAGIC) || bytes.starts_with(LEGACY_NES_SAVE_STATE_MAGIC)
}

fn decode_legacy_v1_input_frames(input_data: &[u8]) -> Result<Vec<ReplayJoypadFrame>> {
    if !input_data.len().is_multiple_of(2) {
        bail!(
            "legacy replay input stream has odd byte length: {}",
            input_data.len()
        );
    }

    Ok(input_data
        .chunks_exact(2)
        .map(|chunk| ReplayJoypadFrame::p1(chunk[0], chunk[1]))
        .collect())
}

fn read_replay_input_exact<'a>(
    input_data: &'a [u8],
    offset: &mut usize,
    len: usize,
) -> Result<&'a [u8]> {
    let end = offset
        .checked_add(len)
        .ok_or_else(|| anyhow::anyhow!("replay input offset overflow"))?;
    let bytes = input_data
        .get(*offset..end)
        .ok_or_else(|| anyhow::anyhow!("truncated replay input stream"))?;
    *offset = end;
    Ok(bytes)
}

fn write_u64(out: &mut Vec<u8>, value: u64) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn write_string(out: &mut Vec<u8>, value: &str) {
    write_u32(out, value.len() as u32);
    out.extend_from_slice(value.as_bytes());
}

fn write_optional_string(out: &mut Vec<u8>, value: Option<&str>) {
    match value {
        Some(value) => {
            out.push(1);
            write_string(out, value);
        }
        None => out.push(0),
    }
}

fn write_optional_hash(out: &mut Vec<u8>, value: Option<[u8; 32]>) {
    match value {
        Some(value) => {
            out.push(1);
            out.extend_from_slice(&value);
        }
        None => out.push(0),
    }
}

fn write_optional_u8(out: &mut Vec<u8>, value: Option<u8>) {
    match value {
        Some(value) => {
            out.push(1);
            out.push(value);
        }
        None => out.push(0),
    }
}

fn write_optional_u64(out: &mut Vec<u8>, value: Option<u64>) {
    match value {
        Some(value) => {
            out.push(1);
            write_u64(out, value);
        }
        None => out.push(0),
    }
}

fn read_bool(cursor: &mut MetadataCursor<'_>, name: &str) -> Result<bool> {
    match cursor.read_u8()? {
        0 => Ok(false),
        1 => Ok(true),
        value => bail!("invalid replay metadata {name}: {value}"),
    }
}

fn read_optional_u8(cursor: &mut MetadataCursor<'_>, name: &str) -> Result<Option<u8>> {
    match cursor.read_u8()? {
        0 => Ok(None),
        1 => Ok(Some(cursor.read_u8()?)),
        value => bail!("invalid replay metadata {name} tag: {value}"),
    }
}

struct MetadataCursor<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> MetadataCursor<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    fn read_u8(&mut self) -> Result<u8> {
        let byte = *self
            .bytes
            .get(self.offset)
            .ok_or_else(|| anyhow::anyhow!("truncated replay metadata"))?;
        self.offset += 1;
        Ok(byte)
    }

    fn read_u32(&mut self) -> Result<u32> {
        let bytes = self.read_exact(4)?;
        let mut buf = [0u8; 4];
        buf.copy_from_slice(bytes);
        Ok(u32::from_le_bytes(buf))
    }

    fn read_u64(&mut self) -> Result<u64> {
        let bytes = self.read_exact(8)?;
        let mut buf = [0u8; 8];
        buf.copy_from_slice(bytes);
        Ok(u64::from_le_bytes(buf))
    }

    fn read_hash(&mut self) -> Result<[u8; 32]> {
        let bytes = self.read_exact(32)?;
        let mut hash = [0u8; 32];
        hash.copy_from_slice(bytes);
        Ok(hash)
    }

    fn read_optional_hash(&mut self) -> Result<Option<[u8; 32]>> {
        match self.read_u8()? {
            0 => Ok(None),
            1 => Ok(Some(self.read_hash()?)),
            tag => bail!("invalid optional hash tag: {tag}"),
        }
    }

    fn read_string(&mut self) -> Result<String> {
        let len = self.read_u32()? as usize;
        let bytes = self.read_exact(len)?;
        String::from_utf8(bytes.to_vec()).context("replay metadata string is not UTF-8")
    }

    fn read_optional_string(&mut self) -> Result<Option<String>> {
        match self.read_u8()? {
            0 => Ok(None),
            1 => Ok(Some(self.read_string()?)),
            tag => bail!("invalid optional string tag: {tag}"),
        }
    }

    fn read_exact(&mut self, len: usize) -> Result<&'a [u8]> {
        let end = self
            .offset
            .checked_add(len)
            .ok_or_else(|| anyhow::anyhow!("replay metadata offset overflow"))?;
        let bytes = self
            .bytes
            .get(self.offset..end)
            .ok_or_else(|| anyhow::anyhow!("truncated replay metadata"))?;
        self.offset = end;
        Ok(bytes)
    }

    fn finish(self) -> Result<()> {
        if self.is_finished() {
            Ok(())
        } else {
            bail!("trailing replay metadata bytes")
        }
    }

    fn is_finished(&self) -> bool {
        self.offset == self.bytes.len()
    }
}

#[cfg(test)]
mod tests;
