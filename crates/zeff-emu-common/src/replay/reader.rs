use std::fs::File;
use std::io::Read;
use std::path::Path;

use anyhow::{Context, Result, bail};

use super::validation::pad_frames_to_metadata_events;
use super::{
    CAMERA_REPEAT_SENTINEL, FRAME_FIXED_BYTES, LEGACY_GB_SAVE_STATE_MAGIC,
    LEGACY_NES_SAVE_STATE_MAGIC, MAGIC, MAX_REPLAY_CAMERA_FRAME_BYTES, ReplayEvent,
    ReplayGameBoyLinkEvent, ReplayJoypadFrame, ReplayMetadata, ReplayWonderSwanLinkEvent,
    ReplayZapperFrame, VERSION,
};

pub struct ReplayPlayer {
    save_state: Vec<u8>,
    frames: Vec<ReplayJoypadFrame>,
    cursor: usize,
    metadata: ReplayMetadata,
    event_cursor: usize,
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
        .as_chunks::<2>()
        .0
        .iter()
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
