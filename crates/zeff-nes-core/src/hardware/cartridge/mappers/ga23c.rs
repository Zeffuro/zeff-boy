use crate::hardware::cartridge::{Mapper, Mirroring};

pub struct Ga23c {
    prg_rom: Vec<u8>,
    chr: Vec<u8>,
    chr_ram: bool,
    prg_ram: [u8; 0x2000],
    mirroring: Mirroring,
    fixed_four_screen: bool,

    bank_select: u8,
    bank_registers: [u8; 8],
    prg_ram_enable: bool,
    prg_ram_write_protect: bool,

    outer_regs: [u8; 4],
    outer_index: u8,
    outer_locked: bool,

    irq_latch: u8,
    irq_counter: u8,
    irq_reload: bool,
    irq_enabled: bool,
    irq_pending: bool,
}

impl Ga23c {
    pub fn new(prg_rom: Vec<u8>, chr: Vec<u8>, mirroring: Mirroring) -> Self {
        Self::with_chr_ram(prg_rom, chr, mirroring, false)
    }

    pub fn with_chr_ram(
        prg_rom: Vec<u8>,
        chr: Vec<u8>,
        mirroring: Mirroring,
        chr_ram: bool,
    ) -> Self {
        let chr_ram = chr_ram || chr.is_empty();
        let chr = if chr_ram { vec![0; 0x2000] } else { chr };
        Self {
            prg_rom,
            chr,
            chr_ram,
            prg_ram: [0; 0x2000],
            mirroring,
            fixed_four_screen: matches!(mirroring, Mirroring::FourScreen),
            bank_select: 0,
            bank_registers: [0; 8],
            prg_ram_enable: true,
            prg_ram_write_protect: false,
            outer_regs: [0; 4],
            outer_index: 0,
            outer_locked: false,
            irq_latch: 0,
            irq_counter: 0,
            irq_reload: false,
            irq_enabled: false,
            irq_pending: false,
        }
    }

    fn prg_bank_count_8k(&self) -> usize {
        (self.prg_rom.len() / 0x2000).max(1)
    }

    fn chr_bank_count_1k(&self) -> usize {
        (self.chr.len() / 0x0400).max(1)
    }

    fn inner_prg_bank(&self, addr: u16) -> usize {
        let bank_count = self.prg_bank_count_8k();
        let last = bank_count - 1;
        let second_last = bank_count.saturating_sub(2);
        let prg_mode = (self.bank_select >> 6) & 1;

        match addr {
            0x8000..=0x9FFF => {
                if prg_mode == 0 {
                    self.bank_registers[6] as usize
                } else {
                    second_last
                }
            }
            0xA000..=0xBFFF => self.bank_registers[7] as usize,
            0xC000..=0xDFFF => {
                if prg_mode == 0 {
                    second_last
                } else {
                    self.bank_registers[6] as usize
                }
            }
            0xE000..=0xFFFF => last,
            _ => 0,
        }
    }

    fn map_prg_bank(&self, addr: u16) -> usize {
        let bank_count = self.prg_bank_count_8k();
        let inner = self.inner_prg_bank(addr);
        let prg_and = ((self.outer_regs[3] & 0x3F) ^ 0x3F) as usize;
        let prg_or = self.outer_regs[1] as usize | (((self.outer_regs[2] as usize) & 0xC0) << 2);

        ((inner & prg_and) | prg_or) % bank_count
    }

    fn inner_chr_bank(&self, addr: u16) -> usize {
        let chr_mode = (self.bank_select >> 7) & 1;

        let bank_0 = (self.bank_registers[0] & !1) as usize;
        let bank_1 = (self.bank_registers[1] & !1) as usize;
        let bank_2 = self.bank_registers[2] as usize;
        let bank_3 = self.bank_registers[3] as usize;
        let bank_4 = self.bank_registers[4] as usize;
        let bank_5 = self.bank_registers[5] as usize;

        match (chr_mode, addr) {
            (0, 0x0000..=0x03FF) => bank_0,
            (0, 0x0400..=0x07FF) => bank_0 + 1,
            (0, 0x0800..=0x0BFF) => bank_1,
            (0, 0x0C00..=0x0FFF) => bank_1 + 1,
            (0, 0x1000..=0x13FF) => bank_2,
            (0, 0x1400..=0x17FF) => bank_3,
            (0, 0x1800..=0x1BFF) => bank_4,
            (0, 0x1C00..=0x1FFF) => bank_5,

            (1, 0x0000..=0x03FF) => bank_2,
            (1, 0x0400..=0x07FF) => bank_3,
            (1, 0x0800..=0x0BFF) => bank_4,
            (1, 0x0C00..=0x0FFF) => bank_5,
            (1, 0x1000..=0x13FF) => bank_0,
            (1, 0x1400..=0x17FF) => bank_0 + 1,
            (1, 0x1800..=0x1BFF) => bank_1,
            (1, 0x1C00..=0x1FFF) => bank_1 + 1,
            _ => 0,
        }
    }

    fn chr_and_mask(&self) -> usize {
        let bits = self.outer_regs[2] & 0x0F;
        if bits >= 8 {
            (1usize << (bits - 7)) - 1
        } else {
            0
        }
    }

    fn map_chr_bank(&self, addr: u16) -> usize {
        let bank_count = self.chr_bank_count_1k();
        let inner = self.inner_chr_bank(addr);
        let chr_or = self.outer_regs[0] as usize | (((self.outer_regs[2] as usize) & 0xF0) << 4);
        ((inner & self.chr_and_mask()) | chr_or) % bank_count
    }

    fn reset_outer_regs(&mut self) {
        self.outer_regs = [0; 4];
        self.outer_index = 0;
        self.outer_locked = false;
    }

    fn outer_write(&mut self, val: u8) {
        if !self.outer_locked {
            self.outer_regs[self.outer_index as usize] = val;
            if self.outer_index == 3 && val & 0x80 != 0 {
                self.outer_locked = true;
            }
        }
        self.outer_index = (self.outer_index + 1) & 0x03;
    }
}

impl Mapper for Ga23c {
    fn cpu_peek(&self, addr: u16) -> u8 {
        match addr {
            0x5000..=0x5FFF => {
                // GA23C menu selection #1: emulate DIP switch position 0.
                // The selected address line appears on D0.
                u8::from(addr & 0x0010 != 0)
            }
            0x6000..=0x7FFF if self.prg_ram_enable => self.prg_ram[(addr - 0x6000) as usize],
            0x8000..=0xFFFF => {
                let bank = self.map_prg_bank(addr);
                let offset = (addr as usize) & 0x1FFF;
                self.prg_rom[(bank * 0x2000 + offset) % self.prg_rom.len()]
            }
            _ => 0,
        }
    }

    fn cpu_write(&mut self, addr: u16, val: u8) {
        match addr {
            0x6000..=0x7FFF if addr & 0xF001 == 0x6000 => {
                self.outer_write(val);
                if self.prg_ram_enable && !self.prg_ram_write_protect {
                    self.prg_ram[(addr - 0x6000) as usize] = val;
                }
            }
            0x6000..=0x7FFF if addr & 0xF001 == 0x6001 => self.reset_outer_regs(),
            0x6000..=0x7FFF if self.prg_ram_enable && !self.prg_ram_write_protect => {
                self.prg_ram[(addr - 0x6000) as usize] = val;
            }
            0x8000..=0x9FFF => {
                if addr & 1 == 0 {
                    self.bank_select = val;
                } else {
                    let register = (self.bank_select & 0x07) as usize;
                    self.bank_registers[register] = val;
                }
            }
            0xA000..=0xBFFF => {
                if addr & 1 == 0 {
                    if !self.fixed_four_screen {
                        self.mirroring = if val & 1 == 0 {
                            Mirroring::Vertical
                        } else {
                            Mirroring::Horizontal
                        };
                    }
                } else {
                    self.prg_ram_enable = val & 0x80 != 0;
                    self.prg_ram_write_protect = val & 0x40 != 0;
                }
            }
            0xC000..=0xDFFF => {
                if addr & 1 == 0 {
                    self.irq_latch = val;
                } else {
                    self.irq_reload = true;
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
        let bank = self.map_chr_bank(addr);
        let offset = (addr as usize) & 0x03FF;
        self.chr[(bank * 0x0400 + offset) % self.chr.len()]
    }

    fn chr_write(&mut self, addr: u16, val: u8) {
        if !self.chr_ram {
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
        let old = self.irq_counter;

        if self.irq_counter == 0 || self.irq_reload {
            self.irq_counter = self.irq_latch;
        } else {
            self.irq_counter -= 1;
        }

        if self.irq_counter == 0 && self.irq_enabled && (old != 0 || self.irq_reload) {
            self.irq_pending = true;
        }

        self.irq_reload = false;
    }

    fn write_state(&self, w: &mut crate::save_state::StateWriter) {
        w.write_bytes(&self.prg_ram);
        w.write_u8(crate::save_state::encode_mirroring(self.mirroring));
        w.write_bool(self.fixed_four_screen);
        w.write_bool(self.chr_ram);

        w.write_u8(self.bank_select);
        w.write_bytes(&self.bank_registers);
        w.write_bool(self.prg_ram_enable);
        w.write_bool(self.prg_ram_write_protect);

        w.write_bytes(&self.outer_regs);
        w.write_u8(self.outer_index);
        w.write_bool(self.outer_locked);

        w.write_u8(self.irq_latch);
        w.write_u8(self.irq_counter);
        w.write_bool(self.irq_reload);
        w.write_bool(self.irq_enabled);
        w.write_bool(self.irq_pending);

        crate::save_state::write_chr_state(w, &self.chr);
    }

    fn read_state(&mut self, r: &mut crate::save_state::StateReader) -> anyhow::Result<()> {
        r.read_exact(&mut self.prg_ram)?;
        self.mirroring = crate::save_state::decode_mirroring(r.read_u8()?)?;
        self.fixed_four_screen = r.read_bool()?;
        self.chr_ram = r.read_bool()?;

        self.bank_select = r.read_u8()?;
        r.read_exact(&mut self.bank_registers)?;
        self.prg_ram_enable = r.read_bool()?;
        self.prg_ram_write_protect = r.read_bool()?;

        r.read_exact(&mut self.outer_regs)?;
        self.outer_index = r.read_u8()? & 0x03;
        self.outer_locked = r.read_bool()?;

        self.irq_latch = r.read_u8()?;
        self.irq_counter = r.read_u8()?;
        self.irq_reload = r.read_bool()?;
        self.irq_enabled = r.read_bool()?;
        self.irq_pending = r.read_bool()?;

        crate::save_state::read_chr_state(r, &mut self.chr, "GA23C")?;
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
    fn defaults_to_mmc3_prg_layout() {
        let mut mapper = Ga23c::new(prg_banks(128), chr_banks(256), Mirroring::Vertical);

        mapper.cpu_write(0x8000, 0x06);
        mapper.cpu_write(0x8001, 0x03);
        mapper.cpu_write(0x8000, 0x07);
        mapper.cpu_write(0x8001, 0x04);

        assert_eq!(mapper.cpu_peek(0x8000), 0x03);
        assert_eq!(mapper.cpu_peek(0xA000), 0x04);
        assert_eq!(mapper.cpu_peek(0xC000), 0x3E);
        assert_eq!(mapper.cpu_peek(0xE000), 0x3F);
    }

    #[test]
    fn outer_prg_or_and_mask_apply_to_mmc3_bank_registers() {
        let mut mapper = Ga23c::new(prg_banks(128), chr_banks(256), Mirroring::Vertical);

        mapper.cpu_write(0x6000, 0x00);
        mapper.cpu_write(0x6000, 0x40);
        mapper.cpu_write(0x6000, 0x00);
        mapper.cpu_write(0x6000, 0x20);

        mapper.cpu_write(0x8000, 0x06);
        mapper.cpu_write(0x8001, 0x03);

        assert_eq!(mapper.cpu_peek(0x8000), 0x43);
    }

    #[test]
    fn reset_write_clears_outer_regs_and_lock() {
        let mut mapper = Ga23c::new(prg_banks(128), chr_banks(256), Mirroring::Vertical);

        mapper.cpu_write(0x6000, 0x00);
        mapper.cpu_write(0x6000, 0x40);
        mapper.cpu_write(0x6000, 0x00);
        mapper.cpu_write(0x6000, 0xA0);
        mapper.cpu_write(0x6000, 0xFF);
        mapper.cpu_write(0x6001, 0x00);
        mapper.cpu_write(0x6000, 0x00);
        mapper.cpu_write(0x6000, 0x20);

        mapper.cpu_write(0x8000, 0x06);
        mapper.cpu_write(0x8001, 0x03);

        assert_eq!(mapper.cpu_peek(0x8000), 0x23);
    }

    #[test]
    fn outer_chr_mask_and_or_apply_to_mmc3_chr_bank_registers() {
        let mut mapper = Ga23c::new(prg_banks(128), chr_banks(256), Mirroring::Vertical);

        mapper.cpu_write(0x6000, 0x20);
        mapper.cpu_write(0x6000, 0x00);
        mapper.cpu_write(0x6000, 0x0F);
        mapper.cpu_write(0x6000, 0x00);

        mapper.cpu_write(0x8000, 0x02);
        mapper.cpu_write(0x8001, 0x05);

        assert_eq!(mapper.chr_read(0x1000), 0x25);
    }

    #[test]
    fn allocates_chr_ram_when_rom_has_no_chr() {
        let mut mapper = Ga23c::new(prg_banks(128), Vec::new(), Mirroring::Vertical);

        mapper.chr_write(0x0123, 0xA5);

        assert_eq!(mapper.chr_read(0x0123), 0xA5);
    }

    #[test]
    fn exposes_default_menu_selection_dip_zero() {
        let mapper = Ga23c::new(prg_banks(128), chr_banks(256), Mirroring::Vertical);

        assert_eq!(mapper.cpu_peek(0x5000) & 1, 0);
        assert_eq!(mapper.cpu_peek(0x5010) & 1, 1);
        assert_eq!(mapper.cpu_peek(0x5020) & 1, 0);
    }
}
