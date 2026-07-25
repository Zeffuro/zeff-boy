use crate::hardware::cartridge::{Mapper, Mirroring};

pub struct Mapper112 {
    prg_rom: Vec<u8>,
    chr: Vec<u8>,
    mirroring: Mirroring,
    bank_select: u8,
    prg_banks: [u8; 2],
    chr_banks: [u8; 6],
}

impl Mapper112 {
    pub fn new(prg_rom: Vec<u8>, chr: Vec<u8>, mirroring: Mirroring) -> Self {
        Self {
            prg_rom,
            chr,
            mirroring,
            bank_select: 0,
            prg_banks: [0, 1],
            chr_banks: [0, 2, 4, 5, 6, 7],
        }
    }

    fn prg_bank_count_8k(&self) -> usize {
        (self.prg_rom.len() / 0x2000).max(1)
    }

    fn prg_bank(&self, addr: u16) -> usize {
        let count = self.prg_bank_count_8k();
        match addr {
            0x8000..=0x9FFF => usize::from(self.prg_banks[0]) % count,
            0xA000..=0xBFFF => usize::from(self.prg_banks[1]) % count,
            0xC000..=0xDFFF => count.saturating_sub(2),
            0xE000..=0xFFFF => count.saturating_sub(1),
            _ => 0,
        }
    }

    fn chr_bank_count_1k(&self) -> usize {
        (self.chr.len() / 0x0400).max(1)
    }

    fn chr_bank(&self, addr: u16) -> usize {
        let bank = match addr {
            0x0000..=0x07FF => {
                (usize::from(self.chr_banks[0]) & !1) + usize::from((addr & 0x0400) != 0)
            }
            0x0800..=0x0FFF => {
                (usize::from(self.chr_banks[1]) & !1) + usize::from((addr & 0x0400) != 0)
            }
            0x1000..=0x13FF => usize::from(self.chr_banks[2]),
            0x1400..=0x17FF => usize::from(self.chr_banks[3]),
            0x1800..=0x1BFF => usize::from(self.chr_banks[4]),
            0x1C00..=0x1FFF => usize::from(self.chr_banks[5]),
            _ => 0,
        };
        bank % self.chr_bank_count_1k()
    }
}

impl Mapper for Mapper112 {
    fn cpu_peek(&self, addr: u16) -> u8 {
        match addr {
            0x8000..=0xFFFF => {
                let bank = self.prg_bank(addr);
                let offset = addr as usize & 0x1FFF;
                self.prg_rom[(bank * 0x2000 + offset) % self.prg_rom.len()]
            }
            _ => 0,
        }
    }

    fn cpu_write(&mut self, addr: u16, val: u8) {
        match addr {
            0x8000..=0x9FFF => self.bank_select = val & 0x07,
            0xA000..=0xBFFF => match self.bank_select {
                0 => self.prg_banks[0] = val,
                1 => self.prg_banks[1] = val,
                2 => self.chr_banks[0] = val,
                3 => self.chr_banks[1] = val,
                4 => self.chr_banks[2] = val,
                5 => self.chr_banks[3] = val,
                6 => self.chr_banks[4] = val,
                7 => self.chr_banks[5] = val,
                _ => unreachable!(),
            },
            0xE000..=0xFFFF => {
                self.mirroring = if val & 0x01 != 0 {
                    Mirroring::Vertical
                } else {
                    Mirroring::Horizontal
                };
            }
            _ => {}
        }
    }

    fn chr_read(&mut self, addr: u16) -> u8 {
        if self.chr.is_empty() {
            return 0;
        }
        let bank = self.chr_bank(addr);
        let offset = addr as usize & 0x03FF;
        self.chr[(bank * 0x0400 + offset) % self.chr.len()]
    }

    fn chr_write(&mut self, addr: u16, val: u8) {
        if self.chr.is_empty() {
            return;
        }
        let bank = self.chr_bank(addr);
        let offset = addr as usize & 0x03FF;
        let idx = (bank * 0x0400 + offset) % self.chr.len();
        self.chr[idx] = val;
    }

    fn mirroring(&self) -> Mirroring {
        self.mirroring
    }

    fn write_state(&self, w: &mut crate::save_state::StateWriter) {
        w.write_u8(crate::save_state::encode_mirroring(self.mirroring));
        w.write_u8(self.bank_select);
        w.write_bytes(&self.prg_banks);
        w.write_bytes(&self.chr_banks);
        crate::save_state::write_chr_state(w, &self.chr);
    }

    fn read_state(&mut self, r: &mut crate::save_state::StateReader) -> anyhow::Result<()> {
        self.mirroring = crate::save_state::decode_mirroring(r.read_u8()?)?;
        self.bank_select = r.read_u8()? & 0x07;
        r.read_exact(&mut self.prg_banks)?;
        r.read_exact(&mut self.chr_banks)?;
        crate::save_state::read_chr_state(r, &mut self.chr, "Mapper 112")?;
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
            chr.extend(vec![bank as u8; 0x0400]);
        }
        chr
    }

    #[test]
    fn switches_prg_and_scrambled_chr_registers() {
        let mut mapper = Mapper112::new(prg_banks(16), chr_banks(32), Mirroring::Horizontal);

        mapper.cpu_write(0x8000, 0);
        mapper.cpu_write(0xA000, 3);
        mapper.cpu_write(0x8000, 1);
        mapper.cpu_write(0xA000, 4);
        mapper.cpu_write(0x8000, 2);
        mapper.cpu_write(0xA000, 6);
        mapper.cpu_write(0x8000, 4);
        mapper.cpu_write(0xA000, 9);

        assert_eq!(mapper.cpu_peek(0x8000), 3);
        assert_eq!(mapper.cpu_peek(0xA000), 4);
        assert_eq!(mapper.cpu_peek(0xC000), 14);
        assert_eq!(mapper.cpu_peek(0xE000), 15);
        assert_eq!(mapper.chr_read(0x0000), 6);
        assert_eq!(mapper.chr_read(0x0400), 7);
        assert_eq!(mapper.chr_read(0x1000), 9);
    }

    #[test]
    fn switches_mirroring() {
        let mut mapper = Mapper112::new(prg_banks(4), chr_banks(8), Mirroring::Horizontal);

        mapper.cpu_write(0xE000, 1);
        assert_eq!(mapper.mirroring(), Mirroring::Vertical);
        mapper.cpu_write(0xE000, 0);
        assert_eq!(mapper.mirroring(), Mirroring::Horizontal);
    }
}
