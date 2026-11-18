use super::*;
use crate::constants::MAX_STATE_BYTES;
use crate::hardware::audio::MAX_FRONTEND_AUDIO_BYTES;
use anyhow::{Result, ensure};
use zeff_emu_common::save_state::{StateReader, StateWriter};

const MAGIC: &[u8; 8] = b"ZMDFRONT";
const VERSION: u32 = 1;
pub const MAX_FRONTEND_STATE_BYTES: usize = MAX_STATE_BYTES + 20 + MAX_FRONTEND_AUDIO_BYTES;

impl Emulator {
    pub fn encode_frontend_state(&self) -> Result<Vec<u8>> {
        let core = self.encode_state()?;
        let mut audio = StateWriter::new();
        self.bus.audio.write_frontend_state(&mut audio)?;
        let audio = audio.into_bytes();
        ensure!(
            audio.len() <= MAX_FRONTEND_AUDIO_BYTES,
            "MD frontend audio exceeds size bound"
        );
        let mut writer = StateWriter::with_capacity(20 + core.len() + audio.len());
        writer.write_bytes(MAGIC);
        writer.write_u32(VERSION);
        writer.write_vec(&core);
        writer.write_vec(&audio);
        Ok(writer.into_bytes())
    }

    pub fn decode_frontend_state(&mut self, bytes: &[u8]) -> Result<()> {
        if bytes.starts_with(crate::save_state::STATE_MAGIC) {
            return self.decode_state(bytes);
        }
        ensure!(
            bytes.len() <= MAX_FRONTEND_STATE_BYTES,
            "MD frontend state exceeds size bound"
        );
        let mut reader = StateReader::new(bytes);
        let mut magic = [0; 8];
        reader.read_exact(&mut magic)?;
        ensure!(
            magic == *MAGIC && reader.read_u32()? == VERSION,
            "unsupported MD frontend state format"
        );
        let core = reader.read_slice(MAX_STATE_BYTES)?;
        let audio = reader.read_slice(MAX_FRONTEND_AUDIO_BYTES)?;
        ensure!(reader.is_exhausted(), "trailing MD frontend state data");
        let mut candidate = self.clone();
        candidate.decode_state(core)?;
        candidate
            .bus
            .audio
            .read_frontend_state(&mut StateReader::new(audio))?;
        *self = candidate;
        Ok(())
    }
}

#[cfg(test)]
mod tests;
