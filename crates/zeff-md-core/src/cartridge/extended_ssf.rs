use anyhow::{Result, ensure};
use zeff_emu_common::save_state::{StateReader, StateWriter};

use crate::constants::MAX_BANKED_CARTRIDGE_SIZE as MAX_ROM_BYTES;
const PAGE_BYTES: usize = 0x80000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ExtendedSsf {
    rom_bytes: usize,
    page_mask: u8,
    slots: [u8; 8],
}

impl ExtendedSsf {
    pub fn detect(rom: &[u8]) -> bool {
        rom.len() > 5 * 1024 * 1024
            && rom.len() <= MAX_ROM_BYTES
            && rom.get(0x100..0x108) == Some(b"SEGA SSF")
            && rom
                .get(0x1b0..0x1bc)
                .is_some_and(|s| s.iter().all(|v| matches!(v, 0 | b' ')))
    }
    pub fn new(rom_bytes: usize) -> Self {
        let pages = rom_bytes.div_ceil(PAGE_BYTES).next_power_of_two();
        Self {
            rom_bytes,
            page_mask: (pages - 1) as u8,
            slots: [0, 1, 2, 3, 4, 5, 6, 7],
        }
    }
    pub fn matches_rom(&self, rom: &[u8]) -> bool {
        self.rom_bytes == rom.len()
            && Self::detect(rom)
            && self.slots.iter().all(|page| *page <= self.page_mask)
    }
    pub fn domain(address: u32) -> bool {
        address <= 0x7fffff || (0xa13000..=0xa130ff).contains(&address)
    }
    pub fn source_offset(&self, address: u32) -> Option<usize> {
        if address >= 0x400000 {
            return None;
        }
        let slot = address as usize / PAGE_BYTES;
        let offset = usize::from(self.slots[slot]) * PAGE_BYTES + address as usize % PAGE_BYTES;
        (offset < self.rom_bytes).then_some(offset)
    }
    pub fn control(&mut self, address: u32, value: u8) {
        self.slots[((address - 0xa130f0) / 2) as usize] = value & self.page_mask;
    }
    pub fn power_on(&mut self) {
        self.slots = [0, 1, 2, 3, 4, 5, 6, 7];
    }
    pub fn write_state(&self, w: &mut StateWriter) {
        w.write_bytes(b"ESF1");
        w.write_bytes(&self.slots);
    }
    pub fn read_state(&mut self, r: &mut StateReader<'_>) -> Result<()> {
        let mut tag = [0; 4];
        r.read_exact(&mut tag)?;
        ensure!(&tag == b"ESF1", "MD extended mapper tag mismatch");
        let mut slots = [0; 8];
        r.read_exact(&mut slots)?;
        ensure!(
            slots.iter().all(|page| *page <= self.page_mask),
            "invalid MD extended ROM page"
        );
        self.slots = slots;
        Ok(())
    }
}
