use crate::hardware::cartridge::{Mapper, Mirroring};

pub struct Mmc1 {
    prg_rom: Vec<u8>,
    chr: Vec<u8>,
    prg_ram: [u8; 0x2000],
    mirroring: Mirroring,

    shift_register: u8,
    shift_count: u8,

    control: u8,

    chr_bank_0: u8,
    chr_bank_1: u8,
    prg_bank: u8,

    write_suppression_cycles: u8,
}

impl Mmc1 {
    pub fn new(prg_rom: Vec<u8>, chr: Vec<u8>, mirroring: Mirroring) -> Self {
        Self {
            prg_rom,
            chr,
            prg_ram: [0; 0x2000],
            mirroring,
            shift_register: 0x10,
            shift_count: 0,
            control: 0x0C,
            chr_bank_0: 0,
            chr_bank_1: 0,
            prg_bank: 0,
            write_suppression_cycles: 0,
        }
    }

    fn prg_mode(&self) -> u8 {
        (self.control >> 2) & 0x03
    }

    fn chr_mode(&self) -> bool {
        self.control & 0x10 != 0
    }

    fn update_mirroring(&mut self) {
        self.mirroring = match self.control & 0x03 {
            0 => Mirroring::SingleScreenLower,
            1 => Mirroring::SingleScreenUpper,
            2 => Mirroring::Vertical,
            3 => Mirroring::Horizontal,
            _ => unreachable!(),
        };
    }

    fn write_register(&mut self, addr: u16) {
        let value = self.shift_register;
        match addr {
            0x8000..=0x9FFF => {
                self.control = value & 0x1F;
                self.update_mirroring();
            }
            0xA000..=0xBFFF => {
                self.chr_bank_0 = value & 0x1F;
            }
            0xC000..=0xDFFF => {
                self.chr_bank_1 = value & 0x1F;
            }
            0xE000..=0xFFFF => {
                self.prg_bank = value & 0x0F;
            }
            _ => {}
        }
    }

    fn prg_bank_count(&self) -> usize {
        self.prg_rom.len() / 0x4000
    }

    fn uses_large_chr_ram_prg_layout(&self) -> bool {
        self.chr.len() <= 0x2000 && self.prg_bank_count() > 16
    }

    fn prg_outer_bank_count(&self) -> usize {
        if self.uses_large_chr_ram_prg_layout() {
            self.prg_bank_count().div_ceil(16)
        } else {
            1
        }
    }

    fn prg_outer_bank_base(&self) -> usize {
        let outer_count = self.prg_outer_bank_count();
        if outer_count == 1 {
            return 0;
        }

        let outer_bank = if outer_count <= 2 {
            usize::from((self.chr_bank_0 >> 4) & 0x01)
        } else {
            usize::from((self.chr_bank_0 >> 3) & 0x03)
        };

        (outer_bank % outer_count) * 16
    }

    fn prg_inner_bank_count(&self) -> usize {
        if self.uses_large_chr_ram_prg_layout() {
            16
        } else {
            self.prg_bank_count().max(1)
        }
    }

    fn prg_rom_index(&self, bank: usize, offset: usize) -> usize {
        let bank_count = self.prg_bank_count().max(1);
        (bank % bank_count) * 0x4000 + offset
    }

    fn chr_index(&self, addr: u16) -> usize {
        let raw = if self.chr_mode() {
            match addr {
                0x0000..=0x0FFF => (self.chr_bank_0 as usize) * 0x1000 + addr as usize,
                0x1000..=0x1FFF => (self.chr_bank_1 as usize) * 0x1000 + (addr - 0x1000) as usize,
                _ => addr as usize,
            }
        } else {
            (self.chr_bank_0 as usize >> 1) * 0x2000 + addr as usize
        };
        raw % self.chr.len()
    }
}

impl Mapper for Mmc1 {
    fn cpu_peek(&self, addr: u16) -> u8 {
        match addr {
            0x6000..=0x7FFF => self.prg_ram[(addr - 0x6000) as usize],
            0x8000..=0xBFFF => {
                let outer = self.prg_outer_bank_base();
                let inner_count = self.prg_inner_bank_count();
                let bank = match self.prg_mode() {
                    0 | 1 => outer + ((self.prg_bank as usize & 0x0E) % inner_count),
                    2 => outer,
                    3 => outer + ((self.prg_bank as usize) % inner_count),
                    _ => unreachable!(),
                };
                let offset = (addr - 0x8000) as usize;
                self.prg_rom[self.prg_rom_index(bank, offset)]
            }
            0xC000..=0xFFFF => {
                let outer = self.prg_outer_bank_base();
                let inner_count = self.prg_inner_bank_count();
                let bank = match self.prg_mode() {
                    0 | 1 => outer + ((self.prg_bank as usize | 0x01) % inner_count),
                    2 => outer + ((self.prg_bank as usize) % inner_count),
                    3 => outer + inner_count - 1,
                    _ => unreachable!(),
                };
                let offset = (addr - 0xC000) as usize;
                self.prg_rom[self.prg_rom_index(bank, offset)]
            }
            _ => 0,
        }
    }

    fn cpu_write(&mut self, addr: u16, val: u8) {
        match addr {
            0x6000..=0x7FFF => {
                self.prg_ram[(addr - 0x6000) as usize] = val;
            }
            0x8000..=0xFFFF => {
                if val & 0x80 == 0 && self.write_suppression_cycles > 0 {
                    self.write_suppression_cycles = 2;
                    return;
                }

                self.write_suppression_cycles = 2;
                if val & 0x80 != 0 {
                    self.shift_register = 0x10;
                    self.shift_count = 0;
                    self.control |= 0x0C;
                } else {
                    self.shift_register >>= 1;
                    self.shift_register |= (val & 0x01) << 4;
                    self.shift_count += 1;
                    if self.shift_count == 5 {
                        self.write_register(addr);
                        self.shift_register = 0x10;
                        self.shift_count = 0;
                    }
                }
            }
            _ => {}
        }
    }

    fn clock_cpu(&mut self) {
        self.write_suppression_cycles = self.write_suppression_cycles.saturating_sub(1);
    }

    fn chr_read(&mut self, addr: u16) -> u8 {
        if self.chr.is_empty() {
            return 0;
        }
        self.chr[self.chr_index(addr)]
    }

    fn chr_write(&mut self, addr: u16, val: u8) {
        if self.chr.is_empty() {
            return;
        }
        let idx = self.chr_index(addr);
        self.chr[idx] = val;
    }

    fn mirroring(&self) -> Mirroring {
        self.mirroring
    }

    fn write_state(&self, w: &mut crate::save_state::StateWriter) {
        w.write_bytes(&self.prg_ram);
        w.write_u8(self.shift_register);
        w.write_u8(self.shift_count);
        w.write_u8(self.control);
        w.write_u8(self.chr_bank_0);
        w.write_u8(self.chr_bank_1);
        w.write_u8(self.prg_bank);
        w.write_u8(crate::save_state::encode_mirroring(self.mirroring));
        crate::save_state::write_chr_state(w, &self.chr);
    }

    fn read_state(&mut self, r: &mut crate::save_state::StateReader) -> anyhow::Result<()> {
        r.read_exact(&mut self.prg_ram)?;
        self.shift_register = r.read_u8()?;
        self.shift_count = r.read_u8()?;
        self.control = r.read_u8()?;
        self.chr_bank_0 = r.read_u8()?;
        self.chr_bank_1 = r.read_u8()?;
        self.prg_bank = r.read_u8()?;
        self.mirroring = crate::save_state::decode_mirroring(r.read_u8()?)?;
        crate::save_state::read_chr_state(r, &mut self.chr, "MMC1")?;
        self.write_suppression_cycles = 0;
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

    fn write_serial(mapper: &mut Mmc1, addr: u16, value: u8) {
        for bit in 0..5 {
            mapper.cpu_write(addr, (value >> bit) & 0x01);
            mapper.clock_cpu();
            mapper.clock_cpu();
        }
    }

    #[test]
    fn ignores_data_write_immediately_after_reset_write() {
        let mut mapper = Mmc1::new(prg_banks(8), vec![0; 0x2000], Mirroring::Vertical);
        mapper.shift_register = 0x0B;
        mapper.shift_count = 3;

        mapper.cpu_write(0xFFFF, 0xFF);
        mapper.cpu_write(0xFFFF, 0x00);

        assert_eq!(mapper.shift_register, 0x10);
        assert_eq!(mapper.shift_count, 0);
    }

    #[test]
    fn accepts_data_write_after_suppression_window_expires() {
        let mut mapper = Mmc1::new(prg_banks(8), vec![0; 0x2000], Mirroring::Vertical);

        mapper.cpu_write(0xFFFF, 0xFF);
        mapper.clock_cpu();
        mapper.clock_cpu();
        mapper.cpu_write(0xE000, 0x01);

        assert_eq!(mapper.shift_count, 1);
        assert_eq!(mapper.shift_register, 0x18);
    }

    #[test]
    fn ignores_consecutive_data_write() {
        let mut mapper = Mmc1::new(prg_banks(8), vec![0; 0x2000], Mirroring::Vertical);

        mapper.cpu_write(0xE000, 0x01);
        mapper.cpu_write(0xE000, 0x00);

        assert_eq!(mapper.shift_count, 1);
        assert_eq!(mapper.shift_register, 0x18);
    }

    #[test]
    fn large_chr_ram_prg_uses_chr_bank_bit4_as_256k_outer_bank() {
        let mut mapper = Mmc1::new(prg_banks(32), vec![0; 0x2000], Mirroring::Vertical);

        assert_eq!(mapper.cpu_peek(0xC000), 15);

        write_serial(&mut mapper, 0xA000, 0x10);
        write_serial(&mut mapper, 0xE000, 0x02);

        assert_eq!(mapper.cpu_peek(0x8000), 18);
        assert_eq!(mapper.cpu_peek(0xC000), 31);
    }

    #[test]
    fn large_chr_ram_prg_can_use_two_outer_bits_for_1m_layout() {
        let mut mapper = Mmc1::new(prg_banks(64), vec![0; 0x2000], Mirroring::Vertical);

        write_serial(&mut mapper, 0xA000, 0x18);
        write_serial(&mut mapper, 0xE000, 0x03);

        assert_eq!(mapper.cpu_peek(0x8000), 51);
        assert_eq!(mapper.cpu_peek(0xC000), 63);
    }

    #[test]
    fn chr_rom_large_prg_keeps_regular_mmc1_fixed_last_bank() {
        let mut mapper = Mmc1::new(prg_banks(32), vec![0; 0x4000], Mirroring::Vertical);

        write_serial(&mut mapper, 0xA000, 0x10);
        write_serial(&mut mapper, 0xE000, 0x02);

        assert_eq!(mapper.cpu_peek(0x8000), 2);
        assert_eq!(mapper.cpu_peek(0xC000), 31);
    }
}
