/// File format (`.zrpl`):
/// ```text
/// [4 bytes]  magic: "ZRPL"
/// [4 bytes]  version: 1 (u32 LE)
/// [4 bytes]  save_state_length (u32 LE)
/// [N bytes]  save state data
/// [remaining] frames: each 2 bytes (buttons_pressed: u8, dpad_pressed: u8)
/// ```

use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

const MAGIC: &[u8; 4] = b"ZRPL";
const VERSION: u32 = 1;

pub(crate) struct ReplayRecorder {
    path: PathBuf,
    save_state: Vec<u8>,
    frames: Vec<(u8, u8)>,
}

impl ReplayRecorder {
    pub(crate) fn new(path: PathBuf, save_state: Vec<u8>) -> Self {
        Self {
            path,
            save_state,
            frames: Vec::with_capacity(3600),
        }
    }

    pub(crate) fn record_frame(&mut self, buttons_pressed: u8, dpad_pressed: u8) {
        self.frames.push((buttons_pressed, dpad_pressed));
    }

    pub(crate) fn finish(self) -> Result<PathBuf> {
        let mut file = File::create(&self.path)
            .with_context(|| format!("failed to create replay file: {}", self.path.display()))?;

        file.write_all(MAGIC)?;
        file.write_all(&VERSION.to_le_bytes())?;
        file.write_all(&(self.save_state.len() as u32).to_le_bytes())?;
        file.write_all(&self.save_state)?;

        for &(buttons, dpad) in &self.frames {
            file.write_all(&[buttons, dpad])?;
        }

        file.sync_all()?;
        log::info!(
            "Wrote replay: {} frames to {}",
            self.frames.len(),
            self.path.display()
        );
        Ok(self.path)
    }

    pub(crate) fn frame_count(&self) -> usize {
        self.frames.len()
    }
}

pub(crate) struct ReplayPlayer {
    save_state: Vec<u8>,
    frames: Vec<(u8, u8)>,
    cursor: usize,
}

impl ReplayPlayer {
    pub(crate) fn load(path: &Path) -> Result<Self> {
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

        let mut state_len_buf = [0u8; 4];
        file.read_exact(&mut state_len_buf)?;
        let state_len = u32::from_le_bytes(state_len_buf) as usize;

        let mut save_state = vec![0u8; state_len];
        file.read_exact(&mut save_state)?;

        let mut input_data = Vec::new();
        file.read_to_end(&mut input_data)?;

        let frames: Vec<(u8, u8)> = input_data
            .chunks_exact(2)
            .map(|chunk| (chunk[0], chunk[1]))
            .collect();

        log::info!(
            "Loaded replay: {} frames from {}",
            frames.len(),
            path.display()
        );

        Ok(Self {
            save_state,
            frames,
            cursor: 0,
        })
    }

    pub(crate) fn save_state(&self) -> &[u8] {
        &self.save_state
    }

    pub(crate) fn next_frame(&mut self) -> Option<(u8, u8)> {
        if self.cursor < self.frames.len() {
            let frame = self.frames[self.cursor];
            self.cursor += 1;
            Some(frame)
        } else {
            None
        }
    }

    pub(crate) fn remaining(&self) -> usize {
        self.frames.len().saturating_sub(self.cursor)
    }

    pub(crate) fn total_frames(&self) -> usize {
        self.frames.len()
    }

    pub(crate) fn is_finished(&self) -> bool {
        self.cursor >= self.frames.len()
    }
}

