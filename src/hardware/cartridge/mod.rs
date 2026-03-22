use crate::hardware::rom_header::RomHeader;
use crate::hardware::types::CartridgeType;
use crate::save_state::{StateReader, StateWriter};
use anyhow::{Result, bail};

mod mbc1;
mod mbc2;
mod mbc3;
mod mbc5;
mod rom_only;

use mbc1::Mbc1;
use mbc2::Mbc2;
use mbc3::Mbc3;
use mbc5::Mbc5;
use rom_only::RomOnly;

#[derive(Clone)]
pub(crate) struct CartridgeDebugInfo {
    pub(crate) mapper: &'static str,
    pub(crate) active_rom_bank: usize,
    pub(crate) active_ram_bank: usize,
    pub(crate) ram_enabled: bool,
    pub(crate) banking_mode: Option<bool>,
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

pub(crate) enum Cartridge {
    RomOnly(RomOnly),
    Mbc1(Mbc1),
    Mbc2(Mbc2),
    Mbc3(Mbc3),
    Mbc5(Mbc5),
}

impl Cartridge {
    pub(crate) fn new(rom: Vec<u8>, header: &RomHeader) -> Self {
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
            | CartridgeType::Mbc3TimerRamBattery => {
                Cartridge::Mbc3(Mbc3::new(rom, header.ram_size.size_bytes()))
            }
            CartridgeType::Mbc5
            | CartridgeType::Mbc5Ram
            | CartridgeType::Mbc5RamBattery
            | CartridgeType::Mbc5Rumble
            | CartridgeType::Mbc5RumbleRam
            | CartridgeType::Mbc5RumbleRamBattery => {
                Cartridge::Mbc5(Mbc5::new(rom, header.ram_size.size_bytes()))
            }
            _ => {
                log::warn!(
                    "Unsupported MBC: {:?}. Defaulting to MBC1 to attempt execution.",
                    header.cartridge_type
                );
                Cartridge::Mbc1(Mbc1::new(rom, header.ram_size.size_bytes()))
            }
        }
    }

    pub(crate) fn read_rom(&self, addr: u16) -> u8 {
        match self {
            Cartridge::RomOnly(c) => c.read_rom(addr),
            Cartridge::Mbc1(c) => c.read_rom(addr),
            Cartridge::Mbc2(c) => c.read_rom(addr),
            Cartridge::Mbc3(c) => c.read_rom(addr),
            Cartridge::Mbc5(c) => c.read_rom(addr),
        }
    }

    pub(crate) fn write_rom(&mut self, addr: u16, value: u8) {
        match self {
            Cartridge::RomOnly(c) => c.write_rom(addr, value),
            Cartridge::Mbc1(c) => c.write_rom(addr, value),
            Cartridge::Mbc2(c) => c.write_rom(addr, value),
            Cartridge::Mbc3(c) => c.write_rom(addr, value),
            Cartridge::Mbc5(c) => c.write_rom(addr, value),
        }
    }

    pub(crate) fn read_ram(&self, addr: u16) -> u8 {
        match self {
            Cartridge::RomOnly(c) => c.read_ram(addr),
            Cartridge::Mbc1(c) => c.read_ram(addr),
            Cartridge::Mbc2(c) => c.read_ram(addr),
            Cartridge::Mbc3(c) => c.read_ram(addr),
            Cartridge::Mbc5(c) => c.read_ram(addr),
        }
    }

    pub(crate) fn write_ram(&mut self, addr: u16, value: u8) {
        match self {
            Cartridge::RomOnly(c) => c.write_ram(addr, value),
            Cartridge::Mbc1(c) => c.write_ram(addr, value),
            Cartridge::Mbc2(c) => c.write_ram(addr, value),
            Cartridge::Mbc3(c) => c.write_ram(addr, value),
            Cartridge::Mbc5(c) => c.write_ram(addr, value),
        }
    }

    pub(crate) fn rom_bytes(&self) -> &[u8] {
        match self {
            Cartridge::RomOnly(c) => c.rom_bytes(),
            Cartridge::Mbc1(c) => c.rom_bytes(),
            Cartridge::Mbc2(c) => c.rom_bytes(),
            Cartridge::Mbc3(c) => c.rom_bytes(),
            Cartridge::Mbc5(c) => c.rom_bytes(),
        }
    }

    pub(crate) fn debug_info(&self) -> CartridgeDebugInfo {
        match self {
            Cartridge::RomOnly(c) => c.debug_info(),
            Cartridge::Mbc1(c) => c.debug_info(),
            Cartridge::Mbc2(c) => c.debug_info(),
            Cartridge::Mbc3(c) => c.debug_info(),
            Cartridge::Mbc5(c) => c.debug_info(),
        }
    }

    pub(crate) fn restore_rom_bytes(&mut self, rom: Vec<u8>) {
        match self {
            Cartridge::RomOnly(c) => c.restore_rom_bytes(rom),
            Cartridge::Mbc1(c) => c.restore_rom_bytes(rom),
            Cartridge::Mbc2(c) => c.restore_rom_bytes(rom),
            Cartridge::Mbc3(c) => c.restore_rom_bytes(rom),
            Cartridge::Mbc5(c) => c.restore_rom_bytes(rom),
        }
    }

    pub(crate) fn sram_len(&self) -> usize {
        match self {
            Cartridge::RomOnly(c) => c.ram_bytes().len(),
            Cartridge::Mbc1(c) => c.ram_bytes().len(),
            Cartridge::Mbc2(c) => c.ram_bytes().len(),
            Cartridge::Mbc3(c) => c.ram_bytes().len(),
            Cartridge::Mbc5(c) => c.ram_bytes().len(),
        }
    }

    pub(crate) fn dump_sram(&self) -> Vec<u8> {
        match self {
            Cartridge::RomOnly(c) => c.ram_bytes().to_vec(),
            Cartridge::Mbc1(c) => c.ram_bytes().to_vec(),
            Cartridge::Mbc2(c) => c.ram_bytes().to_vec(),
            Cartridge::Mbc3(c) => c.ram_bytes().to_vec(),
            Cartridge::Mbc5(c) => c.ram_bytes().to_vec(),
        }
    }

    pub(crate) fn load_sram(&mut self, bytes: &[u8]) {
        match self {
            Cartridge::RomOnly(c) => c.load_ram_bytes(bytes),
            Cartridge::Mbc1(c) => c.load_ram_bytes(bytes),
            Cartridge::Mbc2(c) => c.load_ram_bytes(bytes),
            Cartridge::Mbc3(c) => c.load_ram_bytes(bytes),
            Cartridge::Mbc5(c) => c.load_ram_bytes(bytes),
        }
    }

    pub(crate) fn write_state(&self, writer: &mut StateWriter) {
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
        }
    }

    pub(crate) fn read_state(reader: &mut StateReader<'_>) -> Result<Self> {
        let mapper_tag = reader.read_u8()?;
        match mapper_tag {
            0 => Ok(Cartridge::RomOnly(RomOnly::read_state(reader)?)),
            1 => Ok(Cartridge::Mbc1(Mbc1::read_state(reader)?)),
            2 => Ok(Cartridge::Mbc2(Mbc2::read_state(reader)?)),
            3 => Ok(Cartridge::Mbc3(Mbc3::read_state(reader)?)),
            4 => Ok(Cartridge::Mbc5(Mbc5::read_state(reader)?)),
            _ => bail!("invalid cartridge mapper tag in save-state file: {mapper_tag}"),
        }
    }
}

fn read_banked_rom(rom: &[u8], bank: usize, addr: u16) -> u8 {
    let bank_count = (rom.len() / 0x4000).max(1);
    let b = bank % bank_count;
    let phys = b * 0x4000 + (addr as usize & 0x3FFF);
    rom.get(phys).copied().unwrap_or(0xFF)
}

fn read_fixed_rom(rom: &[u8], addr: u16) -> u8 {
    rom.get(addr as usize).copied().unwrap_or(0xFF)
}

fn read_banked_ram(ram: &[u8], bank: usize, addr: u16) -> u8 {
    let offset = (addr - 0xA000) as usize;
    let physical_addr = bank * 0x2000 + offset;
    ram.get(physical_addr).copied().unwrap_or(0xFF)
}

fn write_banked_ram(ram: &mut [u8], bank: usize, addr: u16, value: u8) {
    let offset = (addr - 0xA000) as usize;
    let physical_addr = bank * 0x2000 + offset;

    if let Some(byte) = ram.get_mut(physical_addr) {
        *byte = value;
    }
}
