use std::fs::File;
use std::io::Read;
use std::path::Path;

use anyhow::{Context, Result, bail};

use super::validation::pad_frames_to_metadata_events;
use super::{
    CAMERA_REPEAT_SENTINEL, FRAME_FIXED_BYTES, LEGACY_GB_SAVE_STATE_MAGIC,
    LEGACY_NES_SAVE_STATE_MAGIC, MAGIC, MAX_REPLAY_CAMERA_FRAME_BYTES, ReplayColecoControllerFrame,
    ReplayEvent, ReplayGameBoyLinkEvent, ReplayJoypadFrame, ReplayMetadata,
    ReplayWonderSwanLinkEvent, ReplayZapperFrame, V1_FRAME_FIXED_BYTES, V2_FRAME_FIXED_BYTES,
    V2_VERSION, VERSION,
};

pub struct ReplayPlayer {
    save_state: Vec<u8>,
    frames: Vec<ReplayJoypadFrame>,
    cursor: usize,
    metadata: ReplayMetadata,
    event_cursor: usize,
    version: u32,
}

#[derive(Clone, Copy, Debug)]
pub struct ReplayLoadLimits {
    pub max_file_bytes: u64,
    pub max_metadata_bytes: usize,
    pub max_state_bytes: usize,
    pub max_frames: usize,
    pub max_decoded_camera_bytes: usize,
}

impl ReplayPlayer {
    pub fn load(path: &Path) -> Result<Self> {
        Self::load_with_limits(path, None)
    }

    pub fn load_bounded(path: &Path, limits: ReplayLoadLimits) -> Result<Self> {
        let file = File::open(path)
            .with_context(|| format!("failed to open replay file: {}", path.display()))?;
        if file.metadata()?.len() > limits.max_file_bytes {
            bail!("replay file exceeds its bounded load limit");
        }
        let read_limit = limits
            .max_file_bytes
            .checked_add(1)
            .ok_or_else(|| anyhow::anyhow!("bounded replay file limit overflow"))?;
        let mut bytes = Vec::new();
        file.take(read_limit).read_to_end(&mut bytes)?;
        if bytes.len() as u64 > limits.max_file_bytes {
            bail!("replay file exceeds its bounded load limit");
        }
        Self::decode(
            std::io::Cursor::new(bytes),
            &path.display().to_string(),
            Some(limits),
        )
    }

    fn load_with_limits(path: &Path, limits: Option<ReplayLoadLimits>) -> Result<Self> {
        let file = File::open(path)
            .with_context(|| format!("failed to open replay file: {}", path.display()))?;
        if let Some(limits) = limits
            && file.metadata()?.len() > limits.max_file_bytes
        {
            bail!("replay file exceeds its bounded load limit");
        }
        Self::decode(file, &path.display().to_string(), limits)
    }

    pub fn decode_bounded(bytes: &[u8], limits: ReplayLoadLimits) -> Result<Self> {
        if bytes.len() as u64 > limits.max_file_bytes {
            bail!("replay file exceeds its bounded load limit");
        }
        Self::decode(std::io::Cursor::new(bytes), "memory", Some(limits))
    }

    fn decode(mut file: impl Read, source: &str, limits: Option<ReplayLoadLimits>) -> Result<Self> {
        let mut magic = [0u8; 4];
        file.read_exact(&mut magic)?;
        if &magic != MAGIC {
            bail!("not a valid replay file");
        }

        let mut version_buf = [0u8; 4];
        file.read_exact(&mut version_buf)?;
        let version = u32::from_le_bytes(version_buf);
        if !matches!(version, 1 | V2_VERSION | VERSION) {
            bail!("unsupported replay version: {version}");
        }

        let mut first_len_buf = [0u8; 4];
        file.read_exact(&mut first_len_buf)?;
        let first_len = u32::from_le_bytes(first_len_buf) as usize;
        if let Some(limits) = limits
            && first_len > limits.max_metadata_bytes.max(limits.max_state_bytes)
        {
            bail!("replay first block exceeds its bounded load limit");
        }
        let mut first_block = vec![0u8; first_len];
        file.read_exact(&mut first_block)?;

        if legacy_v1_save_state_block(&first_block) {
            if limits.is_some_and(|limits| first_len > limits.max_state_bytes) {
                bail!("replay save state exceeds its bounded load limit");
            }
            let mut input_data = Vec::new();
            file.read_to_end(&mut input_data)?;
            let frames =
                decode_legacy_v1_input_frames(&input_data, limits.map(|limits| limits.max_frames))?;

            log::info!(
                "Loaded legacy replay: {} frames from {}",
                frames.len(),
                source
            );

            return Ok(Self {
                save_state: first_block,
                frames,
                cursor: 0,
                metadata: ReplayMetadata::default(),
                event_cursor: 0,
                version: 1,
            });
        }

        if limits.is_some_and(|limits| first_len > limits.max_metadata_bytes) {
            bail!("replay metadata exceeds its bounded load limit");
        }

        let metadata = ReplayMetadata::decode(&first_block).context("invalid replay metadata")?;
        let required_frames = limits
            .map(|limits| validate_bounded_metadata_frames(&metadata, limits.max_frames))
            .transpose()?;

        let mut state_len_buf = [0u8; 4];
        file.read_exact(&mut state_len_buf)?;
        let state_len = u32::from_le_bytes(state_len_buf) as usize;
        if limits.is_some_and(|limits| state_len > limits.max_state_bytes) {
            bail!("replay save state exceeds its bounded load limit");
        }

        let mut save_state = vec![0u8; state_len];
        file.read_exact(&mut save_state)?;

        let mut input_data = Vec::new();
        file.read_to_end(&mut input_data)?;
        let mut frames = decode_input_frames(&input_data, version, limits)?;
        if let (Some(limits), Some(required_frames)) = (limits, required_frames) {
            validate_bounded_padding(&frames, required_frames, limits.max_decoded_camera_bytes)?;
        }
        pad_frames_to_metadata_events(&mut frames, &metadata);

        log::info!("Loaded replay: {} frames from {}", frames.len(), source);

        Ok(Self {
            save_state,
            frames,
            cursor: 0,
            metadata,
            event_cursor: 0,
            version,
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

    pub fn version(&self) -> u32 {
        self.version
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

    pub fn uses_coleco_input(&self) -> bool {
        self.frames.iter().any(ReplayJoypadFrame::uses_coleco_input)
    }

    pub fn uses_non_coleco_input(&self) -> bool {
        self.frames
            .iter()
            .any(ReplayJoypadFrame::uses_non_coleco_input)
    }

    pub fn uses_game_boy_link_events(&self) -> bool {
        self.metadata.events.iter().any(|event| {
            matches!(
                event,
                ReplayEvent::GameBoyLink { .. }
                    | ReplayEvent::GameBoyLinkState { .. }
                    | ReplayEvent::GameBoyLinkStateAtTick { .. }
            )
        })
    }

    pub fn uses_game_boy_link(&self) -> bool {
        self.metadata.game_boy_link_start_state.is_some()
            || self
                .metadata
                .game_boy_link_coordinator_start_state
                .is_some()
            || self.uses_game_boy_link_events()
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

fn decode_input_frames(
    input_data: &[u8],
    version: u32,
    limits: Option<ReplayLoadLimits>,
) -> Result<Vec<ReplayJoypadFrame>> {
    if input_data.len() < 4 {
        bail!("replay input stream is missing frame count");
    }
    let frame_count =
        u32::from_le_bytes([input_data[0], input_data[1], input_data[2], input_data[3]]) as usize;
    if limits.is_some_and(|limits| frame_count > limits.max_frames) {
        bail!("replay frame count exceeds its bounded load limit");
    }

    let mut frames = Vec::with_capacity(frame_count);
    let mut offset = 4usize;
    let mut previous_camera_frame: Option<Vec<u8>> = None;
    let mut decoded_camera_bytes = 0usize;
    for frame_index in 0..frame_count {
        let fixed_len = match version {
            VERSION => FRAME_FIXED_BYTES,
            V2_VERSION => V2_FRAME_FIXED_BYTES,
            _ => V1_FRAME_FIXED_BYTES,
        };
        let chunk = read_replay_input_exact(input_data, &mut offset, fixed_len)
            .with_context(|| format!("truncated replay input frame {frame_index}"))?;
        let player_offset = if version >= V2_VERSION { 6 } else { 0 };
        if chunk[17 + player_offset] != 0 {
            bail!(
                "invalid replay frame reserved byte: {:#04X}",
                chunk[17 + player_offset]
            );
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
        if let Some(camera_frame) = &camera_frame {
            decoded_camera_bytes = decoded_camera_bytes
                .checked_add(camera_frame.len())
                .ok_or_else(|| anyhow::anyhow!("decoded replay camera size overflow"))?;
            if limits.is_some_and(|limits| decoded_camera_bytes > limits.max_decoded_camera_bytes) {
                bail!("decoded replay camera data exceeds its bounded load limit");
            }
        }
        frames.push(ReplayJoypadFrame {
            buttons: chunk[0],
            dpad: chunk[1],
            buttons_p2: chunk[2],
            dpad_p2: chunk[3],
            buttons_p3: if version >= V2_VERSION { chunk[4] } else { 0 },
            dpad_p3: if version >= V2_VERSION { chunk[5] } else { 0 },
            buttons_p4: if version >= V2_VERSION { chunk[6] } else { 0 },
            dpad_p4: if version >= V2_VERSION { chunk[7] } else { 0 },
            buttons_p5: if version >= V2_VERSION { chunk[8] } else { 0 },
            dpad_p5: if version >= V2_VERSION { chunk[9] } else { 0 },
            zapper: ReplayZapperFrame::from_parts(
                chunk[4 + player_offset],
                u16::from_le_bytes([chunk[5 + player_offset], chunk[6 + player_offset]]),
                u16::from_le_bytes([chunk[7 + player_offset], chunk[8 + player_offset]]),
            )?,
            host_tilt: (
                f32::from_le_bytes([
                    chunk[9 + player_offset],
                    chunk[10 + player_offset],
                    chunk[11 + player_offset],
                    chunk[12 + player_offset],
                ]),
                f32::from_le_bytes([
                    chunk[13 + player_offset],
                    chunk[14 + player_offset],
                    chunk[15 + player_offset],
                    chunk[16 + player_offset],
                ]),
            ),
            camera_frame,
            coleco: if version == VERSION {
                let packed = u32::from_le_bytes([chunk[24], chunk[25], chunk[26], 0]);
                [
                    ReplayColecoControllerFrame::from_packed((packed & 0x03FF) as u16)?,
                    ReplayColecoControllerFrame::from_packed(((packed >> 10) & 0x03FF) as u16)?,
                ]
            } else {
                [ReplayColecoControllerFrame::default(); 2]
            },
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

fn decode_legacy_v1_input_frames(
    input_data: &[u8],
    max_frames: Option<usize>,
) -> Result<Vec<ReplayJoypadFrame>> {
    if !input_data.len().is_multiple_of(2) {
        bail!(
            "legacy replay input stream has odd byte length: {}",
            input_data.len()
        );
    }
    if max_frames.is_some_and(|max_frames| input_data.len() / 2 > max_frames) {
        bail!("legacy replay frame count exceeds its bounded load limit");
    }

    Ok(input_data
        .as_chunks::<2>()
        .0
        .iter()
        .map(|chunk| ReplayJoypadFrame::p1(chunk[0], chunk[1]))
        .collect())
}

fn validate_bounded_metadata_frames(metadata: &ReplayMetadata, max_frames: usize) -> Result<usize> {
    let max_frames_u64 = u64::try_from(max_frames).unwrap_or(u64::MAX);
    let mut required_frames = 0u64;
    for event in &metadata.events {
        let required = event
            .required_frame_count()
            .ok_or_else(|| anyhow::anyhow!("replay event frame count overflow"))?;
        if required > max_frames_u64 {
            bail!("replay event exceeds the bounded frame limit");
        }
        required_frames = required_frames.max(required);
    }
    for checkpoint in &metadata.checkpoints {
        if checkpoint.frame > max_frames_u64 {
            bail!("replay checkpoint exceeds the bounded frame limit");
        }
        required_frames = required_frames.max(checkpoint.frame);
    }
    usize::try_from(required_frames).map_err(Into::into)
}

fn validate_bounded_padding(
    frames: &[ReplayJoypadFrame],
    required_frames: usize,
    max_decoded_camera_bytes: usize,
) -> Result<()> {
    if required_frames <= frames.len() {
        return Ok(());
    }
    let decoded_camera_bytes = frames.iter().try_fold(0usize, |total, frame| {
        total
            .checked_add(frame.camera_frame.as_ref().map_or(0, Vec::len))
            .ok_or_else(|| anyhow::anyhow!("decoded replay camera size overflow"))
    })?;
    let pad_camera_bytes = frames
        .last()
        .and_then(|frame| frame.camera_frame.as_ref())
        .map_or(Ok(0), |camera| {
            (required_frames - frames.len())
                .checked_mul(camera.len())
                .ok_or_else(|| anyhow::anyhow!("padded replay camera size overflow"))
        })?;
    let total_camera_bytes = decoded_camera_bytes
        .checked_add(pad_camera_bytes)
        .ok_or_else(|| anyhow::anyhow!("padded replay camera size overflow"))?;
    if total_camera_bytes > max_decoded_camera_bytes {
        bail!("padded replay camera data exceeds its bounded load limit");
    }
    Ok(())
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
