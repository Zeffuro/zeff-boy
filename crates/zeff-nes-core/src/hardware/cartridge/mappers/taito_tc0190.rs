use crate::hardware::cartridge::{Mapper, Mirroring};

pub struct TaitoTc0190 {
    prg_rom: Vec<u8>,
    chr: Vec<u8>,
    mirroring: Mirroring,
    prg_banks: [u8; 2],
    chr_2k_banks: [u8; 2],
    chr_1k_banks: [u8; 4],
}

impl TaitoTc0190 {
    pub fn new(prg_rom: Vec<u8>, chr: Vec<u8>, mirroring: Mirroring) -> Self {
        Self {
            prg_rom,
            chr,
            mirroring,
            prg_banks: [0, 1],
            chr_2k_banks: [0, 1],
            chr_1k_banks: [0, 1, 2, 3],
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

    fn chr_bank_count_1k(&self) -> usize {
        (self.chr.len() / 0x0400).max(1)
    }

    fn chr_addr(&self, addr: u16) -> usize {
        let addr = addr as usize;
        let (bank, offset) = match addr {
            0x0000..=0x07FF => (self.chr_2k_banks[0] as usize * 2, addr & 0x07FF),
            0x0800..=0x0FFF => (self.chr_2k_banks[1] as usize * 2, addr & 0x07FF),
            0x1000..=0x13FF => (self.chr_1k_banks[0] as usize, addr & 0x03FF),
            0x1400..=0x17FF => (self.chr_1k_banks[1] as usize, addr & 0x03FF),
            0x1800..=0x1BFF => (self.chr_1k_banks[2] as usize, addr & 0x03FF),
            _ => (self.chr_1k_banks[3] as usize, addr & 0x03FF),
        };
        (bank % self.chr_bank_count_1k() * 0x0400 + offset) % self.chr.len()
    }
}

impl Mapper for TaitoTc0190 {
    fn cpu_peek(&self, addr: u16) -> u8 {
        let fixed_second_last = self.prg_bank_count_8k().saturating_sub(2);
        let fixed_last = self.prg_bank_count_8k().saturating_sub(1);
        match addr {
            0x8000..=0x9FFF => self.prg_read_bank(self.prg_banks[0] as usize, addr, 0x8000),
            0xA000..=0xBFFF => self.prg_read_bank(self.prg_banks[1] as usize, addr, 0xA000),
            0xC000..=0xDFFF => self.prg_read_bank(fixed_second_last, addr, 0xC000),
            0xE000..=0xFFFF => self.prg_read_bank(fixed_last, addr, 0xE000),
            _ => 0,
        }
    }

    fn cpu_write(&mut self, addr: u16, val: u8) {
        match addr & 0xA003 {
            0x8000 => {
                self.mirroring = if val & 0x40 != 0 {
                    Mirroring::Horizontal
                } else {
                    Mirroring::Vertical
                };
                self.prg_banks[0] = val & 0x3F;
            }
            0x8001 => self.prg_banks[1] = val & 0x3F,
            0x8002 => self.chr_2k_banks[0] = val,
            0x8003 => self.chr_2k_banks[1] = val,
            0xA000 => self.chr_1k_banks[0] = val,
            0xA001 => self.chr_1k_banks[1] = val,
            0xA002 => self.chr_1k_banks[2] = val,
            0xA003 => self.chr_1k_banks[3] = val,
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
        w.write_bytes(&self.chr_2k_banks);
        w.write_bytes(&self.chr_1k_banks);
        crate::save_state::write_chr_state(w, &self.chr);
    }

    fn read_state(&mut self, r: &mut crate::save_state::StateReader) -> anyhow::Result<()> {
        self.mirroring = crate::save_state::decode_mirroring(r.read_u8()?)?;
        r.read_exact(&mut self.prg_banks)?;
        r.read_exact(&mut self.chr_2k_banks)?;
        r.read_exact(&mut self.chr_1k_banks)?;
        crate::save_state::read_chr_state(r, &mut self.chr, "Taito TC0190")?;
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
    fn switches_prg_banks_and_mirroring() {
        let mut mapper = TaitoTc0190::new(prg_banks(8), chr_banks(16), Mirroring::Vertical);

        mapper.cpu_write(0x8000, 0x43);
        mapper.cpu_write(0x8001, 0x04);

        assert_eq!(mapper.cpu_peek(0x8000), 3);
        assert_eq!(mapper.cpu_peek(0xA000), 4);
        assert_eq!(mapper.cpu_peek(0xC000), 6);
        assert_eq!(mapper.cpu_peek(0xE000), 7);
        assert_eq!(mapper.mirroring(), Mirroring::Horizontal);
    }

    #[test]
    fn switches_2k_and_1k_chr_banks() {
        let mut mapper = TaitoTc0190::new(prg_banks(8), chr_banks(32), Mirroring::Vertical);

        mapper.cpu_write(0x8002, 0x03);
        mapper.cpu_write(0xA003, 0x0B);

        assert_eq!(mapper.chr_read(0x0000), 0x06);
        assert_eq!(mapper.chr_read(0x1C00), 0x0B);
    }
}
