use crate::hardware::bus::Bus;
use crate::hardware::cartridge::{Cartridge, MinimumSystem};
use crate::hardware::cpu::Cpu;
use sha2::{Digest, Sha256};
use std::fmt;
use zeff_emu_common::debug::AddressDebugController;

mod public_api;
mod runtime;
mod state_io;

#[cfg(test)]
mod tests;

pub const DEFAULT_SAMPLE_RATE: u32 = 48_000;

pub struct Emulator {
    pub(crate) cpu: Cpu,
    pub(crate) bus: Bus,
    pub(crate) rom_hash: [u8; 32],
    pub(crate) rom_crc32: u32,
    pub(crate) frame_count: u64,
    pub(crate) debug: AddressDebugController,
}

impl Emulator {
    pub fn new(rom_data: &[u8], sample_rate: u32) -> anyhow::Result<Self> {
        let cartridge = Cartridge::load(rom_data)?;
        let rom_hash: [u8; 32] = Sha256::digest(rom_data).into();
        let rom_crc32 = crc32fast::hash(rom_data);
        let mut bus = Bus::new(cartridge);
        bus.apu.set_sample_rate(sample_rate);
        let mut emu = Self {
            cpu: Cpu::new(),
            bus,
            rom_hash,
            rom_crc32,
            frame_count: 0,
            debug: AddressDebugController::new(),
        };
        emu.reset();
        Ok(emu)
    }

    pub fn from_rom_data(rom_data: &[u8]) -> anyhow::Result<Self> {
        Self::new(rom_data, DEFAULT_SAMPLE_RATE)
    }

    pub fn reset(&mut self) {
        self.bus.reset();
        self.cpu.apply_cartridge_start_state(
            self.bus.cartridge.minimum_system() != MinimumSystem::WonderSwan,
        );
        self.frame_count = 0;
        self.debug.clear_hits();
    }
}

impl fmt::Debug for Emulator {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Emulator")
            .field("pc", &format_args!("{:#07X}", self.cpu_pc()))
            .field("state", &self.cpu.state)
            .field("cycles", &self.cpu.cycles)
            .field("frame_count", &self.frame_count)
            .field("rom_crc32", &format_args!("{:#010X}", self.rom_crc32))
            .field("debug", &self.debug)
            .field("footer", &self.bus.cartridge.footer())
            .finish_non_exhaustive()
    }
}
