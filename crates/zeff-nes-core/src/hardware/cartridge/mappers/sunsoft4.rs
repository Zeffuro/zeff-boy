use crate::hardware::cartridge::{Mapper, Mirroring};

pub struct Sunsoft4 {
    prg_rom: Vec<u8>,
    chr: Vec<u8>,
    prg_ram: [u8; 0x2000],
    mirroring: Mirroring,
    prg_bank: u8,
    prg_ram_enabled: bool,
    chr_banks: [u8; 4],
    nt_banks: [u8; 2],
    nt_control: u8,
}

impl Sunsoft4 {
    pub fn new(prg_rom: Vec<u8>, chr: Vec<u8>, mirroring: Mirroring) -> Self {
        Self {
            prg_rom,
            chr,
            prg_ram: [0; 0x2000],
            mirroring,
            prg_bank: 0,
            prg_ram_enabled: false,
            chr_banks: [0; 4],
            nt_banks: [0; 2],
            nt_control: 0,
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

    fn chr_bank_count_2k(&self) -> usize {
        (self.chr.len() / 0x0800).max(1)
    }

    fn chr_bank_count_1k(&self) -> usize {
        (self.chr.len() / 0x0400).max(1)
    }

    fn chr_addr(&self, addr: u16) -> usize {
        let slot = (addr as usize / 0x0800) & 0x03;
        let bank = self.chr_banks[slot] as usize % self.chr_bank_count_2k();
        let offset = addr as usize & 0x07FF;
        (bank * 0x0800 + offset) % self.chr.len()
    }

    fn nt_rom_enabled(&self) -> bool {
        self.nt_control & 0x10 != 0
    }

    fn nt_slot(&self, addr: u16) -> usize {
        let table = ((addr - 0x2000) & 0x0FFF) / 0x0400;
        match self.nt_control & 0x03 {
            0 => (table & 0x01) as usize,
            1 => ((table >> 1) & 0x01) as usize,
            2 => 0,
            3 => 1,
            _ => unreachable!(),
        }
    }

    fn nt_chr_addr(&self, addr: u16) -> usize {
        let slot = self.nt_slot(addr);
        let bank = (0x80 | (self.nt_banks[slot] & 0x7F)) as usize % self.chr_bank_count_1k();
        let offset = addr as usize & 0x03FF;
        (bank * 0x0400 + offset) % self.chr.len()
    }

    fn sync_mirroring(&mut self) {
        self.mirroring = match self.nt_control & 0x03 {
            0 => Mirroring::Vertical,
            1 => Mirroring::Horizontal,
            2 => Mirroring::SingleScreenLower,
            3 => Mirroring::SingleScreenUpper,
            _ => unreachable!(),
        };
    }
}

impl Mapper for Sunsoft4 {
    fn cpu_peek(&self, addr: u16) -> u8 {
        match addr {
            0x6000..=0x7FFF if self.prg_ram_enabled => self.prg_ram[(addr - 0x6000) as usize],
            0x8000..=0xBFFF => self.prg_read_16k(self.prg_bank as usize, addr, 0x8000),
            0xC000..=0xFFFF => {
                self.prg_read_16k(self.prg_bank_count_16k().saturating_sub(1), addr, 0xC000)
            }
            _ => 0,
        }
    }

    fn cpu_write(&mut self, addr: u16, val: u8) {
        match addr {
            0x6000..=0x7FFF if self.prg_ram_enabled => {
                self.prg_ram[(addr - 0x6000) as usize] = val;
            }
            0x8000..=0x8FFF => self.chr_banks[0] = val,
            0x9000..=0x9FFF => self.chr_banks[1] = val,
            0xA000..=0xAFFF => self.chr_banks[2] = val,
            0xB000..=0xBFFF => self.chr_banks[3] = val,
            0xC000..=0xCFFF => self.nt_banks[0] = val & 0x7F,
            0xD000..=0xDFFF => self.nt_banks[1] = val & 0x7F,
            0xE000..=0xEFFF => {
                self.nt_control = val & 0x13;
                self.sync_mirroring();
            }
            0xF000..=0xFFFF => {
                self.prg_bank = val & 0x0F;
                self.prg_ram_enabled = val & 0x10 != 0;
            }
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

    fn ppu_nametable_read(&mut self, addr: u16, _ciram: &[u8]) -> Option<u8> {
        if !self.nt_rom_enabled() || self.chr.is_empty() {
            return None;
        }
        Some(self.chr[self.nt_chr_addr(addr)])
    }

    fn ppu_nametable_write(&mut self, _addr: u16, _val: u8, _ciram: &mut [u8]) -> bool {
        self.nt_rom_enabled()
    }

    fn mirroring(&self) -> Mirroring {
        self.mirroring
    }

    fn write_state(&self, w: &mut crate::save_state::StateWriter) {
        w.write_bytes(&self.prg_ram);
        w.write_u8(crate::save_state::encode_mirroring(self.mirroring));
        w.write_u8(self.prg_bank);
        w.write_bool(self.prg_ram_enabled);
        w.write_bytes(&self.chr_banks);
        w.write_bytes(&self.nt_banks);
        w.write_u8(self.nt_control);
        crate::save_state::write_chr_state(w, &self.chr);
    }

    fn read_state(&mut self, r: &mut crate::save_state::StateReader) -> anyhow::Result<()> {
        r.read_exact(&mut self.prg_ram)?;
        self.mirroring = crate::save_state::decode_mirroring(r.read_u8()?)?;
        self.prg_bank = r.read_u8()? & 0x0F;
        self.prg_ram_enabled = r.read_bool()?;
        r.read_exact(&mut self.chr_banks)?;
        r.read_exact(&mut self.nt_banks)?;
        self.nt_control = r.read_u8()? & 0x13;
        for bank in &mut self.nt_banks {
            *bank &= 0x7F;
        }
        self.sync_mirroring();
        crate::save_state::read_chr_state(r, &mut self.chr, "Sunsoft-4")?;
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

    fn chr_2k_banks(count: usize) -> Vec<u8> {
        let mut chr = Vec::new();
        for bank in 0..count {
            chr.extend(vec![bank as u8; 0x0800]);
        }
        chr
    }

    fn chr_1k_banks(count: usize) -> Vec<u8> {
        let mut chr = Vec::new();
        for bank in 0..count {
            chr.extend(vec![bank as u8; 0x0400]);
        }
        chr
    }

    #[test]
    fn switches_prg_chr_and_prg_ram_enable() {
        let mut mapper = Sunsoft4::new(prg_banks(8), chr_2k_banks(8), Mirroring::Vertical);

        mapper.cpu_write(0xF000, 0x13);
        mapper.cpu_write(0x6000, 0xA5);
        mapper.cpu_write(0x8000, 0x04);
        mapper.cpu_write(0x9000, 0x05);
        mapper.cpu_write(0xA000, 0x06);
        mapper.cpu_write(0xB000, 0x07);

        assert_eq!(mapper.cpu_peek(0x6000), 0xA5);
        assert_eq!(mapper.cpu_peek(0x8000), 3);
        assert_eq!(mapper.cpu_peek(0xC000), 7);
        assert_eq!(mapper.chr_read(0x0000), 4);
        assert_eq!(mapper.chr_read(0x0800), 5);
        assert_eq!(mapper.chr_read(0x1000), 6);
        assert_eq!(mapper.chr_read(0x1800), 7);
    }

    #[test]
    fn maps_rom_nametables_using_mapper_mirroring() {
        let mut mapper = Sunsoft4::new(prg_banks(8), chr_1k_banks(256), Mirroring::Vertical);
        let mut ciram = [0xEE; 0x800];

        mapper.cpu_write(0xC000, 0x02);
        mapper.cpu_write(0xD000, 0x03);
        mapper.cpu_write(0xE000, 0x10);

        assert_eq!(mapper.ppu_nametable_read(0x2000, &ciram), Some(0x82));
        assert_eq!(mapper.ppu_nametable_read(0x2400, &ciram), Some(0x83));
        assert_eq!(mapper.ppu_nametable_read(0x2800, &ciram), Some(0x82));
        assert_eq!(mapper.ppu_nametable_read(0x2C00, &ciram), Some(0x83));

        mapper.cpu_write(0xE000, 0x13);
        assert_eq!(mapper.mirroring(), Mirroring::SingleScreenUpper);
        assert_eq!(mapper.ppu_nametable_read(0x2000, &ciram), Some(0x83));
        assert!(mapper.ppu_nametable_write(0x2000, 0x55, &mut ciram));
    }
}
