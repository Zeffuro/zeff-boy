use crate::hardware::cartridge::{Mapper, Mirroring};

pub struct HolyDiver {
    prg_rom: Vec<u8>,
    chr: Vec<u8>,
    mirroring: Mirroring,
    holy_diver_mirroring: bool,
    prg_bank: u8,
    chr_bank: u8,
}

impl HolyDiver {
    pub fn new(prg_rom: Vec<u8>, chr: Vec<u8>, mirroring: Mirroring, submapper: u8) -> Self {
        let holy_diver_mirroring = submapper == 3 || matches!(mirroring, Mirroring::FourScreen);
        Self {
            prg_rom,
            chr,
            mirroring: if holy_diver_mirroring {
                Mirroring::Horizontal
            } else {
                Mirroring::SingleScreenLower
            },
            holy_diver_mirroring,
            prg_bank: 0,
            chr_bank: 0,
        }
    }

    fn prg_bank_count_16k(&self) -> usize {
        (self.prg_rom.len() / 0x4000).max(1)
    }

    fn prg_read_16k(&self, bank: usize, addr: u16, base: u16) -> u8 {
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

    fn update_mirroring(&mut self, vertical_or_upper: bool) {
        self.mirroring = if self.holy_diver_mirroring {
            if vertical_or_upper {
                Mirroring::Vertical
            } else {
                Mirroring::Horizontal
            }
        } else if vertical_or_upper {
            Mirroring::SingleScreenUpper
        } else {
            Mirroring::SingleScreenLower
        };
    }
}

impl Mapper for HolyDiver {
    fn cpu_peek(&self, addr: u16) -> u8 {
        match addr {
            0x8000..=0xBFFF => self.prg_read_16k(self.prg_bank as usize, addr, 0x8000),
            0xC000..=0xFFFF => {
                self.prg_read_16k(self.prg_bank_count_16k().saturating_sub(1), addr, 0xC000)
            }
            _ => 0,
        }
    }

    fn cpu_write(&mut self, addr: u16, val: u8) {
        if addr >= 0x8000 {
            self.prg_bank = val & 0x07;
            self.chr_bank = val >> 4;
            self.update_mirroring(val & 0x08 != 0);
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
        w.write_bool(self.holy_diver_mirroring);
        w.write_u8(self.prg_bank);
        w.write_u8(self.chr_bank);
        crate::save_state::write_chr_state(w, &self.chr);
    }

    fn read_state(&mut self, r: &mut crate::save_state::StateReader) -> anyhow::Result<()> {
        self.mirroring = crate::save_state::decode_mirroring(r.read_u8()?)?;
        self.holy_diver_mirroring = r.read_bool()?;
        self.prg_bank = r.read_u8()? & 0x07;
        self.chr_bank = r.read_u8()? & 0x0F;
        crate::save_state::read_chr_state(r, &mut self.chr, "Mapper 78")?;
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
    fn switches_prg_chr_and_holy_diver_mirroring() {
        let mut mapper = HolyDiver::new(prg_banks(8), chr_banks(16), Mirroring::FourScreen, 0);

        mapper.cpu_write(0x8000, 0xA3);

        assert_eq!(mapper.cpu_peek(0x8000), 3);
        assert_eq!(mapper.cpu_peek(0xC000), 7);
        assert_eq!(mapper.chr_read(0x0000), 10);
        assert_eq!(mapper.mirroring(), Mirroring::Horizontal);

        mapper.cpu_write(0x8000, 0xAB);
        assert_eq!(mapper.mirroring(), Mirroring::Vertical);
    }

    #[test]
    fn cosmo_carrier_mode_uses_single_screen_mirroring() {
        let mut mapper = HolyDiver::new(prg_banks(8), chr_banks(16), Mirroring::Horizontal, 1);

        mapper.cpu_write(0x8000, 0xA0);
        assert_eq!(mapper.mirroring(), Mirroring::SingleScreenLower);
        mapper.cpu_write(0x8000, 0xA8);
        assert_eq!(mapper.mirroring(), Mirroring::SingleScreenUpper);
    }
}
