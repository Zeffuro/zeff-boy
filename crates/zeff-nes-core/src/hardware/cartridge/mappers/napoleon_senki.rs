use crate::hardware::cartridge::{Mapper, Mirroring};

pub struct NapoleonSenki {
    prg_rom: Vec<u8>,
    chr_rom: Vec<u8>,
    chr_ram: [u8; 0x2000],
    prg_bank: u8,
    chr_bank: u8,
}

impl NapoleonSenki {
    pub fn new(prg_rom: Vec<u8>, chr_rom: Vec<u8>) -> Self {
        Self {
            prg_rom,
            chr_rom,
            chr_ram: [0; 0x2000],
            prg_bank: 0,
            chr_bank: 0,
        }
    }

    fn prg_bank_count_32k(&self) -> usize {
        (self.prg_rom.len() / 0x8000).max(1)
    }

    fn chr_rom_bank_count_2k(&self) -> usize {
        (self.chr_rom.len() / 0x0800).max(1)
    }
}

impl Mapper for NapoleonSenki {
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
        if addr >= 0x8000 {
            self.prg_bank = val & 0x0F;
            self.chr_bank = val >> 4;
        }
    }

    fn chr_read(&mut self, addr: u16) -> u8 {
        match addr {
            0x0000..=0x07FF => {
                if self.chr_rom.is_empty() {
                    0
                } else {
                    let bank = self.chr_bank as usize % self.chr_rom_bank_count_2k();
                    self.chr_rom[(bank * 0x0800 + addr as usize) % self.chr_rom.len()]
                }
            }
            0x0800..=0x1FFF => self.chr_ram[(addr - 0x0800) as usize],
            _ => 0,
        }
    }

    fn chr_write(&mut self, addr: u16, val: u8) {
        if (0x0800..=0x1FFF).contains(&addr) {
            self.chr_ram[(addr - 0x0800) as usize] = val;
        }
    }

    fn ppu_nametable_read(&mut self, addr: u16, _ciram: &[u8]) -> Option<u8> {
        let offset = (addr - 0x2000) & 0x0FFF;
        if offset < 0x0800 {
            Some(self.chr_ram[0x1800 + offset as usize])
        } else {
            None
        }
    }

    fn ppu_nametable_write(&mut self, addr: u16, val: u8, _ciram: &mut [u8]) -> bool {
        let offset = (addr - 0x2000) & 0x0FFF;
        if offset < 0x0800 {
            self.chr_ram[0x1800 + offset as usize] = val;
            true
        } else {
            false
        }
    }

    fn mirroring(&self) -> Mirroring {
        Mirroring::FourScreen
    }

    fn write_state(&self, w: &mut crate::save_state::StateWriter) {
        w.write_bytes(&self.chr_ram);
        w.write_u8(self.prg_bank);
        w.write_u8(self.chr_bank);
    }

    fn read_state(&mut self, r: &mut crate::save_state::StateReader) -> anyhow::Result<()> {
        r.read_exact(&mut self.chr_ram)?;
        self.prg_bank = r.read_u8()? & 0x0F;
        self.chr_bank = r.read_u8()? & 0x0F;
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
            chr.extend(vec![bank as u8; 0x0800]);
        }
        chr
    }

    #[test]
    fn switches_prg_and_chr_rom_and_maps_cartridge_nametables() {
        let mut mapper = NapoleonSenki::new(prg_banks(4), chr_banks(16));
        let mut ciram = [0; 0x1000];

        mapper.cpu_write(0x8000, 0x21);
        assert_eq!(mapper.cpu_peek(0x8000), 0x01);
        assert_eq!(mapper.chr_read(0x0000), 0x02);

        mapper.chr_write(0x0800, 0xA5);
        assert_eq!(mapper.chr_read(0x0800), 0xA5);

        assert!(mapper.ppu_nametable_write(0x2000, 0x5A, &mut ciram));
        assert_eq!(mapper.ppu_nametable_read(0x2000, &ciram), Some(0x5A));
        assert_eq!(mapper.ppu_nametable_read(0x2800, &ciram), None);
        assert_eq!(mapper.mirroring(), Mirroring::FourScreen);
    }
}
