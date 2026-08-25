use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::PathBuf;

use anyhow::{Context, Result, bail};

use super::validation::pad_frames_to_metadata_events;
use super::{
    CAMERA_REPEAT_SENTINEL, MAGIC, ReplayCheckpoint, ReplayEvent, ReplayJoypadFrame,
    ReplayMetadata, VERSION,
};
use crate::media::MediaEvent;

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

    pub fn record_media_event(&mut self, frame: u64, event: MediaEvent) {
        let sequence = self
            .metadata
            .events
            .iter()
            .filter_map(|candidate| match candidate {
                ReplayEvent::Media {
                    frame: candidate_frame,
                    sequence,
                    ..
                } if *candidate_frame == frame => Some(*sequence),
                _ => None,
            })
            .max()
            .map_or(0, |sequence| sequence.saturating_add(1));
        self.record_event(ReplayEvent::Media {
            frame,
            sequence,
            event,
        });
    }

    pub fn set_final_state_sha256(&mut self, hash: [u8; 32]) {
        self.metadata.final_state_sha256 = Some(hash);
    }

    pub fn record_checkpoint(&mut self, frame: u64, state_sha256: [u8; 32]) {
        self.metadata.checkpoints.push(ReplayCheckpoint {
            frame,
            state_sha256,
        });
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
                frame.buttons_p3,
                frame.dpad_p3,
                frame.buttons_p4,
                frame.dpad_p4,
                frame.buttons_p5,
                frame.dpad_p5,
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
