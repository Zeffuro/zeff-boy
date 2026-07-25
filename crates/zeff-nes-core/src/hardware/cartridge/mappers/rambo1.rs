use crate::hardware::cartridge::{Mapper, Mirroring};

pub struct Rambo1 {
    prg_rom: Vec<u8>,
    chr: Vec<u8>,
    mirroring: Mirroring,
    fixed_four_screen: bool,

    bank_select: u8,
    bank_registers: [u8; 16],

    irq_latch: u8,
    irq_counter: u8,
    irq_reload_pending: bool,
    irq_enabled: bool,
    irq_pending: bool,
    irq_cycle_mode: bool,
    irq_cycle_divider: u8,
}

impl Rambo1 {
    pub fn new(prg_rom: Vec<u8>, chr: Vec<u8>, mirroring: Mirroring) -> Self {
        Self {
            prg_rom,
            chr,
            mirroring,
            fixed_four_screen: matches!(mirroring, Mirroring::FourScreen),
            bank_select: 0,
            bank_registers: [0; 16],
            irq_latch: 0,
            irq_counter: 0,
            irq_reload_pending: false,
            irq_enabled: false,
            irq_pending: false,
            irq_cycle_mode: false,
            irq_cycle_divider: 4,
        }
    }

    fn prg_bank_count_8k(&self) -> usize {
        (self.prg_rom.len() / 0x2000).max(1)
    }

    fn chr_bank_count_1k(&self) -> usize {
        (self.chr.len() / 0x0400).max(1)
    }

    fn read_prg_bank(&self, bank: usize, addr: u16) -> u8 {
        let bank = bank % self.prg_bank_count_8k();
        let offset = (addr as usize) & 0x1FFF;
        self.prg_rom[(bank * 0x2000 + offset) % self.prg_rom.len()]
    }

    fn map_prg_bank(&self, addr: u16) -> usize {
        let prg_mode = self.bank_select & 0x40 != 0;
        let r6 = self.bank_registers[6] as usize;
        let r7 = self.bank_registers[7] as usize;
        let rf = self.bank_registers[15] as usize;
        let last = self.prg_bank_count_8k().saturating_sub(1);

        match addr {
            0x8000..=0x9FFF if prg_mode => rf,
            0x8000..=0x9FFF => r6,
            0xA000..=0xBFFF => r7,
            0xC000..=0xDFFF if prg_mode => r6,
            0xC000..=0xDFFF => rf,
            0xE000..=0xFFFF => last,
            _ => 0,
        }
    }

    fn map_chr_bank(&self, addr: u16) -> usize {
        let slot = ((addr as usize) >> 10) & 0x07;
        let invert = self.bank_select & 0x80 != 0;
        let full_1k = self.bank_select & 0x20 != 0;

        let bank = match (invert, full_1k, slot) {
            (false, false, 0) => self.bank_registers[0] & !1,
            (false, false, 1) => (self.bank_registers[0] & !1).wrapping_add(1),
            (false, false, 2) => self.bank_registers[1] & !1,
            (false, false, 3) => (self.bank_registers[1] & !1).wrapping_add(1),
            (false, true, 0) => self.bank_registers[0],
            (false, true, 1) => self.bank_registers[8],
            (false, true, 2) => self.bank_registers[1],
            (false, true, 3) => self.bank_registers[9],
            (false, _, 4) => self.bank_registers[2],
            (false, _, 5) => self.bank_registers[3],
            (false, _, 6) => self.bank_registers[4],
            (false, _, 7) => self.bank_registers[5],

            (true, _, 0) => self.bank_registers[2],
            (true, _, 1) => self.bank_registers[3],
            (true, _, 2) => self.bank_registers[4],
            (true, _, 3) => self.bank_registers[5],
            (true, false, 4) => self.bank_registers[0] & !1,
            (true, false, 5) => (self.bank_registers[0] & !1).wrapping_add(1),
            (true, false, 6) => self.bank_registers[1] & !1,
            (true, false, 7) => (self.bank_registers[1] & !1).wrapping_add(1),
            (true, true, 4) => self.bank_registers[0],
            (true, true, 5) => self.bank_registers[8],
            (true, true, 6) => self.bank_registers[1],
            (true, true, 7) => self.bank_registers[9],
            _ => 0,
        };

        bank as usize % self.chr_bank_count_1k()
    }

    fn clock_irq_counter(&mut self) {
        if self.irq_reload_pending {
            self.irq_counter = self.irq_latch;
            if self.irq_counter != 0 {
                self.irq_counter |= 1;
            }
            self.irq_reload_pending = false;
        } else if self.irq_counter == 0 {
            self.irq_counter = self.irq_latch;
        } else {
            self.irq_counter -= 1;
        }

        if self.irq_counter == 0 && self.irq_enabled {
            self.irq_pending = true;
        }
    }
}

impl Mapper for Rambo1 {
    fn cpu_peek(&self, addr: u16) -> u8 {
        match addr {
            0x8000..=0xFFFF => self.read_prg_bank(self.map_prg_bank(addr), addr),
            _ => 0,
        }
    }

    fn cpu_write(&mut self, addr: u16, val: u8) {
        match addr {
            0x8000..=0x9FFF => {
                if addr & 1 == 0 {
                    self.bank_select = val;
                } else {
                    let register = (self.bank_select & 0x0F) as usize;
                    self.bank_registers[register] = val;
                }
            }
            0xA000..=0xBFFF => {
                if addr & 1 == 0 && !self.fixed_four_screen {
                    self.mirroring = if val & 0x01 == 0 {
                        Mirroring::Vertical
                    } else {
                        Mirroring::Horizontal
                    };
                }
            }
            0xC000..=0xDFFF => {
                if addr & 1 == 0 {
                    self.irq_latch = val;
                } else {
                    self.irq_cycle_mode = val & 0x01 != 0;
                    self.irq_reload_pending = true;
                    self.irq_cycle_divider = 4;
                }
            }
            0xE000..=0xFFFF => {
                if addr & 1 == 0 {
                    self.irq_enabled = false;
                    self.irq_pending = false;
                } else {
                    self.irq_enabled = true;
                }
            }
            _ => {}
        }
    }

    fn chr_read(&mut self, addr: u16) -> u8 {
        if self.chr.is_empty() {
            return 0;
        }
        let bank = self.map_chr_bank(addr);
        let offset = (addr as usize) & 0x03FF;
        self.chr[(bank * 0x0400 + offset) % self.chr.len()]
    }

    fn chr_write(&mut self, addr: u16, val: u8) {
        if self.chr.is_empty() {
            return;
        }
        let bank = self.map_chr_bank(addr);
        let offset = (addr as usize) & 0x03FF;
        let idx = (bank * 0x0400 + offset) % self.chr.len();
        self.chr[idx] = val;
    }

    fn mirroring(&self) -> Mirroring {
        self.mirroring
    }

    fn irq_pending(&self) -> bool {
        self.irq_pending
    }

    fn notify_scanline(&mut self) {
        if !self.irq_cycle_mode {
            self.clock_irq_counter();
        }
    }

    fn clock_cpu(&mut self) {
        if !self.irq_cycle_mode {
            return;
        }

        self.irq_cycle_divider = self.irq_cycle_divider.saturating_sub(1);
        if self.irq_cycle_divider == 0 {
            self.irq_cycle_divider = 4;
            self.clock_irq_counter();
        }
    }

    fn write_state(&self, w: &mut crate::save_state::StateWriter) {
        w.write_u8(crate::save_state::encode_mirroring(self.mirroring));
        w.write_bool(self.fixed_four_screen);

        w.write_u8(self.bank_select);
        w.write_bytes(&self.bank_registers);

        w.write_u8(self.irq_latch);
        w.write_u8(self.irq_counter);
        w.write_bool(self.irq_reload_pending);
        w.write_bool(self.irq_enabled);
        w.write_bool(self.irq_pending);
        w.write_bool(self.irq_cycle_mode);
        w.write_u8(self.irq_cycle_divider);

        crate::save_state::write_chr_state(w, &self.chr);
    }

    fn read_state(&mut self, r: &mut crate::save_state::StateReader) -> anyhow::Result<()> {
        self.mirroring = crate::save_state::decode_mirroring(r.read_u8()?)?;
        self.fixed_four_screen = r.read_bool()?;

        self.bank_select = r.read_u8()?;
        r.read_exact(&mut self.bank_registers)?;

        self.irq_latch = r.read_u8()?;
        self.irq_counter = r.read_u8()?;
        self.irq_reload_pending = r.read_bool()?;
        self.irq_enabled = r.read_bool()?;
        self.irq_pending = r.read_bool()?;
        self.irq_cycle_mode = r.read_bool()?;
        self.irq_cycle_divider = r.read_u8()?;

        crate::save_state::read_chr_state(r, &mut self.chr, "RAMBO-1")?;
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
    fn switches_extra_prg_register_and_mode() {
        let mut mapper = Rambo1::new(prg_banks(16), chr_banks(16), Mirroring::Vertical);

        mapper.cpu_write(0x8000, 0x06);
        mapper.cpu_write(0x8001, 0x03);
        mapper.cpu_write(0x8000, 0x07);
        mapper.cpu_write(0x8001, 0x04);
        mapper.cpu_write(0x8000, 0x0F);
        mapper.cpu_write(0x8001, 0x05);

        assert_eq!(mapper.cpu_peek(0x8000), 0x03);
        assert_eq!(mapper.cpu_peek(0xA000), 0x04);
        assert_eq!(mapper.cpu_peek(0xC000), 0x05);
        assert_eq!(mapper.cpu_peek(0xE000), 0x0F);

        mapper.cpu_write(0x8000, 0x46);
        assert_eq!(mapper.cpu_peek(0x8000), 0x05);
        assert_eq!(mapper.cpu_peek(0xC000), 0x03);
    }

    #[test]
    fn supports_full_1k_chr_mode_and_inversion() {
        let mut mapper = Rambo1::new(prg_banks(8), chr_banks(32), Mirroring::Vertical);

        mapper.cpu_write(0x8000, 0x20);
        mapper.cpu_write(0x8001, 0x02);
        mapper.cpu_write(0x8000, 0x28);
        mapper.cpu_write(0x8001, 0x0A);
        mapper.cpu_write(0x8000, 0x22);
        mapper.cpu_write(0x8001, 0x14);

        assert_eq!(mapper.chr_read(0x0000), 0x02);
        assert_eq!(mapper.chr_read(0x0400), 0x0A);
        assert_eq!(mapper.chr_read(0x1000), 0x14);

        mapper.cpu_write(0x8000, 0xA0);
        assert_eq!(mapper.chr_read(0x0000), 0x14);
        assert_eq!(mapper.chr_read(0x1400), 0x0A);
    }

    #[test]
    fn supports_scanline_and_cpu_cycle_irq_modes() {
        let mut mapper = Rambo1::new(prg_banks(8), chr_banks(8), Mirroring::Vertical);

        mapper.cpu_write(0xC000, 0x02);
        mapper.cpu_write(0xC001, 0x00);
        mapper.cpu_write(0xE001, 0x00);

        mapper.notify_scanline();
        mapper.notify_scanline();
        mapper.notify_scanline();
        mapper.notify_scanline();
        assert!(mapper.irq_pending());

        mapper.cpu_write(0xE000, 0x00);
        mapper.cpu_write(0xC000, 0x01);
        mapper.cpu_write(0xC001, 0x01);
        mapper.cpu_write(0xE001, 0x00);
        for _ in 0..8 {
            mapper.clock_cpu();
        }
        assert!(mapper.irq_pending());
    }
}
