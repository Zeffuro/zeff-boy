use crate::hardware::cartridge::{Mapper, Mirroring};

pub struct JalecoJf17 {
    prg_rom: Vec<u8>,
    chr: Vec<u8>,
    mirroring: Mirroring,
    fixed_low_prg: bool,
    prg_bank: u8,
    chr_bank: u8,
    control: u8,
}

impl JalecoJf17 {
    pub fn new(prg_rom: Vec<u8>, chr: Vec<u8>, mirroring: Mirroring) -> Self {
        Self {
            prg_rom,
            chr,
            mirroring,
            fixed_low_prg: false,
            prg_bank: 0,
            chr_bank: 0,
            control: 0,
        }
    }

    pub fn new_fixed_low_prg(prg_rom: Vec<u8>, chr: Vec<u8>, mirroring: Mirroring) -> Self {
        let mut mapper = Self::new(prg_rom, chr, mirroring);
        mapper.fixed_low_prg = true;
        mapper
    }

    fn prg_bank_count_16k(&self) -> usize {
        (self.prg_rom.len() / 0x4000).max(1)
    }

    fn prg_read_bank(&self, bank: usize, addr: u16, base: u16) -> u8 {
        let bank = bank % self.prg_bank_count_16k();
        let offset = (addr - base) as usize;
        self.prg_rom[(bank * 0x4000 + offset) % self.prg_rom.len()]
    }

    fn chr_bank_count_8k(&self) -> usize {
        (self.chr.len() / 0x2000).max(1)
    }

    fn chr_addr(&self, addr: u16) -> usize {
        let bank = self.chr_bank as usize % self.chr_bank_count_8k();
        (bank * 0x2000 + addr as usize) % self.chr.len()
    }
}

impl Mapper for JalecoJf17 {
    fn cpu_peek(&self, addr: u16) -> u8 {
        match (addr, self.fixed_low_prg) {
            (0x8000..=0xBFFF, true) => self.prg_read_bank(0, addr, 0x8000),
            (0x8000..=0xBFFF, false) => self.prg_read_bank(self.prg_bank as usize, addr, 0x8000),
            (0xC000..=0xFFFF, true) => self.prg_read_bank(self.prg_bank as usize, addr, 0xC000),
            (0xC000..=0xFFFF, false) => {
                self.prg_read_bank(self.prg_bank_count_16k().saturating_sub(1), addr, 0xC000)
            }
            _ => 0,
        }
    }

    fn cpu_write(&mut self, addr: u16, val: u8) {
        if addr < 0x8000 {
            return;
        }

        let val = val & self.cpu_peek(addr);
        if val & 0x80 != 0 && self.control & 0x80 == 0 {
            self.prg_bank = val & 0x0F;
        }
        if val & 0x40 != 0 && self.control & 0x40 == 0 {
            self.chr_bank = val & 0x0F;
        }
        self.control = val & 0xC0;
    }

    fn chr_read(&mut self, addr: u16) -> u8 {
        if self.chr.is_empty() {
            return 0;
        }
        self.chr[self.chr_addr(addr)]
    }

    fn chr_write(&mut self, addr: u16, val: u8) {
        if self.chr.is_empty() {
            return;
        }
        let idx = self.chr_addr(addr);
        self.chr[idx] = val;
    }

    fn mirroring(&self) -> Mirroring {
        self.mirroring
    }

    fn write_state(&self, w: &mut crate::save_state::StateWriter) {
        w.write_u8(self.prg_bank);
        w.write_u8(self.chr_bank);
        w.write_u8(self.control);
        crate::save_state::write_chr_state(w, &self.chr);
    }

    fn read_state(&mut self, r: &mut crate::save_state::StateReader) -> anyhow::Result<()> {
        self.prg_bank = r.read_u8()? & 0x0F;
        self.chr_bank = r.read_u8()? & 0x0F;
        self.control = r.read_u8()? & 0xC0;
        crate::save_state::read_chr_state(r, &mut self.chr, "Jaleco JF-17")?;
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

    fn chr_banks(count: usize) -> Vec<u8> {
        let mut chr = Vec::new();
        for bank in 0..count {
            chr.extend(vec![bank as u8; 0x2000]);
        }
        chr
    }

    #[test]
    fn switches_prg_and_chr_on_control_bit_edges() {
        let mut prg = prg_banks(8);
        for bank in 0..8 {
            prg[bank * 0x4000] = 0xFF;
        }
        let mut mapper = JalecoJf17::new(prg, chr_banks(16), Mirroring::Vertical);

        mapper.cpu_write(0x8000, 0xC3);
        assert_eq!(mapper.cpu_peek(0x8001), 3);
        assert_eq!(mapper.chr_read(0x0000), 3);

        mapper.cpu_write(0xC000, 0xC4);
        assert_eq!(mapper.cpu_peek(0x8001), 3);
        assert_eq!(mapper.chr_read(0x0000), 3);

        mapper.cpu_write(0xC000, 0x04);
        mapper.cpu_write(0xC000, 0x84);
        assert_eq!(mapper.cpu_peek(0x8001), 4);
        assert_eq!(mapper.chr_read(0x0000), 3);
    }

    #[test]
    fn fixed_last_prg_bank() {
        let mut prg = prg_banks(8);
        prg[0] = 0xFF;
        let mapper = JalecoJf17::new(prg, chr_banks(1), Mirroring::Horizontal);

        assert_eq!(mapper.cpu_peek(0xC001), 7);
    }

    #[test]
    fn mapper92_uses_fixed_low_prg_layout() {
        let mut prg = prg_banks(8);
        for bank in 0..8 {
            prg[bank * 0x4000] = 0xFF;
        }
        let mut mapper = JalecoJf17::new_fixed_low_prg(prg, chr_banks(1), Mirroring::Horizontal);

        mapper.cpu_write(0x8000, 0x84);

        assert_eq!(mapper.cpu_peek(0x8001), 0);
        assert_eq!(mapper.cpu_peek(0xC001), 4);
    }
}
