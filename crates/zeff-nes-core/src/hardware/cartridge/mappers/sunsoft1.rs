use crate::hardware::cartridge::{Mapper, Mirroring};

pub struct Sunsoft1 {
    prg_rom: Vec<u8>,
    chr: Vec<u8>,
    mirroring: Mirroring,
    chr_low_bank: u8,
    chr_high_bank: u8,
}

impl Sunsoft1 {
    pub fn new(prg_rom: Vec<u8>, chr: Vec<u8>, mirroring: Mirroring) -> Self {
        Self {
            prg_rom,
            chr,
            mirroring,
            chr_low_bank: 0,
            chr_high_bank: 4,
        }
    }

    fn chr_bank_count_4k(&self) -> usize {
        (self.chr.len() / 0x1000).max(1)
    }

    fn chr_addr(&self, addr: u16) -> usize {
        let bank = if addr < 0x1000 {
            self.chr_low_bank
        } else {
            self.chr_high_bank
        } as usize
            % self.chr_bank_count_4k();
        let offset = addr as usize & 0x0FFF;
        (bank * 0x1000 + offset) % self.chr.len()
    }
}

impl Mapper for Sunsoft1 {
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

    fn cpu_rom_offset(&self, addr: u16) -> Option<usize> {
        if addr < 0x8000 {
            return None;
        }
        let offset = if self.prg_rom.len() <= 0x4000 {
            (addr as usize - 0x8000) % 0x4000
        } else {
            addr as usize - 0x8000
        };
        Some(offset % self.prg_rom.len())
    }

    fn cpu_write(&mut self, addr: u16, val: u8) {
        if (0x6000..=0x7FFF).contains(&addr) {
            self.chr_low_bank = val & 0x07;
            self.chr_high_bank = (val >> 4) & 0x07;
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
        w.write_u8(self.chr_low_bank);
        w.write_u8(self.chr_high_bank);
        crate::save_state::write_chr_state(w, &self.chr);
    }

    fn read_state(&mut self, r: &mut crate::save_state::StateReader) -> anyhow::Result<()> {
        self.chr_low_bank = r.read_u8()? & 0x07;
        self.chr_high_bank = r.read_u8()? & 0x07;
        crate::save_state::read_chr_state(r, &mut self.chr, "Sunsoft-1")?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chr_banks(count: usize) -> Vec<u8> {
        let mut chr = Vec::new();
        for bank in 0..count {
            chr.extend(vec![bank as u8; 0x1000]);
        }
        chr
    }

    #[test]
    fn switches_two_4k_chr_banks() {
        let mut mapper = Sunsoft1::new(vec![0xEA; 0x8000], chr_banks(8), Mirroring::Horizontal);

        mapper.cpu_write(0x6000, 0x62);

        assert_eq!(mapper.chr_read(0x0000), 2);
        assert_eq!(mapper.chr_read(0x1000), 6);
    }
}
