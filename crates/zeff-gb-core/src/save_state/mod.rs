use anyhow::{Result, bail};

mod bess;
mod decode;
mod encode;

pub use bess::{has_bess_footer, import_bess};
#[cfg(test)]
use decode::decode_state;
pub use decode::{decode_on_thread, validate_compatibility};
pub use encode::encode_state_bytes;
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
