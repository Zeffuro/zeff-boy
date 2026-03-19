use crate::hardware::rom_header::RomHeader;
use anyhow::Result;

pub struct MMU {
    pub rom: Vec<u8>,
    pub header: RomHeader,
}

impl MMU {
    pub fn new(rom: Vec<u8>) -> Result<Self> {
        let header = RomHeader::from_rom(&rom)?;
        Ok(Self { rom, header })
    }

    pub fn read_byte(&self, addr: u16) -> u8 {
        self.rom[addr as usize]
    }

    pub fn write_byte(&mut self, _addr: u16, _value: u8) {

    }
}