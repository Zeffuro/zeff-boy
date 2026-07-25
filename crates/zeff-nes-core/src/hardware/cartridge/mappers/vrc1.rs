use crate::hardware::cartridge::{Mapper, Mirroring};

pub struct Vrc1 {
    prg_rom: Vec<u8>,
    chr: Vec<u8>,
    mirroring: Mirroring,
    fixed_four_screen: bool,
    prg_banks: [u8; 3],
    chr_low: [u8; 2],
    chr_high: [u8; 2],
}

impl Vrc1 {
    pub fn new(prg_rom: Vec<u8>, chr: Vec<u8>, mirroring: Mirroring) -> Self {
        Self {
            prg_rom,
            chr,
            mirroring,
            fixed_four_screen: matches!(mirroring, Mirroring::FourScreen),
            prg_banks: [0, 1, 2],
            chr_low: [0, 0],
            chr_high: [0, 0],
        }
    }

    fn prg_bank_count_8k(&self) -> usize {
        (self.prg_rom.len() / 0x2000).max(1)
    }

    fn prg_read_bank(&self, bank: usize, addr: u16, base: u16) -> u8 {
        let bank = bank % self.prg_bank_count_8k();
        let offset = (addr - base) as usize;
        self.prg_rom[(bank * 0x2000 + offset) % self.prg_rom.len()]
    }

    fn chr_bank_count_4k(&self) -> usize {
        (self.chr.len() / 0x1000).max(1)
    }

    fn chr_bank(&self, slot: usize) -> usize {
        ((self.chr_high[slot] << 4) | self.chr_low[slot]) as usize % self.chr_bank_count_4k()
    }

    fn chr_addr(&self, addr: u16) -> usize {
        let slot = (addr as usize / 0x1000) & 0x01;
        let offset = addr as usize & 0x0FFF;
        (self.chr_bank(slot) * 0x1000 + offset) % self.chr.len()
    }
}

impl Mapper for Vrc1 {
    fn cpu_peek(&self, addr: u16) -> u8 {
        match addr {
            0x8000..=0x9FFF => self.prg_read_bank(self.prg_banks[0] as usize, addr, 0x8000),
            0xA000..=0xBFFF => self.prg_read_bank(self.prg_banks[1] as usize, addr, 0xA000),
            0xC000..=0xDFFF => self.prg_read_bank(self.prg_banks[2] as usize, addr, 0xC000),
            0xE000..=0xFFFF => {
                self.prg_read_bank(self.prg_bank_count_8k().saturating_sub(1), addr, 0xE000)
            }
            _ => 0,
        }
    }

    fn cpu_write(&mut self, addr: u16, val: u8) {
        match addr & 0xF000 {
            0x8000 => self.prg_banks[0] = val & 0x0F,
            0x9000 => {
                if !self.fixed_four_screen {
                    self.mirroring = if val & 0x01 != 0 {
                        Mirroring::Horizontal
                    } else {
                        Mirroring::Vertical
                    };
                }
                self.chr_high[0] = (val >> 1) & 0x01;
                self.chr_high[1] = (val >> 2) & 0x01;
            }
            0xA000 => self.prg_banks[1] = val & 0x0F,
            0xC000 => self.prg_banks[2] = val & 0x0F,
            0xE000 => self.chr_low[0] = val & 0x0F,
            0xF000 => self.chr_low[1] = val & 0x0F,
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
        w.write_u8(crate::save_state::encode_mirroring(self.mirroring));
        w.write_bytes(&self.prg_banks);
        w.write_bytes(&self.chr_low);
        w.write_bytes(&self.chr_high);
        crate::save_state::write_chr_state(w, &self.chr);
    }

    fn read_state(&mut self, r: &mut crate::save_state::StateReader) -> anyhow::Result<()> {
        self.mirroring = crate::save_state::decode_mirroring(r.read_u8()?)?;
        r.read_exact(&mut self.prg_banks)?;
        r.read_exact(&mut self.chr_low)?;
        r.read_exact(&mut self.chr_high)?;
        for bank in &mut self.prg_banks {
            *bank &= 0x0F;
        }
        for bank in &mut self.chr_low {
            *bank &= 0x0F;
        }
        for bank in &mut self.chr_high {
            *bank &= 0x01;
        }
        crate::save_state::read_chr_state(r, &mut self.chr, "VRC1")?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn prg_banks(count: usize) -> Vec<u8> {
        let mut prg = Vec::new();
        for bank in 0..count {
            prg.extend(vec![bank as u8; 0x2000]);
        }
        prg
    }

    fn chr_banks(count: usize) -> Vec<u8> {
        let mut chr = Vec::new();
        for bank in 0..count {
            chr.extend(vec![bank as u8; 0x1000]);
        }
        chr
    }

    #[test]
    fn switches_prg_banks() {
        let mut mapper = Vrc1::new(prg_banks(8), chr_banks(8), Mirroring::Vertical);

        mapper.cpu_write(0x8000, 0x03);
        mapper.cpu_write(0xA000, 0x04);
        mapper.cpu_write(0xC000, 0x05);

        assert_eq!(mapper.cpu_peek(0x8000), 3);
        assert_eq!(mapper.cpu_peek(0xA000), 4);
        assert_eq!(mapper.cpu_peek(0xC000), 5);
        assert_eq!(mapper.cpu_peek(0xE000), 7);
    }

    #[test]
    fn switches_chr_banks() {
        let mut mapper = Vrc1::new(prg_banks(8), chr_banks(32), Mirroring::Vertical);

        mapper.cpu_write(0x9000, 0x06);
        mapper.cpu_write(0xE000, 0x03);
        mapper.cpu_write(0xF000, 0x04);

        assert_eq!(mapper.chr_read(0x0000), 0x13);
        assert_eq!(mapper.chr_read(0x1000), 0x14);
    }

    #[test]
    fn switches_mirroring_unless_four_screen() {
        let mut mapper = Vrc1::new(prg_banks(8), chr_banks(8), Mirroring::Vertical);

        mapper.cpu_write(0x9000, 0x01);
        assert_eq!(mapper.mirroring(), Mirroring::Horizontal);

        let mut four_screen = Vrc1::new(prg_banks(8), chr_banks(8), Mirroring::FourScreen);
        four_screen.cpu_write(0x9000, 0x01);
        assert_eq!(four_screen.mirroring(), Mirroring::FourScreen);
    }
}
