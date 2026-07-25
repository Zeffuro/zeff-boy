use crate::hardware::cartridge::{Mapper, Mirroring};

pub struct JalecoJf11 {
    prg_rom: Vec<u8>,
    chr: Vec<u8>,
    mirroring: Mirroring,
    bank_select: u8,
}

impl JalecoJf11 {
    pub fn new(prg_rom: Vec<u8>, chr: Vec<u8>, mirroring: Mirroring) -> Self {
        Self {
            prg_rom,
            chr,
            mirroring,
            bank_select: 0,
        }
    }

    fn prg_bank(&self) -> usize {
        usize::from((self.bank_select >> 4) & 0x03)
    }

    fn chr_bank(&self) -> usize {
        usize::from(self.bank_select & 0x03)
    }

    fn prg_bank_count_32k(&self) -> usize {
        (self.prg_rom.len() / 0x8000).max(1)
    }

    fn chr_bank_count_8k(&self) -> usize {
        (self.chr.len() / 0x2000).max(1)
    }

    fn prg_addr(&self, addr: u16) -> usize {
        let bank = self.prg_bank() % self.prg_bank_count_32k();
        let offset = (addr - 0x8000) as usize;
        (bank * 0x8000 + offset) % self.prg_rom.len()
    }

    fn chr_addr(&self, addr: u16) -> usize {
        let bank = self.chr_bank() % self.chr_bank_count_8k();
        (bank * 0x2000 + addr as usize) % self.chr.len()
    }
}

impl Mapper for JalecoJf11 {
    fn cpu_peek(&self, addr: u16) -> u8 {
        match addr {
            0x8000..=0xFFFF => self.prg_rom[self.prg_addr(addr)],
            _ => 0,
        }
    }

    fn cpu_write(&mut self, addr: u16, val: u8) {
        if (0x6000..=0x7FFF).contains(&addr) {
            self.bank_select = val;
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
        w.write_u8(self.bank_select);
        crate::save_state::write_chr_state(w, &self.chr);
    }

    fn read_state(&mut self, r: &mut crate::save_state::StateReader) -> anyhow::Result<()> {
        self.bank_select = r.read_u8()?;
        crate::save_state::read_chr_state(r, &mut self.chr, "Jaleco JF-11/JF-14")?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn prg_banks(values: &[u8]) -> Vec<u8> {
        let mut prg = Vec::new();
        for &value in values {
            prg.extend(vec![value; 0x8000]);
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
    fn switches_32k_prg_and_8k_chr_from_6000_port() {
        let mut mapper = JalecoJf11::new(
            prg_banks(&[0x00, 0x11, 0x22, 0x33]),
            chr_banks(&[0x00, 0x44, 0x88, 0xCC]),
            Mirroring::Horizontal,
        );

        mapper.cpu_write(0x6000, 0x21);
        assert_eq!(mapper.cpu_peek(0x8000), 0x22);
        assert_eq!(mapper.chr_read(0x0000), 0x44);

        mapper.cpu_write(0x8000, 0x13);
        assert_eq!(mapper.cpu_peek(0x8000), 0x22);
        assert_eq!(mapper.chr_read(0x0000), 0x44);
    }
}
