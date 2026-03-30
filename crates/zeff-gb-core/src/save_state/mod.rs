use anyhow::{Context, Result, bail};
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

mod bess;
mod decode;
mod encode;

pub use bess::{has_bess_footer, import_bess};
pub use decode::{decode_on_thread, validate_compatibility};
pub use encode::encode_state_bytes;
#[cfg(test)]
use decode::decode_state;
pub use zeff_emu_common::save_state::{StateReader, StateWriter};

use crate::hardware::bus::Bus;
use crate::hardware::cpu::Cpu;
use crate::hardware::types::hardware_mode::{HardwareMode, HardwareModePreference};

pub const SAVE_STATE_VERSION: u32 = 1;
pub const SAVE_STATE_FORMAT_VERSION: u32 = 2;
pub const SAVE_STATE_MAGIC: [u8; 8] = *b"ZBSTATE\0";
const SAVE_STATE_DECODE_STACK_SIZE: usize = 8 * 1024 * 1024;

pub trait StateWriterGbExt {
    fn write_len(&mut self, len: usize);
    fn write_hardware_mode(&mut self, mode: HardwareMode);
}

impl StateWriterGbExt for StateWriter {
    fn write_len(&mut self, len: usize) {
        self.write_u32(len as u32);
    }

    fn write_hardware_mode(&mut self, mode: HardwareMode) {
        self.write_u8(encode_hardware_mode(mode));
    }
}

pub trait StateReaderGbExt {
    fn read_hardware_mode(&mut self) -> Result<HardwareMode>;
}

impl StateReaderGbExt for StateReader<'_> {
    fn read_hardware_mode(&mut self) -> Result<HardwareMode> {
        decode_hardware_mode(self.read_u8()?)
    }
}

pub struct SaveState {
    pub version: u32,
    pub rom_hash: [u8; 32],
    pub cpu: Cpu,
    pub bus: Bus,
    pub hardware_mode_preference: HardwareModePreference,
    pub hardware_mode: HardwareMode,
    pub cycle_count: u64,
    pub last_opcode: u8,
    pub last_opcode_pc: u16,
}

pub struct SaveStateRef<'a> {
    pub version: u32,
    pub rom_hash: [u8; 32],
    pub cpu: &'a Cpu,
    pub bus: &'a Bus,
    pub hardware_mode_preference: HardwareModePreference,
    pub hardware_mode: HardwareMode,
    pub cycle_count: u64,
    pub last_opcode: u8,
    pub last_opcode_pc: u16,
}

pub fn write_to_file(path: &Path, state: &SaveStateRef<'_>) -> Result<()> {
    let bytes = encode_state_bytes(state)?;
    write_state_bytes_to_file(path, &bytes)
}

pub fn write_state_bytes_to_file(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).context("failed to create save-state directory")?;
    }

    let tmp_path = temp_path_for(path);
    {
        let mut file = File::create(&tmp_path)
            .with_context(|| format!("failed to create temp save state: {}", tmp_path.display()))?;
        file.write_all(bytes)
            .with_context(|| format!("failed to write temp save state: {}", tmp_path.display()))?;
        file.sync_all()
            .with_context(|| format!("failed to flush temp save state: {}", tmp_path.display()))?;
    }

    if path.exists() {
        let _ = fs::remove_file(path);
    }

    fs::rename(&tmp_path, path)
        .with_context(|| format!("failed to finalize save state: {}", path.display()))
}

fn temp_path_for(path: &Path) -> PathBuf {
    let mut tmp = path.to_path_buf();
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let ext = path
        .extension()
        .and_then(|v| v.to_str())
        .unwrap_or("state");
    tmp.set_extension(format!("{ext}.tmp.{suffix}"));
    tmp
}

pub fn encode_hardware_mode(mode: HardwareMode) -> u8 {
    match mode {
        HardwareMode::DMG => 0,
        HardwareMode::SGB1 => 1,
        HardwareMode::SGB2 => 2,
        HardwareMode::CGBNormal => 3,
        HardwareMode::CGBDouble => 4,
    }
}

pub fn decode_hardware_mode(tag: u8) -> Result<HardwareMode> {
    match tag {
        0 => Ok(HardwareMode::DMG),
        1 => Ok(HardwareMode::SGB1),
        2 => Ok(HardwareMode::SGB2),
        3 => Ok(HardwareMode::CGBNormal),
        4 => Ok(HardwareMode::CGBDouble),
        _ => bail!("invalid hardware mode tag in save-state file: {tag}"),
    }
}

#[cfg(test)]
mod tests;
