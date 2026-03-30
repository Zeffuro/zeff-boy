use super::CartridgeDebugInfo;
use super::{
    MAX_SAVE_RAM, build_debug_info, is_ram_enable, load_ram_into, read_banked_ram, read_banked_rom,
    read_fixed_rom, write_banked_ram,
};
use crate::save_state::{StateReader, StateWriter, StateWriterGbExt};
use anyhow::Result;

mod sensor;

use sensor::{CAMERA_PIXELS, SENSOR_REG_COUNT};

const SENSOR_BANK: usize = 0x10;

const CAMERA_RAM_SIZE: usize = 16 * 0x2000;

pub struct PocketCamera {
    rom: Vec<u8>,
    ram: Vec<u8>,
    ram_enable: bool,
    rom_bank: usize,
    ram_bank: usize,

    sensor_regs: [u8; SENSOR_REG_COUNT],
    capture_active: bool,
    capture_cycles_remaining: u64,
    host_frame: Vec<u8>,
}

impl PocketCamera {
    pub fn new(rom: Vec<u8>, ram_size: usize) -> Self {
        let actual_ram = if ram_size == 0 {
            CAMERA_RAM_SIZE
        } else {
            ram_size
        };

        Self {
            rom,
            ram: vec![0; actual_ram],
            ram_enable: false,
            rom_bank: 1,
            ram_bank: 0,
            sensor_regs: [0; SENSOR_REG_COUNT],
            capture_active: false,
            capture_cycles_remaining: 0,
            host_frame: vec![0xFF; CAMERA_PIXELS],
        }
    }

    pub fn read_rom(&self, addr: u16) -> u8 {
        match addr {
            0x0000..=0x3FFF => read_fixed_rom(&self.rom, addr),
            0x4000..=0x7FFF => read_banked_rom(&self.rom, self.rom_bank, addr),
            _ => 0xFF,
        }
    }

    pub fn write_rom(&mut self, addr: u16, value: u8) {
        match addr {
            0x0000..=0x1FFF => self.ram_enable = is_ram_enable(value),
            0x2000..=0x2FFF => self.rom_bank = (self.rom_bank & 0x100) | value as usize,
            0x3000..=0x3FFF => {
                self.rom_bank = (self.rom_bank & 0x0FF) | (((value & 0x01) as usize) << 8);
            }
            0x4000..=0x5FFF => self.ram_bank = (value & 0x1F) as usize,
            _ => {}
        }
    }

    pub fn read_ram(&self, addr: u16) -> u8 {
        if self.ram_bank == SENSOR_BANK {
            let offset = (addr as usize).wrapping_sub(0xA000);
            return self.read_sensor_reg(offset);
        }

        if self.ram_bank < 0x10 {
            return read_banked_ram(&self.ram, self.ram_bank, addr);
        }

        0xFF
    }

    pub fn write_ram(&mut self, addr: u16, value: u8) {
        if self.ram_bank == SENSOR_BANK {
            let offset = (addr as usize).wrapping_sub(0xA000);
            self.write_sensor_reg(offset, value);
            return;
        }

        if self.ram_bank < 0x10 {
            write_banked_ram(&mut self.ram, self.ram_bank, addr, value);
        }
    }

    pub fn step(&mut self, t_cycles: u64) {
        self.tick_capture(t_cycles);
    }

    pub fn set_host_frame(&mut self, frame: &[u8]) {
        let copy_len = self.host_frame.len().min(frame.len());
        self.host_frame[..copy_len].copy_from_slice(&frame[..copy_len]);
    }

    pub fn rom_bytes(&self) -> &[u8] {
        &self.rom
    }

    pub fn debug_info(&self) -> CartridgeDebugInfo {
        build_debug_info("PocketCamera", self.rom_bank, self.ram_bank, self.ram_enable, None)
    }

    pub fn restore_rom_bytes(&mut self, rom: Vec<u8>) {
        self.rom = rom;
    }

    pub(super) fn ram_bytes(&self) -> &[u8] {
        &self.ram
    }

    pub(super) fn load_ram_bytes(&mut self, bytes: &[u8]) {
        load_ram_into(&mut self.ram, bytes);
    }

    pub(super) fn write_state(&self, writer: &mut StateWriter) {
        writer.write_len(self.ram.len());
        writer.write_bytes(&self.ram);
        writer.write_bool(self.ram_enable);
        writer.write_u64(self.rom_bank as u64);
        writer.write_u64(self.ram_bank as u64);
        writer.write_bytes(&self.sensor_regs);
        writer.write_bool(self.capture_active);
        writer.write_u64(self.capture_cycles_remaining);
    }

    pub(super) fn read_state(reader: &mut StateReader<'_>) -> Result<Self> {
        let ram = reader.read_vec(MAX_SAVE_RAM)?;
        let ram_enable = reader.read_bool()?;
        let rom_bank = reader.read_u64()? as usize;
        let ram_bank = reader.read_u64()? as usize;
        let mut sensor_regs = [0u8; SENSOR_REG_COUNT];
        reader.read_exact(&mut sensor_regs)?;
        let capture_active = reader.read_bool()?;
        let capture_cycles_remaining = reader.read_u64()?;

        Ok(Self {
            rom: Vec::new(),
            ram,
            ram_enable,
            rom_bank,
            ram_bank,
            sensor_regs,
            capture_active,
            capture_cycles_remaining,
            host_frame: vec![0xFF; CAMERA_PIXELS],
        })
    }

    pub(super) fn bess_mbc_writes(&self) -> Vec<(u16, u8)> {
        vec![
            (0x0000, if self.ram_enable { 0x0A } else { 0x00 }),
            (0x2000, (self.rom_bank & 0xFF) as u8),
            (0x3000, ((self.rom_bank >> 8) & 0x01) as u8),
            (0x4000, self.ram_bank as u8),
        ]
    }
}


#[cfg(test)]
mod tests;

