use crate::hardware::rom_header::RomHeader;
use crate::hardware::types::CartridgeType;
use crate::save_state::{StateReader, StateWriter};
use anyhow::{Result, bail};

mod huc1;
mod huc3;
mod mbc1;
mod mbc2;
mod mbc3;
mod mbc5;
mod mbc7;
mod rom_only;
mod rtc;

use huc1::HuC1;
use huc3::HuC3;
use mbc1::Mbc1;
use mbc2::Mbc2;
use mbc3::Mbc3;
use mbc5::Mbc5;
use mbc7::Mbc7;
use rom_only::RomOnly;

const RAM_ENABLE_MAGIC: u8 = 0x0A;
const ROM_BANK_SIZE: usize = 0x4000;
const RAM_BANK_SIZE: usize = 0x2000;
const RAM_BASE_ADDR: u16 = 0xA000;
const MAX_SAVE_RAM: usize = 0x20_000;

#[derive(Clone)]
pub struct CartridgeDebugInfo {
    pub mapper: &'static str,
    pub active_rom_bank: usize,
    pub active_ram_bank: usize,
    pub ram_enabled: bool,
    pub banking_mode: Option<bool>,
}

fn build_debug_info(
    mapper: &'static str,
    active_rom_bank: usize,
    active_ram_bank: usize,
    ram_enabled: bool,
    banking_mode: Option<bool>,
) -> CartridgeDebugInfo {
    CartridgeDebugInfo {
        mapper,
        active_rom_bank,
        active_ram_bank,
        ram_enabled,
        banking_mode,
    }
}

pub enum Cartridge {
    RomOnly(RomOnly),
    Mbc1(Mbc1),
    Mbc2(Mbc2),
    Mbc3(Mbc3),
    Mbc5(Mbc5),
    Mbc7(Mbc7),
    HuC1(HuC1),
    HuC3(HuC3),
}

impl Cartridge {
    pub fn new(rom: Vec<u8>, header: &RomHeader) -> Self {
        match header.cartridge_type {
            CartridgeType::RomOnly | CartridgeType::RomRam | CartridgeType::RomRamBattery => {
                Cartridge::RomOnly(RomOnly::new(rom, header.ram_size.size_bytes()))
            }
            CartridgeType::Mbc1 | CartridgeType::Mbc1Ram | CartridgeType::Mbc1RamBattery => {
                Cartridge::Mbc1(Mbc1::new(rom, header.ram_size.size_bytes()))
            }
            CartridgeType::Mbc2 | CartridgeType::Mbc2Battery => Cartridge::Mbc2(Mbc2::new(rom)),
            CartridgeType::Mbc3
            | CartridgeType::Mbc3Ram
            | CartridgeType::Mbc3RamBattery
            | CartridgeType::Mbc3TimerBattery
            | CartridgeType::Mbc3TimerRamBattery => Cartridge::Mbc3(Mbc3::new(
                rom,
                header.ram_size.size_bytes(),
                header.cartridge_type.is_mbc3_with_rtc(),
            )),
            CartridgeType::Mbc5
            | CartridgeType::Mbc5Ram
            | CartridgeType::Mbc5RamBattery
            | CartridgeType::Mbc5Rumble
            | CartridgeType::Mbc5RumbleRam
            | CartridgeType::Mbc5RumbleRamBattery => Cartridge::Mbc5(Mbc5::new(
                rom,
                header.ram_size.size_bytes(),
                header.cartridge_type.is_mbc5_with_rumble(),
            )),
            CartridgeType::Mbc7SensorRumbleRamBattery => Cartridge::Mbc7(Mbc7::new(rom)),
            CartridgeType::HuC1RamBattery => {
                Cartridge::HuC1(HuC1::new(rom, header.ram_size.size_bytes()))
            }
            CartridgeType::HuC3 => Cartridge::HuC3(HuC3::new(rom, header.ram_size.size_bytes())),
            _ => {
                log::warn!(
                    "Unsupported MBC: {:?}. Defaulting to MBC1 to attempt execution.",
                    header.cartridge_type
                );
                Cartridge::Mbc1(Mbc1::new(rom, header.ram_size.size_bytes()))
            }
        }
    }

    pub fn read_rom(&self, addr: u16) -> u8 {
        match self {
            Cartridge::RomOnly(c) => c.read_rom(addr),
            Cartridge::Mbc1(c) => c.read_rom(addr),
            Cartridge::Mbc2(c) => c.read_rom(addr),
            Cartridge::Mbc3(c) => c.read_rom(addr),
            Cartridge::Mbc5(c) => c.read_rom(addr),
            Cartridge::Mbc7(c) => c.read_rom(addr),
            Cartridge::HuC1(c) => c.read_rom(addr),
            Cartridge::HuC3(c) => c.read_rom(addr),
        }
    }

    pub fn write_rom(&mut self, addr: u16, value: u8) {
        match self {
            Cartridge::RomOnly(c) => c.write_rom(addr, value),
            Cartridge::Mbc1(c) => c.write_rom(addr, value),
            Cartridge::Mbc2(c) => c.write_rom(addr, value),
            Cartridge::Mbc3(c) => c.write_rom(addr, value),
            Cartridge::Mbc5(c) => c.write_rom(addr, value),
            Cartridge::Mbc7(c) => c.write_rom(addr, value),
            Cartridge::HuC1(c) => c.write_rom(addr, value),
            Cartridge::HuC3(c) => c.write_rom(addr, value),
        }
    }

    pub fn read_ram(&self, addr: u16) -> u8 {
        match self {
            Cartridge::RomOnly(c) => c.read_ram(addr),
            Cartridge::Mbc1(c) => c.read_ram(addr),
            Cartridge::Mbc2(c) => c.read_ram(addr),
            Cartridge::Mbc3(c) => c.read_ram(addr),
            Cartridge::Mbc5(c) => c.read_ram(addr),
            Cartridge::Mbc7(c) => c.read_ram(addr),
            Cartridge::HuC1(c) => c.read_ram(addr),
            Cartridge::HuC3(c) => c.read_ram(addr),
        }
    }

    pub fn write_ram(&mut self, addr: u16, value: u8) {
        match self {
            Cartridge::RomOnly(c) => c.write_ram(addr, value),
            Cartridge::Mbc1(c) => c.write_ram(addr, value),
            Cartridge::Mbc2(c) => c.write_ram(addr, value),
            Cartridge::Mbc3(c) => c.write_ram(addr, value),
            Cartridge::Mbc5(c) => c.write_ram(addr, value),
            Cartridge::Mbc7(c) => c.write_ram(addr, value),
            Cartridge::HuC1(c) => c.write_ram(addr, value),
            Cartridge::HuC3(c) => c.write_ram(addr, value),
        }
    }

    pub fn step(&mut self, t_cycles: u64) {
        if let Cartridge::Mbc3(c) = self { c.step(t_cycles) }
    }

    pub fn rumble_active(&self) -> bool {
        match self {
            Cartridge::Mbc5(c) => c.rumble_active(),
            Cartridge::Mbc7(c) => c.rumble_active(),
            _ => false,
        }
    }

    pub fn set_rumble_flag(&mut self, has_rumble: bool) {
        if let Cartridge::Mbc5(c) = self {
            c.set_has_rumble(has_rumble);
        }
    }

    pub fn set_mbc7_tilt(&mut self, x: f32, y: f32) {
        if let Cartridge::Mbc7(c) = self {
            c.set_host_tilt(x, y);
        }
    }

    pub fn rom_bytes(&self) -> &[u8] {
        match self {
            Cartridge::RomOnly(c) => c.rom_bytes(),
            Cartridge::Mbc1(c) => c.rom_bytes(),
            Cartridge::Mbc2(c) => c.rom_bytes(),
            Cartridge::Mbc3(c) => c.rom_bytes(),
            Cartridge::Mbc5(c) => c.rom_bytes(),
            Cartridge::Mbc7(c) => c.rom_bytes(),
            Cartridge::HuC1(c) => c.rom_bytes(),
            Cartridge::HuC3(c) => c.rom_bytes(),
        }
    }

    pub fn debug_info(&self) -> CartridgeDebugInfo {
        match self {
            Cartridge::RomOnly(c) => c.debug_info(),
            Cartridge::Mbc1(c) => c.debug_info(),
            Cartridge::Mbc2(c) => c.debug_info(),
            Cartridge::Mbc3(c) => c.debug_info(),
            Cartridge::Mbc5(c) => c.debug_info(),
            Cartridge::Mbc7(c) => c.debug_info(),
            Cartridge::HuC1(c) => c.debug_info(),
            Cartridge::HuC3(c) => c.debug_info(),
        }
    }

    pub fn restore_rom_bytes(&mut self, rom: Vec<u8>) {
        match self {
            Cartridge::RomOnly(c) => c.restore_rom_bytes(rom),
            Cartridge::Mbc1(c) => c.restore_rom_bytes(rom),
            Cartridge::Mbc2(c) => c.restore_rom_bytes(rom),
            Cartridge::Mbc3(c) => c.restore_rom_bytes(rom),
            Cartridge::Mbc5(c) => c.restore_rom_bytes(rom),
            Cartridge::Mbc7(c) => c.restore_rom_bytes(rom),
            Cartridge::HuC1(c) => c.restore_rom_bytes(rom),
            Cartridge::HuC3(c) => c.restore_rom_bytes(rom),
        }
    }

    pub fn sram_len(&self) -> usize {
        match self {
            Cartridge::RomOnly(c) => c.ram_bytes().len(),
            Cartridge::Mbc1(c) => c.ram_bytes().len(),
            Cartridge::Mbc2(c) => c.ram_bytes().len(),
            Cartridge::Mbc3(c) => c.save_len(),
            Cartridge::Mbc5(c) => c.ram_bytes().len(),
            Cartridge::Mbc7(c) => c.ram_bytes().len(),
            Cartridge::HuC1(c) => c.ram_bytes().len(),
            Cartridge::HuC3(c) => c.sram_len(),
        }
    }

    pub fn dump_sram(&self) -> Vec<u8> {
        match self {
            Cartridge::RomOnly(c) => c.ram_bytes().to_vec(),
            Cartridge::Mbc1(c) => c.ram_bytes().to_vec(),
            Cartridge::Mbc2(c) => c.ram_bytes().to_vec(),
            Cartridge::Mbc3(c) => c.dump_sram(),
            Cartridge::Mbc5(c) => c.ram_bytes().to_vec(),
            Cartridge::Mbc7(c) => c.ram_bytes().to_vec(),
            Cartridge::HuC1(c) => c.ram_bytes().to_vec(),
            Cartridge::HuC3(c) => c.dump_sram(),
        }
    }

    pub fn load_sram(&mut self, bytes: &[u8]) {
        match self {
            Cartridge::RomOnly(c) => c.load_ram_bytes(bytes),
            Cartridge::Mbc1(c) => c.load_ram_bytes(bytes),
            Cartridge::Mbc2(c) => c.load_ram_bytes(bytes),
            Cartridge::Mbc3(c) => c.load_sram(bytes),
            Cartridge::Mbc5(c) => c.load_ram_bytes(bytes),
            Cartridge::Mbc7(c) => c.load_ram_bytes(bytes),
            Cartridge::HuC1(c) => c.load_ram_bytes(bytes),
            Cartridge::HuC3(c) => c.load_sram(bytes),
        }
    }

    /// Returns a reference to the raw MBC RAM bytes (no RTC footer).
    pub fn mbc_ram_bytes(&self) -> &[u8] {
        match self {
            Cartridge::RomOnly(c) => c.ram_bytes(),
            Cartridge::Mbc1(c) => c.ram_bytes(),
            Cartridge::Mbc2(c) => c.ram_bytes(),
            Cartridge::Mbc3(c) => c.ram_bytes(),
            Cartridge::Mbc5(c) => c.ram_bytes(),
            Cartridge::Mbc7(c) => c.ram_bytes(),
            Cartridge::HuC1(c) => c.ram_bytes(),
            Cartridge::HuC3(c) => c.ram_bytes(),
        }
    }

    /// Returns BESS MBC register write pairs for the current mapper state.
    pub fn bess_mbc_writes(&self) -> Vec<(u16, u8)> {
        match self {
            Cartridge::RomOnly(_) => Vec::new(),
            Cartridge::Mbc1(c) => c.bess_mbc_writes(),
            Cartridge::Mbc2(c) => c.bess_mbc_writes(),
            Cartridge::Mbc3(c) => c.bess_mbc_writes(),
            Cartridge::Mbc5(c) => c.bess_mbc_writes(),
            Cartridge::Mbc7(c) => c.bess_mbc_writes(),
            Cartridge::HuC1(c) => c.bess_mbc_writes(),
            Cartridge::HuC3(c) => c.bess_mbc_writes(),
        }
    }

    /// Returns BESS RTC data (current, latched registers) for MBC3 carts with RTC.
    pub fn bess_rtc_data(&self) -> Option<([u8; 5], [u8; 5])> {
        match self {
            Cartridge::Mbc3(c) => c.bess_rtc_data(),
            _ => None,
        }
    }

    /// Applies BESS RTC data to the cartridge (MBC3 only).
    pub fn apply_bess_rtc(
        &mut self,
        current: [u8; 5],
        latched: [u8; 5],
        elapsed_seconds: u64,
    ) {
        if let Cartridge::Mbc3(c) = self {
            c.apply_bess_rtc(current, latched, elapsed_seconds);
        }
    }

    pub fn write_state(&self, writer: &mut StateWriter) {
        match self {
            Cartridge::RomOnly(c) => {
                writer.write_u8(0);
                c.write_state(writer);
            }
            Cartridge::Mbc1(c) => {
                writer.write_u8(1);
                c.write_state(writer);
            }
            Cartridge::Mbc2(c) => {
                writer.write_u8(2);
                c.write_state(writer);
            }
            Cartridge::Mbc3(c) => {
                writer.write_u8(3);
                c.write_state(writer);
            }
            Cartridge::Mbc5(c) => {
                writer.write_u8(4);
                c.write_state(writer);
            }
            Cartridge::Mbc7(c) => {
                writer.write_u8(5);
                c.write_state(writer);
            }
            Cartridge::HuC1(c) => {
                writer.write_u8(6);
                c.write_state(writer);
            }
            Cartridge::HuC3(c) => {
                writer.write_u8(7);
                c.write_state(writer);
            }
        }
    }

    pub fn read_state(reader: &mut StateReader<'_>) -> Result<Self> {
        let mapper_tag = reader.read_u8()?;
        match mapper_tag {
            0 => Ok(Cartridge::RomOnly(RomOnly::read_state(reader)?)),
            1 => Ok(Cartridge::Mbc1(Mbc1::read_state(reader)?)),
            2 => Ok(Cartridge::Mbc2(Mbc2::read_state(reader)?)),
            3 => Ok(Cartridge::Mbc3(Mbc3::read_state(reader)?)),
            4 => Ok(Cartridge::Mbc5(Mbc5::read_state(reader)?)),
            5 => Ok(Cartridge::Mbc7(Mbc7::read_state(reader)?)),
            6 => Ok(Cartridge::HuC1(HuC1::read_state(reader)?)),
            7 => Ok(Cartridge::HuC3(HuC3::read_state(reader)?)),
            _ => bail!("invalid cartridge mapper tag in save-state file: {mapper_tag}"),
        }
    }
}

fn read_banked_rom(rom: &[u8], bank: usize, addr: u16) -> u8 {
    let bank_count = (rom.len() / ROM_BANK_SIZE).max(1);
    let b = bank % bank_count;
    let phys = b * ROM_BANK_SIZE + (addr as usize & (ROM_BANK_SIZE - 1));
    rom.get(phys).copied().unwrap_or(0xFF)
}

fn read_fixed_rom(rom: &[u8], addr: u16) -> u8 {
    rom.get(addr as usize).copied().unwrap_or(0xFF)
}

fn read_banked_ram(ram: &[u8], bank: usize, addr: u16) -> u8 {
    let offset = (addr - RAM_BASE_ADDR) as usize;
    let physical_addr = bank * RAM_BANK_SIZE + offset;
    ram.get(physical_addr).copied().unwrap_or(0xFF)
}

fn write_banked_ram(ram: &mut [u8], bank: usize, addr: u16, value: u8) {
    let offset = (addr - RAM_BASE_ADDR) as usize;
    let physical_addr = bank * RAM_BANK_SIZE + offset;

    if let Some(byte) = ram.get_mut(physical_addr) {
        *byte = value;
    }
}

fn is_ram_enable(value: u8) -> bool {
    (value & 0x0F) == RAM_ENABLE_MAGIC
}

fn load_ram_into(ram: &mut [u8], bytes: &[u8]) {
    let copy_len = ram.len().min(bytes.len());
    ram[..copy_len].copy_from_slice(&bytes[..copy_len]);
    if copy_len < ram.len() {
        ram[copy_len..].fill(0);
    }
}
