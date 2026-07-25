use crate::hardware::cartridge::{Mapper, Mirroring};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Namco108Variant {
    Mapper88,
    DxRom,
}

pub struct Namco108 {
    prg_rom: Vec<u8>,
    chr: Vec<u8>,
    mirroring: Mirroring,
    variant: Namco108Variant,
    bank_select: u8,
    bank_registers: [u8; 8],
}

impl Namco108 {
    pub fn new_mapper88(prg_rom: Vec<u8>, chr: Vec<u8>, mirroring: Mirroring) -> Self {
        Self::new(prg_rom, chr, mirroring, Namco108Variant::Mapper88)
    }

    pub fn new_dxrom(prg_rom: Vec<u8>, chr: Vec<u8>, mirroring: Mirroring) -> Self {
        Self::new(prg_rom, chr, mirroring, Namco108Variant::DxRom)
    }

    fn new(prg_rom: Vec<u8>, chr: Vec<u8>, mirroring: Mirroring, variant: Namco108Variant) -> Self {
        Self {
            prg_rom,
            chr,
            mirroring,
            variant,
            bank_select: 0,
            bank_registers: [0; 8],
        }
    }

    fn prg_bank_count_8k(&self) -> usize {
        (self.prg_rom.len() / 0x2000).max(1)
    }

    fn chr_bank_count_1k(&self) -> usize {
        (self.chr.len() / 0x0400).max(1)
    }

    fn map_prg_bank(&self, addr: u16) -> usize {
        let bank_count = self.prg_bank_count_8k();
        match addr {
            0x8000..=0x9FFF => self.bank_registers[6] as usize % bank_count,
            0xA000..=0xBFFF => self.bank_registers[7] as usize % bank_count,
            0xC000..=0xDFFF => bank_count.saturating_sub(2),
            0xE000..=0xFFFF => bank_count.saturating_sub(1),
            _ => 0,
        }
    }

    fn map_chr_bank(&self, addr: u16) -> usize {
        let bank = match addr {
            0x0000..=0x07FF => (self.bank_registers[0] & 0x3E) as usize + (addr as usize / 0x0400),
            0x0800..=0x0FFF => {
                (self.bank_registers[1] & 0x3E) as usize + ((addr as usize / 0x0400) & 1)
            }
            0x1000..=0x13FF => self.bank_registers[2] as usize,
            0x1400..=0x17FF => self.bank_registers[3] as usize,
            0x1800..=0x1BFF => self.bank_registers[4] as usize,
            0x1C00..=0x1FFF => self.bank_registers[5] as usize,
            _ => 0,
        };

        let bank = match (self.variant, addr) {
            (Namco108Variant::Mapper88, 0x0000..=0x0FFF) => bank & 0x3F,
            (Namco108Variant::Mapper88, 0x1000..=0x1FFF) => (bank & 0x3F) | 0x40,
            _ => bank & 0x3F,
        };
        bank % self.chr_bank_count_1k()
    }
}

impl Mapper for Namco108 {
    fn cpu_peek(&self, addr: u16) -> u8 {
        match addr {
            0x8000..=0xFFFF => {
                let bank = self.map_prg_bank(addr);
                let offset = addr as usize & 0x1FFF;
                self.prg_rom[(bank * 0x2000 + offset) % self.prg_rom.len()]
            }
            _ => 0,
        }
    }

    fn cpu_write(&mut self, addr: u16, val: u8) {
        match addr {
            0x8000..=0x9FFE if addr & 1 == 0 => self.bank_select = val & 0x07,
            0x8001..=0x9FFF if addr & 1 != 0 => {
                self.bank_registers[self.bank_select as usize] = val;
            }
            _ => {}
        }
    }

    fn chr_read(&mut self, addr: u16) -> u8 {
        if self.chr.is_empty() {
            return 0;
        }
        let bank = self.map_chr_bank(addr);
        let offset = addr as usize & 0x03FF;
        self.chr[(bank * 0x0400 + offset) % self.chr.len()]
    }

    fn chr_write(&mut self, addr: u16, val: u8) {
        if self.chr.is_empty() {
            return;
        }
        let bank = self.map_chr_bank(addr);
        let offset = addr as usize & 0x03FF;
        let idx = (bank * 0x0400 + offset) % self.chr.len();
        self.chr[idx] = val;
    }

    fn mirroring(&self) -> Mirroring {
        self.mirroring
    }

    fn write_state(&self, w: &mut crate::save_state::StateWriter) {
        w.write_u8(self.bank_select);
        w.write_bytes(&self.bank_registers);
        crate::save_state::write_chr_state(w, &self.chr);
    }

    fn read_state(&mut self, r: &mut crate::save_state::StateReader) -> anyhow::Result<()> {
        self.bank_select = r.read_u8()? & 0x07;
        r.read_exact(&mut self.bank_registers)?;
        crate::save_state::read_chr_state(r, &mut self.chr, "Namco 108")?;
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
    fn switches_prg_and_chr_with_mapper88_high_chr_half() {
        let mut mapper = Namco108::new_mapper88(prg_banks(16), chr_banks(128), Mirroring::Vertical);

        mapper.cpu_write(0x8000, 0x06);
        mapper.cpu_write(0x8001, 0x03);
        mapper.cpu_write(0x8000, 0x07);
        mapper.cpu_write(0x8001, 0x04);
        mapper.cpu_write(0x8000, 0x00);
        mapper.cpu_write(0x8001, 0x06);
        mapper.cpu_write(0x8000, 0x02);
        mapper.cpu_write(0x8001, 0x05);

        assert_eq!(mapper.cpu_peek(0x8000), 3);
        assert_eq!(mapper.cpu_peek(0xA000), 4);
        assert_eq!(mapper.cpu_peek(0xC000), 14);
        assert_eq!(mapper.chr_read(0x0000), 6);
        assert_eq!(mapper.chr_read(0x0400), 7);
        assert_eq!(mapper.chr_read(0x1000), 0x45);
    }
}
