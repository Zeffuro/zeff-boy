use crate::hardware::cartridge::{Mapper, Mirroring};

pub struct IremG101 {
    prg_rom: Vec<u8>,
    chr: Vec<u8>,
    mirroring: Mirroring,
    prg_bank_0: u8,
    prg_bank_1: u8,
    prg_mode: bool,
    chr_banks: [u8; 8],
}

impl IremG101 {
    pub fn new(prg_rom: Vec<u8>, chr: Vec<u8>, mirroring: Mirroring) -> Self {
        Self {
            prg_rom,
            chr,
            mirroring,
            prg_bank_0: 0,
            prg_bank_1: 1,
            prg_mode: false,
            chr_banks: [0; 8],
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
        let slot = (addr as usize / 0x0400) & 0x07;
        let bank = self.chr_banks[slot] as usize % self.chr_bank_count_1k();
        let offset = addr as usize & 0x03FF;
        (bank * 0x0400 + offset) % self.chr.len()
    }
}

impl Mapper for IremG101 {
    fn cpu_peek(&self, addr: u16) -> u8 {
        let fixed_second_last = self.prg_bank_count_8k().saturating_sub(2);
        let fixed_last = self.prg_bank_count_8k().saturating_sub(1);

        match addr {
            0x8000..=0x9FFF if self.prg_mode => self.prg_read_bank(fixed_second_last, addr, 0x8000),
            0x8000..=0x9FFF => self.prg_read_bank(self.prg_bank_0 as usize, addr, 0x8000),
            0xA000..=0xBFFF => self.prg_read_bank(self.prg_bank_1 as usize, addr, 0xA000),
            0xC000..=0xDFFF if self.prg_mode => {
                self.prg_read_bank(self.prg_bank_0 as usize, addr, 0xC000)
            }
            0xC000..=0xDFFF => self.prg_read_bank(fixed_second_last, addr, 0xC000),
            0xE000..=0xFFFF => self.prg_read_bank(fixed_last, addr, 0xE000),
            _ => 0,
        }
    }

    fn cpu_write(&mut self, addr: u16, val: u8) {
        match addr & 0xF000 {
            0x8000 => self.prg_bank_0 = val & 0x1F,
            0x9000 => {
                self.mirroring = if val & 0x01 != 0 {
                    Mirroring::Horizontal
                } else {
                    Mirroring::Vertical
                };
                self.prg_mode = val & 0x02 != 0;
            }
            0xA000 => self.prg_bank_1 = val & 0x1F,
            0xB000 => self.chr_banks[(addr & 0x0007) as usize] = val,
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
        w.write_u8(self.prg_bank_0);
        w.write_u8(self.prg_bank_1);
        w.write_bool(self.prg_mode);
        w.write_u8(crate::save_state::encode_mirroring(self.mirroring));
        w.write_bytes(&self.chr_banks);
        crate::save_state::write_chr_state(w, &self.chr);
    }

    fn read_state(&mut self, r: &mut crate::save_state::StateReader) -> anyhow::Result<()> {
        self.prg_bank_0 = r.read_u8()? & 0x1F;
        self.prg_bank_1 = r.read_u8()? & 0x1F;
        self.prg_mode = r.read_bool()?;
        self.mirroring = crate::save_state::decode_mirroring(r.read_u8()?)?;
        r.read_exact(&mut self.chr_banks)?;
        crate::save_state::read_chr_state(r, &mut self.chr, "Irem G-101")?;
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
    fn switches_prg_banks_and_mode() {
        let mut mapper = IremG101::new(prg_banks(8), chr_banks(8), Mirroring::Horizontal);

        mapper.cpu_write(0x8000, 0x03);
        mapper.cpu_write(0xA000, 0x04);
        assert_eq!(mapper.cpu_peek(0x8000), 3);
        assert_eq!(mapper.cpu_peek(0xA000), 4);
        assert_eq!(mapper.cpu_peek(0xC000), 6);
        assert_eq!(mapper.cpu_peek(0xE000), 7);

        mapper.cpu_write(0x9000, 0x02);
        assert_eq!(mapper.cpu_peek(0x8000), 6);
        assert_eq!(mapper.cpu_peek(0xC000), 3);
    }

    #[test]
    fn switches_1k_chr_banks() {
        let mut mapper = IremG101::new(prg_banks(8), chr_banks(16), Mirroring::Horizontal);

        mapper.cpu_write(0xB004, 0x0A);
        assert_eq!(mapper.chr_read(0x1000), 0x0A);
    }

    #[test]
    fn mirroring_control_uses_bit0() {
        let mut mapper = IremG101::new(prg_banks(8), chr_banks(8), Mirroring::Horizontal);

        mapper.cpu_write(0x9000, 0x00);
        assert_eq!(mapper.mirroring(), Mirroring::Vertical);
        mapper.cpu_write(0x9000, 0x01);
        assert_eq!(mapper.mirroring(), Mirroring::Horizontal);
    }
}
