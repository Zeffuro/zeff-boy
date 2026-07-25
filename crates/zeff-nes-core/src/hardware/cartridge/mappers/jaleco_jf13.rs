use crate::hardware::cartridge::{Mapper, Mirroring};

pub struct JalecoJf13 {
    prg_rom: Vec<u8>,
    chr: Vec<u8>,
    mirroring: Mirroring,
    prg_bank: u8,
    chr_bank: u8,
}

impl JalecoJf13 {
    pub fn new(prg_rom: Vec<u8>, chr: Vec<u8>, mirroring: Mirroring) -> Self {
        Self {
            prg_rom,
            chr,
            mirroring,
            prg_bank: 0,
            chr_bank: 0,
        }
    }

    fn prg_bank_count_32k(&self) -> usize {
        (self.prg_rom.len() / 0x8000).max(1)
    }

    fn chr_bank_count_8k(&self) -> usize {
        (self.chr.len() / 0x2000).max(1)
    }

    fn chr_addr(&self, addr: u16) -> usize {
        let bank = self.chr_bank as usize % self.chr_bank_count_8k();
        (bank * 0x2000 + addr as usize) % self.chr.len()
    }

    fn write_bank_select(&mut self, val: u8) {
        self.prg_bank = (val >> 4) & 0x03;
        self.chr_bank = (val & 0x03) | ((val >> 4) & 0x04);
    }
}

impl Mapper for JalecoJf13 {
    fn cpu_peek(&self, addr: u16) -> u8 {
        match addr {
            0x8000..=0xFFFF => {
                let bank = self.prg_bank as usize % self.prg_bank_count_32k();
                let offset = (addr - 0x8000) as usize;
                self.prg_rom[(bank * 0x8000 + offset) % self.prg_rom.len()]
            }
            _ => 0,
        }
    }

    fn cpu_write(&mut self, addr: u16, val: u8) {
        match addr {
            0x6000..=0x6FFF | 0xE000..=0xEFFF => self.write_bank_select(val),
            _ => {}
        }
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
        crate::save_state::write_chr_state(w, &self.chr);
    }

    fn read_state(&mut self, r: &mut crate::save_state::StateReader) -> anyhow::Result<()> {
        self.prg_bank = r.read_u8()? & 0x03;
        self.chr_bank = r.read_u8()? & 0x07;
        crate::save_state::read_chr_state(r, &mut self.chr, "Jaleco JF-13")?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn prg_banks(count: usize) -> Vec<u8> {
        let mut prg = Vec::new();
        for bank in 0..count {
            prg.extend(vec![bank as u8; 0x8000]);
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
    fn switches_prg_and_chr_banks() {
        let mut mapper = JalecoJf13::new(prg_banks(4), chr_banks(8), Mirroring::Vertical);

        mapper.cpu_write(0x6000, 0x72);
        assert_eq!(mapper.cpu_peek(0x8000), 3);
        assert_eq!(mapper.chr_read(0x0000), 6);
    }

    #[test]
    fn mirrors_registers_at_e000() {
        let mut mapper = JalecoJf13::new(prg_banks(4), chr_banks(8), Mirroring::Vertical);

        mapper.cpu_write(0xE000, 0x51);
        assert_eq!(mapper.cpu_peek(0x8000), 1);
        assert_eq!(mapper.chr_read(0x0000), 5);
    }
}
