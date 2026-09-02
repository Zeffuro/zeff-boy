use anyhow::{Result, bail, ensure};

mod bess;
mod decode;
mod encode;

pub use bess::{
    canonicalize_replay_hash_bytes, has_bess_footer, import_bess, project_replay_state_bytes,
};
#[cfg(test)]
use decode::decode_state;
pub use decode::{decode_on_thread, validate_compatibility};
pub use encode::encode_state_bytes;
pub use zeff_emu_common::save_state::{StateReader, StateWriter};

use crate::hardware::bus::Bus;
use crate::hardware::cpu::Cpu;
use crate::hardware::types::hardware_mode::{HardwareMode, HardwareModePreference};

pub const SAVE_STATE_VERSION: u32 = 1;
pub const SAVE_STATE_FORMAT_VERSION: u32 = 14;
pub const SAVE_STATE_MAGIC: [u8; 8] = *b"ZBSTATE\0";
pub const TAS_DETERMINISM_ABI_ID: &str = "zeff-gb-determinism-v1";
pub const TAS_STATE_FORMAT_COMPATIBILITY_ID: &str = "zeff-gb-native-state-v14";
#[cfg(test)]
pub(crate) const ZEFF_EXTENSION_BLOCK_LEN_FOR_TEST: u32 = bess::ZEFF_EXTENSION_BLOCK_LEN;
const SAVE_STATE_DECODE_STACK_SIZE: usize = 8 * 1024 * 1024;
const SGB_NATIVE_CONTINUATION_MAGIC: [u8; 4] = *b"SGBC";
const SGB_NATIVE_CONTINUATION_PAYLOAD_LEN: usize = 114;
const SGB_NATIVE_CONTINUATION_BLOCK_LEN: usize = 8 + SGB_NATIVE_CONTINUATION_PAYLOAD_LEN;

#[cfg(test)]
pub(crate) const SGB_NATIVE_CONTINUATION_MAGIC_FOR_TEST: [u8; 4] = SGB_NATIVE_CONTINUATION_MAGIC;

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
    pub boot_rom_enabled: bool,
    pub frame_count: Option<u64>,
    pub lcd_framebuffer: Option<Box<[u8]>>,
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
    pub boot_rom_enabled: bool,
    pub frame_count: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CurrentNativeTasStateProjection {
    pub replay_state_bytes: Vec<u8>,
    pub frame_count: u64,
    pub lcd_framebuffer: Box<[u8]>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CurrentNativeTasStateInspection {
    pub projection: CurrentNativeTasStateProjection,
    pub hardware_mode_preference: HardwareModePreference,
    pub hardware_mode: HardwareMode,
    pub boot_rom_enabled: bool,
    pub serial_device: crate::hardware::GameBoySerialDevice,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CurrentNativeTasStateIdentity {
    pub rom_sha256: [u8; 32],
    pub hardware_mode_preference: HardwareModePreference,
    pub hardware_mode: HardwareMode,
}

pub fn inspect_current_native_tas_state_identity(
    bytes: &[u8],
) -> Result<CurrentNativeTasStateIdentity> {
    ensure!(
        bytes.len() >= 12 && bytes[..8] == SAVE_STATE_MAGIC,
        "TAS requires a native GB save state"
    );
    let format_version = u32::from_le_bytes(bytes[8..12].try_into().expect("length checked"));
    ensure!(
        format_version == SAVE_STATE_FORMAT_VERSION,
        "TAS requires native GB save-state format {SAVE_STATE_FORMAT_VERSION}"
    );
    let state = decode_on_thread(bytes.to_vec())?;
    ensure!(
        state.frame_count.is_some() && state.lcd_framebuffer.is_some(),
        "native GB TAS state is missing its exact frame output"
    );
    Ok(CurrentNativeTasStateIdentity {
        rom_sha256: state.rom_hash,
        hardware_mode_preference: state.hardware_mode_preference,
        hardware_mode: state.hardware_mode,
    })
}

pub fn inspect_current_native_tas_state(
    emulator: &crate::emulator::Emulator,
    bytes: &[u8],
) -> Result<CurrentNativeTasStateInspection> {
    ensure!(
        bytes.len() >= 12 && bytes[..8] == SAVE_STATE_MAGIC,
        "TAS requires a native GB save state"
    );
    let format_version = u32::from_le_bytes(bytes[8..12].try_into().expect("length checked"));
    ensure!(
        format_version == SAVE_STATE_FORMAT_VERSION,
        "TAS requires native GB save-state format {SAVE_STATE_FORMAT_VERSION}"
    );
    let state = decode_on_thread(bytes.to_vec())?;
    validate_compatibility(&state, emulator.rom_hash())?;
    let frame_count = state
        .frame_count
        .ok_or_else(|| anyhow::anyhow!("native GB TAS state is missing its frame count"))?;
    let lcd_framebuffer = state
        .lcd_framebuffer
        .ok_or_else(|| anyhow::anyhow!("native GB TAS state is missing its LCD framebuffer"))?;
    let mut replay_state_bytes = bytes.to_vec();
    project_replay_state_bytes(&mut replay_state_bytes)?;
    Ok(CurrentNativeTasStateInspection {
        projection: CurrentNativeTasStateProjection {
            replay_state_bytes,
            frame_count,
            lcd_framebuffer,
        },
        hardware_mode_preference: state.hardware_mode_preference,
        hardware_mode: state.hardware_mode,
        boot_rom_enabled: state.boot_rom_enabled,
        serial_device: state.bus.game_boy_serial_device(),
    })
}

pub fn validate_and_load_current_native_tas_state(
    emulator: &mut crate::emulator::Emulator,
    bytes: &[u8],
) -> Result<CurrentNativeTasStateProjection> {
    let inspection = inspect_current_native_tas_state(emulator, bytes)?;
    emulator.load_state(bytes)?;
    Ok(inspection.projection)
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
