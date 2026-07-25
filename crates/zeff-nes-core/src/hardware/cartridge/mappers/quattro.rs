use crate::hardware::cartridge::{Mapper, Mirroring};

pub struct Quattro {
    prg_rom: Vec<u8>,
    chr_ram: Vec<u8>,
    mirroring: Mirroring,
    block: u8,
    page: u8,
}

impl Quattro {
    pub fn new(prg_rom: Vec<u8>, chr_ram: Vec<u8>, mirroring: Mirroring) -> Self {
        Self {
            prg_rom,
            chr_ram,
            mirroring,
            block: 0,
            page: 0,
        }
    }

    fn prg_bank_count_16k(&self) -> usize {
        (self.prg_rom.len() / 0x4000).max(1)
    }

    fn read_prg_16k(&self, bank: usize, addr: u16, base: u16) -> u8 {
        let bank = bank % self.prg_bank_count_16k();
        let offset = (addr - base) as usize;
        self.prg_rom[(bank * 0x4000 + offset) % self.prg_rom.len()]
    }
}

impl Mapper for Quattro {
    fn cpu_peek(&self, addr: u16) -> u8 {
        let block_base = self.block as usize * 4;
        match addr {
            0x8000..=0xBFFF => self.read_prg_16k(block_base + self.page as usize, addr, 0x8000),
            0xC000..=0xFFFF => self.read_prg_16k(block_base + 3, addr, 0xC000),
            _ => 0,
        }
    }

    fn cpu_write(&mut self, addr: u16, val: u8) {
        match addr {
            0x8000..=0xBFFF => self.block = (val >> 3) & 0x03,
            0xC000..=0xFFFF => self.page = val & 0x03,
            _ => {}
        }
    }

    fn chr_read(&mut self, addr: u16) -> u8 {
        if self.chr_ram.is_empty() {
            return 0;
        }
        self.chr_ram[addr as usize % self.chr_ram.len()]
    }

    fn chr_write(&mut self, addr: u16, val: u8) {
        let len = self.chr_ram.len();
        if len > 0 {
            self.chr_ram[addr as usize % len] = val;
        }
    }

    fn mirroring(&self) -> Mirroring {
        self.mirroring
    }

    fn write_state(&self, w: &mut crate::save_state::StateWriter) {
        w.write_u8(self.block);
        w.write_u8(self.page);
        crate::save_state::write_chr_state(w, &self.chr_ram);
    }

    fn read_state(&mut self, r: &mut crate::save_state::StateReader) -> anyhow::Result<()> {
        self.block = r.read_u8()? & 0x03;
        self.page = r.read_u8()? & 0x03;
        crate::save_state::read_chr_state(r, &mut self.chr_ram, "Camerica Quattro")?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn prg_banks(count: usize) -> Vec<u8> {
        let mut prg = Vec::new();
        for bank in 0..count {
            prg.extend(vec![bank as u8; 0x4000]);
        }
        prg
    }

    #[test]
    fn selects_block_and_page() {
        let mut mapper = Quattro::new(prg_banks(16), vec![0; 0x2000], Mirroring::Vertical);

        mapper.cpu_write(0x8000, 0x10);
        mapper.cpu_write(0xC000, 0x02);

        assert_eq!(mapper.cpu_peek(0x8000), 10);
        assert_eq!(mapper.cpu_peek(0xC000), 11);
    }

    #[test]
    fn chr_ram_is_read_write() {
        let mut mapper = Quattro::new(prg_banks(4), vec![0; 0x2000], Mirroring::Vertical);

        mapper.chr_write(0x0123, 0xA5);
        assert_eq!(mapper.chr_read(0x0123), 0xA5);
    }
}
