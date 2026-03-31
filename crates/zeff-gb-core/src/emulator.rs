use crate::debug::{DebugController, OpcodeLog};
use crate::hardware::rom_header::RomHeader;
use crate::hardware::types::hardware_mode::{HardwareMode, HardwareModePreference};
use crate::hardware::{bus::Bus, cpu::Cpu};
use std::fmt;

mod boot_init;
mod debug_view;
mod public_api;
mod runtime;
mod state_io;

const CYCLES_PER_FRAME_NORMAL: u64 = 70224;
const CYCLES_PER_FRAME_DOUBLE: u64 = 140448;
type RegisterSeed = (u8, u8, u8, u8, u8, u8, u8, u8);

const DMG_POST_BOOT_REGISTERS: RegisterSeed = (0x01, 0xB0, 0x00, 0x13, 0x00, 0xD8, 0x01, 0x4D);
const CGB_POST_BOOT_REGISTERS: RegisterSeed = (0x11, 0x80, 0x00, 0x00, 0xFF, 0x56, 0x00, 0x0D);

pub struct Emulator {
    pub(crate) cpu: Cpu,
    pub(crate) bus: Box<Bus>,
    pub(crate) header: RomHeader,
    pub(crate) hardware_mode_preference: HardwareModePreference,
    pub(crate) hardware_mode: HardwareMode,
    pub(crate) cycle_count: u64,
    pub(crate) opcode_log: OpcodeLog,
    pub(crate) last_opcode: u8,
    pub(crate) last_opcode_pc: u16,
    pub(crate) debug: DebugController,
    pub(crate) rom_hash: [u8; 32],
}

impl fmt::Debug for Emulator {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Emulator")
            .field("cpu", &self.cpu)
            .field("bus", &self.bus)
            .field("hardware_mode", &self.hardware_mode)
            .field("hardware_mode_preference", &self.hardware_mode_preference)
            .field("cycle_count", &self.cycle_count)
            .field("last_opcode", &format_args!("{:#04X}", self.last_opcode))
            .field(
                "last_opcode_pc",
                &format_args!("{:#06X}", self.last_opcode_pc),
            )
            .field("opcode_log", &self.opcode_log)
            .field("debug", &self.debug)
            .field("title", &self.header.title)
            .finish_non_exhaustive()
    }
}
