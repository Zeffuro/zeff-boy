use crate::hardware::cartridge::{Mapper, Mirroring};

pub struct J87 {
    prg_rom: Vec<u8>,
    chr: Vec<u8>,
    mirroring: Mirroring,
    chr_bank: u8,
}

impl J87 {
    pub fn new(prg_rom: Vec<u8>, chr: Vec<u8>, mirroring: Mirroring) -> Self {
        Self {
            prg_rom,
            chr,
            mirroring,
            chr_bank: 0,
        }
    }

    fn chr_bank_count_8k(&self) -> usize {
        (self.chr.len() / 0x2000).max(1)
    }

    fn chr_addr(&self, addr: u16) -> usize {
        let bank = self.chr_bank as usize % self.chr_bank_count_8k();
        (bank * 0x2000 + addr as usize) % self.chr.len()
    }
}

impl Mapper for J87 {
    fn cpu_peek(&self, addr: u16) -> u8 {
        match addr {
            0x8000..=0xFFFF => {
                let offset = if self.prg_rom.len() <= 0x4000 {
                    (addr - 0x8000) as usize % 0x4000
                } else {
                    (addr - 0x8000) as usize
                };
                self.prg_rom[offset % self.prg_rom.len()]
            }
            _ => 0,
        }
    }

    fn cpu_write(&mut self, addr: u16, val: u8) {
        if (0x6000..=0x7FFF).contains(&addr) {
            self.chr_bank = ((val & 0x01) << 1) | ((val & 0x02) >> 1);
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
        w.write_u8(self.chr_bank);
        crate::save_state::write_chr_state(w, &self.chr);
    }

    fn read_state(&mut self, r: &mut crate::save_state::StateReader) -> anyhow::Result<()> {
        self.chr_bank = r.read_u8()? & 0x03;
        crate::save_state::read_chr_state(r, &mut self.chr, "J87")?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chr_banks(count: usize) -> Vec<u8> {
        let mut chr = Vec::new();
        for bank in 0..count {
            chr.extend(vec![bank as u8; 0x2000]);
        }
        chr
    }

    #[test]
    fn swaps_chr_select_bits() {
        let mut mapper = J87::new(vec![0xEA; 0x8000], chr_banks(4), Mirroring::Vertical);

        mapper.cpu_write(0x6000, 0x01);
        assert_eq!(mapper.chr_read(0x0000), 2);

        mapper.cpu_write(0x6000, 0x02);
        assert_eq!(mapper.chr_read(0x0000), 1);
    }
}
