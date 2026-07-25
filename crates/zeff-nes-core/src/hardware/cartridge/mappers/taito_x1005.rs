use crate::hardware::cartridge::{Mapper, Mirroring};

pub struct TaitoX1005 {
    prg_rom: Vec<u8>,
    chr: Vec<u8>,
    ram: [u8; 0x80],
    has_battery: bool,
    ram_enabled: bool,
    mirroring: Mirroring,
    fixed_four_screen: bool,
    chr_banks: [u8; 6],
    prg_banks: [u8; 3],
}

impl TaitoX1005 {
    pub fn new(prg_rom: Vec<u8>, chr: Vec<u8>, mirroring: Mirroring, has_battery: bool) -> Self {
        let prg_bank_count = (prg_rom.len() / 0x2000).max(1);
        Self {
            prg_rom,
            chr,
            ram: [0; 0x80],
            has_battery,
            ram_enabled: false,
            mirroring,
            fixed_four_screen: matches!(mirroring, Mirroring::FourScreen),
            chr_banks: [0, 2, 4, 5, 6, 7],
            prg_banks: [0, 1, prg_bank_count.saturating_sub(2) as u8],
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

    fn chr_bank(&self, addr: u16) -> usize {
        (match addr {
            0x0000..=0x07FF => ((self.chr_banks[0] & !0x01) as usize) + ((addr >> 10) as usize),
            0x0800..=0x0FFF => {
                ((self.chr_banks[1] & !0x01) as usize) + (((addr - 0x0800) >> 10) as usize)
            }
            0x1000..=0x13FF => self.chr_banks[2] as usize,
            0x1400..=0x17FF => self.chr_banks[3] as usize,
            0x1800..=0x1BFF => self.chr_banks[4] as usize,
            0x1C00..=0x1FFF => self.chr_banks[5] as usize,
            _ => 0,
        }) % self.chr_bank_count_1k()
    }

    fn chr_addr(&self, addr: u16) -> usize {
        let bank = self.chr_bank(addr);
        let offset = addr as usize & 0x03FF;
        (bank * 0x0400 + offset) % self.chr.len()
    }

    fn register_index(addr: u16) -> Option<u16> {
        if (addr & 0xFF70) == 0x7E70 {
            Some(addr & 0x000F)
        } else {
            None
        }
    }
}

impl Mapper for TaitoX1005 {
    fn cpu_peek(&self, addr: u16) -> u8 {
        match addr {
            0x7F00..=0x7FFF if self.ram_enabled => self.ram[addr as usize & 0x7F],
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
        if let Some(reg) = Self::register_index(addr) {
            match reg {
                0x0..=0x5 => self.chr_banks[reg as usize] = val,
                0x6 if !self.fixed_four_screen => {
                    self.mirroring = if val & 0x01 != 0 {
                        Mirroring::Vertical
                    } else {
                        Mirroring::Horizontal
                    };
                }
                0x8 | 0x9 => self.ram_enabled = val == 0xA3,
                0xA | 0xB => self.prg_banks[0] = val,
                0xC | 0xD => self.prg_banks[1] = val,
                0xE | 0xF => self.prg_banks[2] = val,
                _ => {}
            }
            return;
        }

        if (0x7F00..=0x7FFF).contains(&addr) && self.ram_enabled {
            self.ram[addr as usize & 0x7F] = val;
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

    fn dump_battery_data(&self) -> Option<Vec<u8>> {
        if self.has_battery {
            Some(self.ram.to_vec())
        } else {
            None
        }
    }

    fn load_battery_data(&mut self, bytes: &[u8]) -> anyhow::Result<()> {
        let copy_len = self.ram.len().min(bytes.len());
        self.ram[..copy_len].copy_from_slice(&bytes[..copy_len]);
        if copy_len < self.ram.len() {
            self.ram[copy_len..].fill(0);
        }
        Ok(())
    }

    fn write_state(&self, w: &mut crate::save_state::StateWriter) {
        w.write_bool(self.ram_enabled);
        w.write_u8(crate::save_state::encode_mirroring(self.mirroring));
        w.write_bytes(&self.chr_banks);
        w.write_bytes(&self.prg_banks);
        w.write_bytes(&self.ram);
        crate::save_state::write_chr_state(w, &self.chr);
    }

    fn read_state(&mut self, r: &mut crate::save_state::StateReader) -> anyhow::Result<()> {
        self.ram_enabled = r.read_bool()?;
        self.mirroring = crate::save_state::decode_mirroring(r.read_u8()?)?;
        r.read_exact(&mut self.chr_banks)?;
        r.read_exact(&mut self.prg_banks)?;
        r.read_exact(&mut self.ram)?;
        crate::save_state::read_chr_state(r, &mut self.chr, "Taito X1-005")?;
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
    fn switches_prg_and_chr_banks() {
        let mut mapper = TaitoX1005::new(prg_banks(8), chr_banks(16), Mirroring::Horizontal, false);

        mapper.cpu_write(0x7EFA, 0x03);
        mapper.cpu_write(0x7EFC, 0x04);
        mapper.cpu_write(0x7EFE, 0x05);
        mapper.cpu_write(0x7EF0, 0x06);
        mapper.cpu_write(0x7EF1, 0x08);
        mapper.cpu_write(0x7EF5, 0x0E);

        assert_eq!(mapper.cpu_peek(0x8000), 3);
        assert_eq!(mapper.cpu_peek(0xA000), 4);
        assert_eq!(mapper.cpu_peek(0xC000), 5);
        assert_eq!(mapper.cpu_peek(0xE000), 7);
        assert_eq!(mapper.chr_read(0x0000), 6);
        assert_eq!(mapper.chr_read(0x0400), 7);
        assert_eq!(mapper.chr_read(0x0800), 8);
        assert_eq!(mapper.chr_read(0x1C00), 14);
    }

    #[test]
    fn mirrors_registers_at_7e7x() {
        let mut mapper = TaitoX1005::new(prg_banks(8), chr_banks(8), Mirroring::Horizontal, false);

        mapper.cpu_write(0x7E7A, 0x03);
        assert_eq!(mapper.cpu_peek(0x8000), 3);
    }

    #[test]
    fn gates_internal_ram() {
        let mut mapper = TaitoX1005::new(prg_banks(8), chr_banks(8), Mirroring::Horizontal, true);

        mapper.cpu_write(0x7F00, 0x55);
        assert_eq!(mapper.cpu_peek(0x7F00), 0x00);

        mapper.cpu_write(0x7EF8, 0xA3);
        mapper.cpu_write(0x7F00, 0x55);
        assert_eq!(mapper.cpu_peek(0x7F80), 0x55);
    }

    #[test]
    fn switches_mirroring() {
        let mut mapper = TaitoX1005::new(prg_banks(8), chr_banks(8), Mirroring::Horizontal, false);

        mapper.cpu_write(0x7EF6, 0x01);
        assert_eq!(mapper.mirroring(), Mirroring::Vertical);
        mapper.cpu_write(0x7EF6, 0x00);
        assert_eq!(mapper.mirroring(), Mirroring::Horizontal);
    }
}
