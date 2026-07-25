use crate::hardware::cartridge::{Mapper, Mirroring};

pub struct TaitoX1017 {
    prg_rom: Vec<u8>,
    chr: Vec<u8>,
    ram: [u8; 0x1400],
    has_battery: bool,
    ram_enabled: [bool; 3],
    mirroring: Mirroring,
    fixed_four_screen: bool,
    chr_banks: [u8; 6],
    chr_invert: bool,
    prg_banks: [u8; 3],
}

impl TaitoX1017 {
    pub fn new(prg_rom: Vec<u8>, chr: Vec<u8>, mirroring: Mirroring, has_battery: bool) -> Self {
        let prg_bank_count = (prg_rom.len() / 0x2000).max(1);
        Self {
            prg_rom,
            chr,
            ram: [0; 0x1400],
            has_battery,
            ram_enabled: [false; 3],
            mirroring,
            fixed_four_screen: matches!(mirroring, Mirroring::FourScreen),
            chr_banks: [0, 2, 4, 5, 6, 7],
            chr_invert: false,
            prg_banks: [0, 1, prg_bank_count.saturating_sub(2) as u8],
        }
    }

    fn prg_bank_count_8k(&self) -> usize {
        (self.prg_rom.len() / 0x2000).max(1)
    }

    fn chr_bank_count_1k(&self) -> usize {
        (self.chr.len() / 0x0400).max(1)
    }

    fn prg_bank_from_register(val: u8) -> u8 {
        (val >> 2) & 0x0F
    }

    fn prg_read_bank(&self, bank: usize, addr: u16) -> u8 {
        let bank = bank % self.prg_bank_count_8k();
        let offset = (addr as usize) & 0x1FFF;
        self.prg_rom[(bank * 0x2000 + offset) % self.prg_rom.len()]
    }

    fn chr_bank(&self, addr: u16) -> usize {
        let bank = match (self.chr_invert, addr) {
            (false, 0x0000..=0x07FF) => {
                (self.chr_banks[0] & !1).wrapping_add(((addr >> 10) & 1) as u8)
            }
            (false, 0x0800..=0x0FFF) => {
                (self.chr_banks[1] & !1).wrapping_add((((addr - 0x0800) >> 10) & 1) as u8)
            }
            (false, 0x1000..=0x13FF) => self.chr_banks[2],
            (false, 0x1400..=0x17FF) => self.chr_banks[3],
            (false, 0x1800..=0x1BFF) => self.chr_banks[4],
            (false, _) => self.chr_banks[5],

            (true, 0x0000..=0x03FF) => self.chr_banks[2],
            (true, 0x0400..=0x07FF) => self.chr_banks[3],
            (true, 0x0800..=0x0BFF) => self.chr_banks[4],
            (true, 0x0C00..=0x0FFF) => self.chr_banks[5],
            (true, 0x1000..=0x17FF) => {
                (self.chr_banks[0] & !1).wrapping_add((((addr - 0x1000) >> 10) & 1) as u8)
            }
            (true, _) => (self.chr_banks[1] & !1).wrapping_add((((addr - 0x1800) >> 10) & 1) as u8),
        };
        bank as usize % self.chr_bank_count_1k()
    }

    fn chr_addr(&self, addr: u16) -> usize {
        let bank = self.chr_bank(addr);
        let offset = (addr as usize) & 0x03FF;
        (bank * 0x0400 + offset) % self.chr.len()
    }

    fn ram_region(addr: u16) -> Option<(usize, usize)> {
        match addr {
            0x6000..=0x67FF => Some((0, (addr - 0x6000) as usize)),
            0x6800..=0x6FFF => Some((1, 0x0800 + (addr - 0x6800) as usize)),
            0x7000..=0x73FF => Some((2, 0x1000 + (addr - 0x7000) as usize)),
            _ => None,
        }
    }
}

impl Mapper for TaitoX1017 {
    fn cpu_peek(&self, addr: u16) -> u8 {
        if let Some((region, offset)) = Self::ram_region(addr) {
            return if self.ram_enabled[region] {
                self.ram[offset]
            } else {
                0
            };
        }

        match addr {
            0x8000..=0x9FFF => self.prg_read_bank(self.prg_banks[0] as usize, addr),
            0xA000..=0xBFFF => self.prg_read_bank(self.prg_banks[1] as usize, addr),
            0xC000..=0xDFFF => self.prg_read_bank(self.prg_banks[2] as usize, addr),
            0xE000..=0xFFFF => self.prg_read_bank(self.prg_bank_count_8k() - 1, addr),
            _ => 0,
        }
    }

    fn cpu_write(&mut self, addr: u16, val: u8) {
        match addr {
            0x7EF0..=0x7EF5 => self.chr_banks[(addr - 0x7EF0) as usize] = val,
            0x7EF6 => {
                if !self.fixed_four_screen {
                    self.mirroring = if val & 0x01 != 0 {
                        Mirroring::Vertical
                    } else {
                        Mirroring::Horizontal
                    };
                }
                self.chr_invert = val & 0x02 != 0;
            }
            0x7EF7 => self.ram_enabled[0] = val == 0xCA,
            0x7EF8 => self.ram_enabled[1] = val == 0x69,
            0x7EF9 => self.ram_enabled[2] = val == 0x84,
            0x7EFA => self.prg_banks[0] = Self::prg_bank_from_register(val),
            0x7EFB => self.prg_banks[1] = Self::prg_bank_from_register(val),
            0x7EFC => self.prg_banks[2] = Self::prg_bank_from_register(val),
            _ => {
                if let Some((region, offset)) = Self::ram_region(addr)
                    && self.ram_enabled[region]
                {
                    self.ram[offset] = val;
                }
            }
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
        w.write_bytes(&self.ram);
        w.write_bool(self.has_battery);
        for &enabled in &self.ram_enabled {
            w.write_bool(enabled);
        }
        w.write_u8(crate::save_state::encode_mirroring(self.mirroring));
        w.write_bool(self.fixed_four_screen);
        w.write_bytes(&self.chr_banks);
        w.write_bool(self.chr_invert);
        w.write_bytes(&self.prg_banks);
        crate::save_state::write_chr_state(w, &self.chr);
    }

    fn read_state(&mut self, r: &mut crate::save_state::StateReader) -> anyhow::Result<()> {
        r.read_exact(&mut self.ram)?;
        self.has_battery = r.read_bool()?;
        for enabled in &mut self.ram_enabled {
            *enabled = r.read_bool()?;
        }
        self.mirroring = crate::save_state::decode_mirroring(r.read_u8()?)?;
        self.fixed_four_screen = r.read_bool()?;
        r.read_exact(&mut self.chr_banks)?;
        self.chr_invert = r.read_bool()?;
        r.read_exact(&mut self.prg_banks)?;
        crate::save_state::read_chr_state(r, &mut self.chr, "Taito X1-017")?;
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
    fn switches_prg_chr_and_mirroring() {
        let mut mapper = TaitoX1017::new(prg_banks(16), chr_banks(32), Mirroring::Horizontal, true);

        mapper.cpu_write(0x7EFA, 0x0C);
        mapper.cpu_write(0x7EFB, 0x10);
        mapper.cpu_write(0x7EFC, 0x14);
        mapper.cpu_write(0x7EF0, 0x06);
        mapper.cpu_write(0x7EF1, 0x08);
        mapper.cpu_write(0x7EF5, 0x0E);
        mapper.cpu_write(0x7EF6, 0x01);

        assert_eq!(mapper.cpu_peek(0x8000), 3);
        assert_eq!(mapper.cpu_peek(0xA000), 4);
        assert_eq!(mapper.cpu_peek(0xC000), 5);
        assert_eq!(mapper.cpu_peek(0xE000), 15);
        assert_eq!(mapper.chr_read(0x0000), 6);
        assert_eq!(mapper.chr_read(0x0400), 7);
        assert_eq!(mapper.chr_read(0x0800), 8);
        assert_eq!(mapper.chr_read(0x1C00), 14);
        assert_eq!(mapper.mirroring(), Mirroring::Vertical);
    }

    #[test]
    fn gates_three_ram_regions() {
        let mut mapper = TaitoX1017::new(prg_banks(8), chr_banks(8), Mirroring::Horizontal, true);

        mapper.cpu_write(0x6000, 0x11);
        assert_eq!(mapper.cpu_peek(0x6000), 0);

        mapper.cpu_write(0x7EF7, 0xCA);
        mapper.cpu_write(0x7EF8, 0x69);
        mapper.cpu_write(0x7EF9, 0x84);
        mapper.cpu_write(0x6000, 0x11);
        mapper.cpu_write(0x6800, 0x22);
        mapper.cpu_write(0x7000, 0x33);

        assert_eq!(mapper.cpu_peek(0x6000), 0x11);
        assert_eq!(mapper.cpu_peek(0x6800), 0x22);
        assert_eq!(mapper.cpu_peek(0x7000), 0x33);
    }
}
