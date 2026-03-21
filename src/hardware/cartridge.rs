use crate::hardware::rom_header::RomHeader;
use crate::hardware::types::CartridgeType;

pub(crate) enum Cartridge {
    RomOnly(RomOnly),
    Mbc1(Mbc1),
    // Future expansion: Mbc3, Mbc5, etc.
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
            _ => {
                log::warn!("Unsupported MBC: {:?}. Defaulting to MBC1 to attempt execution.", header.cartridge_type);
                Cartridge::Mbc1(Mbc1::new(rom, header.ram_size.size_bytes()))
            }
        }
    }

    pub(crate) fn read_rom(&self, addr: u16) -> u8 {
        match self {
            Cartridge::RomOnly(c) => c.read_rom(addr),
            Cartridge::Mbc1(c) => c.read_rom(addr),
        }
    }

    pub(crate) fn write_rom(&mut self, addr: u16, value: u8) {
        match self {
            Cartridge::RomOnly(c) => c.write_rom(addr, value),
            Cartridge::Mbc1(c) => c.write_rom(addr, value),
        }
    }

    pub(crate) fn read_ram(&self, addr: u16) -> u8 {
        match self {
            Cartridge::RomOnly(c) => c.read_ram(addr),
            Cartridge::Mbc1(c) => c.read_ram(addr),
        }
    }

    pub(crate) fn write_ram(&mut self, addr: u16, value: u8) {
        match self {
            Cartridge::RomOnly(c) => c.write_ram(addr, value),
            Cartridge::Mbc1(c) => c.write_ram(addr, value),
        }
    }
}

// --- ROM Only ---
pub(crate) struct RomOnly {
    rom: Vec<u8>,
    ram: Vec<u8>,
}

impl RomOnly {
    pub(crate) fn new(rom: Vec<u8>, ram_size: usize) -> Self {
        Self { rom, ram: vec![0; ram_size] }
    }

    pub(crate) fn read_rom(&self, addr: u16) -> u8 {
        self.rom.get(addr as usize).copied().unwrap_or(0xFF)
    }

    pub(crate) fn write_rom(&mut self, _addr: u16, _value: u8) {}

    pub(crate) fn read_ram(&self, addr: u16) -> u8 {
        self.ram.get((addr - 0xA000) as usize).copied().unwrap_or(0xFF)
    }

    pub(crate) fn write_ram(&mut self, addr: u16, value: u8) {
        if let Some(byte) = self.ram.get_mut((addr - 0xA000) as usize) {
            *byte = value;
        }
    }
}

// --- MBC1 ---
pub(crate) struct Mbc1 {
    rom: Vec<u8>,
    ram: Vec<u8>,
    ram_enable: bool,
    rom_bank: usize,
    ram_bank: usize,
    banking_mode: bool,
    rom_bank_mask: usize,
}

impl Mbc1 {
    pub(crate) fn new(rom: Vec<u8>, ram_size: usize) -> Self {
        // Calculate the bitmask for the ROM banks (must be power of 2 minus 1)
        let num_banks = (rom.len() / 0x4000).max(1);
        let rom_bank_mask = num_banks.next_power_of_two() - 1;

        Self {
            rom,
            ram: vec![0; ram_size],
            ram_enable: false,
            rom_bank: 1,
            ram_bank: 0,
            banking_mode: false,
            rom_bank_mask,
        }
    }

    pub(crate) fn read_rom(&self, addr: u16) -> u8 {
        let bank = match addr {
            0x0000..=0x3FFF => {
                // Bank 0 only shifts if banking mode is active
                if self.banking_mode { self.ram_bank << 5 } else { 0 }
            }
            0x4000..=0x7FFF => {
                let b = if self.rom_bank == 0 { 1 } else { self.rom_bank };
                b | (self.ram_bank << 5)
            }
            _ => 0,
        };

        let physical_bank = bank & self.rom_bank_mask;
        let physical_addr = (physical_bank * 0x4000) | ((addr & 0x3FFF) as usize);
        self.rom.get(physical_addr).copied().unwrap_or(0xFF)
    }

    pub(crate) fn write_rom(&mut self, addr: u16, value: u8) {
        match addr {
            0x0000..=0x1FFF => self.ram_enable = (value & 0x0F) == 0x0A,
            0x2000..=0x3FFF => self.rom_bank = (value & 0x1F) as usize,
            0x4000..=0x5FFF => self.ram_bank = (value & 0x03) as usize,
            0x6000..=0x7FFF => self.banking_mode = (value & 0x01) != 0,
            _ => {}
        }
    }

    pub(crate) fn read_ram(&self, addr: u16) -> u8 {
        if !self.ram_enable || self.ram.is_empty() { return 0xFF; }

        let bank = if self.banking_mode { self.ram_bank } else { 0 };
        let offset = (addr - 0xA000) as usize;
        let physical_addr = (bank * 0x2000) | offset;
        self.ram.get(physical_addr).copied().unwrap_or(0xFF)
    }

    pub(crate) fn write_ram(&mut self, addr: u16, value: u8) {
        if !self.ram_enable || self.ram.is_empty() { return; }

        let bank = if self.banking_mode { self.ram_bank } else { 0 };
        let offset = (addr - 0xA000) as usize;
        let physical_addr = (bank * 0x2000) | offset;

        if let Some(byte) = self.ram.get_mut(physical_addr) {
            *byte = value;
        }
    }
}