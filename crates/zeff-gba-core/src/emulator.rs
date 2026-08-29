use crate::hardware::bus::Bus;
use crate::hardware::cartridge::Cartridge;
use crate::hardware::cpu::{Cpu, FetchedInstruction, InstructionSet};
use sha2::{Digest, Sha256};
use std::fmt;
use zeff_emu_common::debug::{AddressDebugController, OpcodeLog};

mod public_api;
mod runtime;
mod state_io;

#[cfg(test)]
mod tests;

pub const DEFAULT_SAMPLE_RATE: u32 = crate::hardware::constants::GBA_DEFAULT_HOST_SAMPLE_RATE_HZ;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GbaOpcodeRecord {
    pub pc: u32,
    pub raw: u32,
    pub instruction_set: InstructionSet,
    pub width_bytes: u8,
    pub fetch_cycles: u32,
}

impl Default for GbaOpcodeRecord {
    fn default() -> Self {
        Self {
            pc: 0,
            raw: 0,
            instruction_set: InstructionSet::Arm,
            width_bytes: 4,
            fetch_cycles: 0,
        }
    }
}

impl From<FetchedInstruction> for GbaOpcodeRecord {
    fn from(fetched: FetchedInstruction) -> Self {
        Self {
            pc: fetched.pc,
            raw: fetched.raw,
            instruction_set: fetched.instruction_set,
            width_bytes: fetched.width_bytes,
            fetch_cycles: fetched.fetch_cycles,
        }
    }
}

#[derive(Clone)]
pub struct Emulator {
    pub(crate) cpu: Cpu,
    pub(crate) bus: Bus,
    pub(crate) rom_hash: [u8; 32],
    pub(crate) frame_count: u64,
    pub(crate) debug: AddressDebugController,
    pub(crate) opcode_log: OpcodeLog<GbaOpcodeRecord>,
    pub(crate) instruction_trace: zeff_emu_common::debug::InstructionTraceStore,
}

impl Emulator {
    pub fn new(rom_data: &[u8], sample_rate: u32) -> anyhow::Result<Self> {
        Self::new_inner(rom_data, sample_rate, None)
    }

    pub fn new_with_bios(rom_data: &[u8], bios: &[u8], sample_rate: u32) -> anyhow::Result<Self> {
        Self::new_inner(rom_data, sample_rate, Some(bios))
    }

    fn new_inner(rom_data: &[u8], sample_rate: u32, bios: Option<&[u8]>) -> anyhow::Result<Self> {
        let cartridge = Cartridge::load(rom_data)?;
        let rom_hash: [u8; 32] = Sha256::digest(rom_data).into();
        let bus = match bios {
            Some(bios) => Bus::new_with_bios(cartridge, sample_rate, bios)?,
            None => Bus::new(cartridge, sample_rate),
        };
        let mut emu = Self {
            cpu: Cpu::new(),
            bus,
            rom_hash,
            frame_count: 0,
            debug: AddressDebugController::new(),
            opcode_log: OpcodeLog::new(),
            instruction_trace: zeff_emu_common::debug::InstructionTraceStore::default(),
        };
        emu.reset();
        Ok(emu)
    }

    pub fn from_rom_data(rom_data: &[u8]) -> anyhow::Result<Self> {
        Self::new(rom_data, DEFAULT_SAMPLE_RATE)
    }

    pub fn reset(&mut self) {
        if self.bus.has_external_bios() {
            self.cpu.reset_with_bios();
        } else {
            self.cpu.reset();
        }
        self.frame_count = 0;
        self.debug.clear_hits();
        self.opcode_log.clear();
        self.instruction_trace.clear();
    }
}

impl fmt::Debug for Emulator {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GBA Emulator")
            .field("cpu", &self.cpu)
            .field("debug", &self.debug)
            .field("title", &self.bus.cartridge.header().title)
            .field("backup_kind", &self.bus.cartridge.backup_kind())
            .field("frame_count", &self.frame_count)
            .finish_non_exhaustive()
    }
}
