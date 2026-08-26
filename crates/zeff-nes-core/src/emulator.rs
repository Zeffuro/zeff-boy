use crate::hardware::bus::Bus;
use crate::hardware::cartridge::{Cartridge, TimingMode};
use crate::hardware::cpu::Cpu;
use sha2::{Digest, Sha256};
use std::fmt;

mod public_api;
mod runtime;
mod state_io;

#[cfg(test)]
mod tests;

pub use crate::hardware::constants::CPU_CYCLES_PER_FRAME;

pub const DEFAULT_SAMPLE_RATE: f64 = 48000.0;

pub struct Emulator {
    pub(crate) cpu: Cpu,
    pub(crate) bus: Bus,
    pub(crate) rom_hash: [u8; 32],
    pub(crate) rom_crc32: u32,
    pub(crate) opcode_log: crate::debug::OpcodeLog,
    pub(crate) instruction_trace: zeff_emu_common::debug::InstructionTraceStore,
    pub(crate) call_stack: Vec<crate::debug::CallStackEntry>,
    pub(crate) debug: crate::debug::DebugController,
}

impl Emulator {
    pub fn new(rom_data: &[u8], sample_rate: f64) -> anyhow::Result<Self> {
        let cartridge = Cartridge::load(rom_data)?;
        let rom_hash: [u8; 32] = Sha256::digest(rom_data).into();
        let rom_crc32 = crc32fast::hash(rom_data);
        Self::from_cartridge(cartridge, rom_hash, rom_crc32, sample_rate)
    }

    pub fn new_fds(
        fds_image_data: &[u8],
        bios_data: Vec<u8>,
        sample_rate: f64,
    ) -> anyhow::Result<Self> {
        let image = crate::hardware::cartridge::mappers::FdsImage::parse(fds_image_data)?;
        let mut hasher = Sha256::new();
        for side in image.sides() {
            hasher.update(side);
        }
        let rom_hash = hasher.finalize().into();
        let cartridge = Cartridge::load_fds(image, bios_data)?;
        let rom_crc32 = cartridge.rom_crc32();
        Self::from_cartridge(cartridge, rom_hash, rom_crc32, sample_rate)
    }

    fn from_cartridge(
        cartridge: Cartridge,
        rom_hash: [u8; 32],
        rom_crc32: u32,
        sample_rate: f64,
    ) -> anyhow::Result<Self> {
        match cartridge.header().timing {
            TimingMode::Pal => anyhow::bail!(
                "NES PAL timing is not supported yet; this core currently emulates NTSC timing only"
            ),
            TimingMode::Dendy => anyhow::bail!(
                "NES Dendy timing is not supported yet; this core currently emulates NTSC timing only"
            ),
            TimingMode::Ntsc | TimingMode::MultiRegion => {}
        }
        let bus = Bus::new(cartridge, sample_rate);

        let mut emu = Self {
            cpu: Cpu::new(),
            bus,
            rom_hash,
            rom_crc32,
            opcode_log: crate::debug::OpcodeLog::new(),
            instruction_trace: zeff_emu_common::debug::InstructionTraceStore::default(),
            call_stack: Vec::new(),
            debug: crate::debug::DebugController::new(),
        };
        emu.cpu.power_on(&mut emu.bus);
        Ok(emu)
    }

    pub fn from_rom_data(rom_data: &[u8]) -> anyhow::Result<Self> {
        Self::new(rom_data, DEFAULT_SAMPLE_RATE)
    }

    pub fn reset(&mut self) {
        self.bus.reset();
        self.cpu.reset(&mut self.bus);
        self.opcode_log.clear();
        self.instruction_trace.clear();
        self.call_stack.clear();
        self.debug.clear_hits();
    }
}

impl fmt::Debug for Emulator {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NES Emulator")
            .field("cpu", &self.cpu)
            .field("bus", &self.bus)
            .field("opcode_log", &self.opcode_log)
            .field("debug", &self.debug)
            .finish_non_exhaustive()
    }
}
