use std::path::Path;

use sha2::{Digest, Sha256};
use zeff_emu_common::debug::{AddressDebugController, OpcodeLog};

use crate::hardware::bus::Bus;
use crate::hardware::cartridge::{Cartridge, Sega8System, SystemHint};
use crate::hardware::constants::{
    GG_SCREEN_H, GG_SCREEN_W, RGBA_CHANNELS, SMS_SCREEN_H, SMS_SCREEN_W,
};
use crate::hardware::cpu::Cpu;

mod public_api;
mod runtime;
mod state_io;

#[cfg(test)]
mod tests;

pub const DEFAULT_SAMPLE_RATE: u32 = 48_000;
const HOST_DPAD_RIGHT: u8 = 1 << 0;
const HOST_DPAD_LEFT: u8 = 1 << 1;
const HOST_DPAD_UP: u8 = 1 << 2;
const HOST_DPAD_DOWN: u8 = 1 << 3;
const HOST_BUTTON_1: u8 = 1 << 0;
const HOST_BUTTON_2: u8 = 1 << 1;
const SMS_PAD_UP: u8 = 1 << 0;
const SMS_PAD_DOWN: u8 = 1 << 1;
const SMS_PAD_LEFT: u8 = 1 << 2;
const SMS_PAD_RIGHT: u8 = 1 << 3;
const SMS_PAD_BUTTON_1: u8 = 1 << 4;
const SMS_PAD_BUTTON_2: u8 = 1 << 5;

#[derive(Debug)]
pub struct Emulator {
    pub(crate) cpu: Cpu,
    pub(crate) bus: Bus,
    pub(crate) rom_hash: [u8; 32],
    pub(crate) frame_count: u64,
    pub(crate) framebuffer: Vec<u8>,
    pub(crate) sample_rate: u32,
    pub(crate) debug: AddressDebugController,
    pub(crate) opcode_log: OpcodeLog<(u16, u8, u32)>,
}

impl Emulator {
    pub fn new(rom_data: &[u8], sample_rate: u32) -> anyhow::Result<Self> {
        Self::new_with_hint(rom_data, sample_rate, SystemHint::Auto)
    }

    pub fn new_with_path_hint(
        rom_data: &[u8],
        sample_rate: u32,
        path: &Path,
    ) -> anyhow::Result<Self> {
        let hint = SystemHint::from_path(path).unwrap_or(SystemHint::Auto);
        Self::new_with_hint(rom_data, sample_rate, hint)
    }

    pub fn new_with_hint(
        rom_data: &[u8],
        sample_rate: u32,
        hint: SystemHint,
    ) -> anyhow::Result<Self> {
        let sample_rate = if sample_rate == 0 {
            DEFAULT_SAMPLE_RATE
        } else {
            sample_rate
        };
        let cartridge = Cartridge::load_with_hint(rom_data, hint)?;
        let rom_hash: [u8; 32] = Sha256::digest(rom_data).into();
        let framebuffer = vec![0; framebuffer_len(cartridge.system())];
        Ok(Self {
            cpu: Cpu::new(),
            bus: Bus::new_with_sample_rate(cartridge, sample_rate),
            rom_hash,
            frame_count: 0,
            framebuffer,
            sample_rate,
            debug: AddressDebugController::new(),
            opcode_log: OpcodeLog::new(),
        })
    }

    pub fn from_rom_data(rom_data: &[u8]) -> anyhow::Result<Self> {
        Self::new(rom_data, DEFAULT_SAMPLE_RATE)
    }

    pub fn reset(&mut self) {
        self.cpu.reset();
        self.bus.reset();
        self.frame_count = 0;
        self.framebuffer.fill(0);
        self.debug.clear_hits();
        self.opcode_log.clear();
    }
}

fn dimensions_for_system(system: Sega8System) -> (usize, usize) {
    match system {
        Sega8System::GameGear => (GG_SCREEN_W, GG_SCREEN_H),
        Sega8System::MasterSystem | Sega8System::Sg1000 => (SMS_SCREEN_W, SMS_SCREEN_H),
    }
}

fn framebuffer_len(system: Sega8System) -> usize {
    let (w, h) = dimensions_for_system(system);
    w * h * RGBA_CHANNELS
}
