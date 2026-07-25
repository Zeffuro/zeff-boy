use crate::hardware::cartridge::{Mapper, Mirroring};

pub struct Bandai74161 {
    prg_rom: Vec<u8>,
    chr: Vec<u8>,
    mirroring: Mirroring,
    control: u8,
}

impl Bandai74161 {
    pub fn new(prg_rom: Vec<u8>, chr: Vec<u8>, mirroring: Mirroring) -> Self {
        Self {
            prg_rom,
            chr,
            mirroring,
            control: 0,
        }
    }

    fn prg_bank_count_16k(&self) -> usize {
        (self.prg_rom.len() / 0x4000).max(1)
    }

    fn chr_bank_count_8k(&self) -> usize {
        (self.chr.len() / 0x2000).max(1)
    }

    fn selected_prg_bank(&self) -> usize {
        usize::from((self.control >> 4) & 0x07) % self.prg_bank_count_16k()
    }

    fn selected_chr_bank(&self) -> usize {
        usize::from(self.control & 0x0F) % self.chr_bank_count_8k()
    }

    fn chr_addr(&self, addr: u16) -> usize {
        (self.selected_chr_bank() * 0x2000 + addr as usize) % self.chr.len()
    }
}

impl Mapper for Bandai74161 {
    fn cpu_peek(&self, addr: u16) -> u8 {
        match addr {
            0x8000..=0xBFFF => {
                let bank = self.selected_prg_bank();
                let offset = (addr - 0x8000) as usize;
                self.prg_rom[(bank * 0x4000 + offset) % self.prg_rom.len()]
            }
            0xC000..=0xFFFF => {
                let bank = self.prg_bank_count_16k() - 1;
                let offset = (addr - 0xC000) as usize;
                self.prg_rom[(bank * 0x4000 + offset) % self.prg_rom.len()]
            }
            _ => 0,
        }
    }

    fn cpu_write(&mut self, addr: u16, val: u8) {
        if (0xC000..=0xC0FF).contains(&addr) {
            self.control = val & self.cpu_peek(addr);
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
        w.write_u8(self.control);
        crate::save_state::write_chr_state(w, &self.chr);
    }

    fn read_state(&mut self, r: &mut crate::save_state::StateReader) -> anyhow::Result<()> {
        self.control = r.read_u8()?;
        crate::save_state::read_chr_state(r, &mut self.chr, "Bandai 74161")?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn prg_banks(values: &[u8]) -> Vec<u8> {
        let mut prg = Vec::new();
        for &value in values {
            prg.extend(vec![value; 0x4000]);
        }
        prg
    }

    fn chr_banks(values: &[u8]) -> Vec<u8> {
        let mut chr = Vec::new();
        for &value in values {
            chr.extend(vec![value; 0x2000]);
        }
        chr
    }

    #[test]
    fn switches_16k_prg_and_8k_chr_with_bus_conflict_safe_write() {
        let mut prg = prg_banks(&[0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77]);
        prg[7 * 0x4000] = 0x25;
        let chr = chr_banks(&[0x00, 0xAA, 0xBB, 0xCC, 0xDD, 0xEE]);

        let mut mapper = Bandai74161::new(prg, chr, Mirroring::Horizontal);

        mapper.cpu_write(0xC000, 0x25);
        assert_eq!(mapper.cpu_peek(0x8000), 0x22);
        assert_eq!(mapper.cpu_peek(0xC001), 0x77);
        assert_eq!(mapper.chr_read(0x0000), 0xEE);
    }

    #[test]
    fn uses_header_mirroring() {
        let mut prg = prg_banks(&[0x00, 0x11, 0x22, 0x33]);
        prg[3 * 0x4000] = 0x80;
        let mut mapper = Bandai74161::new(prg, chr_banks(&[0x00]), Mirroring::Horizontal);

        assert_eq!(mapper.mirroring(), Mirroring::Horizontal);
        mapper.cpu_write(0xC000, 0x80);
        assert_eq!(mapper.mirroring(), Mirroring::Horizontal);
    }
}
