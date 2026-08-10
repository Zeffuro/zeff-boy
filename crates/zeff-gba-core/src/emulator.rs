use crate::hardware::bus::Bus;
use crate::hardware::cartridge::Cartridge;
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
    pub(crate) frame_count: u64,
    pub(crate) debug: AddressDebugController,
}

impl Emulator {
    pub fn new(rom_data: &[u8], sample_rate: u32) -> anyhow::Result<Self> {
        let cartridge = Cartridge::load(rom_data)?;
        let rom_hash: [u8; 32] = Sha256::digest(rom_data).into();
        let mut emu = Self {
            cpu: Cpu::new(),
            bus: Bus::new(cartridge, sample_rate),
            rom_hash,
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
        self.cpu.reset();
        self.frame_count = 0;
        self.debug.clear_hits();
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
