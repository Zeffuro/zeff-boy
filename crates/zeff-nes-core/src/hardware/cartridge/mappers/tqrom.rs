use crate::hardware::cartridge::{Mapper, Mirroring};

pub struct Tqrom {
    prg_rom: Vec<u8>,
    chr_rom: Vec<u8>,
    chr_ram: [u8; 0x2000],
    prg_ram: [u8; 0x2000],
    mirroring: Mirroring,
    fixed_four_screen: bool,

    bank_select: u8,
    bank_registers: [u8; 8],
    prg_ram_enable: bool,
    prg_ram_write_protect: bool,

    irq_latch: u8,
    irq_counter: u8,
    irq_reload: bool,
    irq_enabled: bool,
    irq_pending: bool,
}

impl Tqrom {
    pub fn new(prg_rom: Vec<u8>, chr_rom: Vec<u8>, mirroring: Mirroring) -> Self {
        Self {
            prg_rom,
            chr_rom,
            chr_ram: [0; 0x2000],
            prg_ram: [0; 0x2000],
            mirroring,
            fixed_four_screen: matches!(mirroring, Mirroring::FourScreen),
            bank_select: 0,
            bank_registers: [0; 8],
            prg_ram_enable: true,
            prg_ram_write_protect: false,
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

    fn chr_rom_bank_count_1k(&self) -> usize {
        (self.chr_rom.len() / 0x0400).max(1)
    }

    fn map_prg_bank(&self, addr: u16) -> usize {
        let bank_count = self.prg_bank_count_8k();
        let last = bank_count - 1;
        let second_last = bank_count.saturating_sub(2);
        let prg_mode = (self.bank_select >> 6) & 1;

        (match addr {
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
        }) % bank_count
    }

    fn map_chr_source(&self, addr: u16) -> (u8, bool, usize) {
        let chr_mode = (self.bank_select >> 7) & 1;

        let bank_0 = self.bank_registers[0] & !1;
        let bank_1 = self.bank_registers[1] & !1;
        let bank_2 = self.bank_registers[2];
        let bank_3 = self.bank_registers[3];
        let bank_4 = self.bank_registers[4];
        let bank_5 = self.bank_registers[5];

        let bank = match (chr_mode, addr) {
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
        };

        let use_ram = bank & 0x40 != 0;
        let bank = (bank & 0x3F) as usize;
        let offset = (addr as usize) & 0x03FF;
        (bank as u8, use_ram, offset)
    }

    fn clock_irq_counter(&mut self) {
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
}

impl Mapper for Tqrom {
    fn cpu_peek(&self, addr: u16) -> u8 {
        match addr {
            0x6000..=0x7FFF if self.prg_ram_enable => self.prg_ram[(addr - 0x6000) as usize],
            0x8000..=0xFFFF => {
                let bank = self.map_prg_bank(addr);
                let offset = (addr as usize) & 0x1FFF;
                self.prg_rom[bank * 0x2000 + offset]
            }
            _ => 0,
        }
    }

    fn cpu_write(&mut self, addr: u16, val: u8) {
        match addr {
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
        let (bank, use_ram, offset) = self.map_chr_source(addr);
        if use_ram {
            let bank = (bank as usize) % 8;
            self.chr_ram[bank * 0x0400 + offset]
        } else {
            let bank = (bank as usize) % self.chr_rom_bank_count_1k();
            self.chr_rom[(bank * 0x0400 + offset) % self.chr_rom.len()]
        }
    }

    fn chr_write(&mut self, addr: u16, val: u8) {
        let (bank, use_ram, offset) = self.map_chr_source(addr);
        if !use_ram {
            return;
        }
        let bank = (bank as usize) % 8;
        self.chr_ram[bank * 0x0400 + offset] = val;
    }

    fn mirroring(&self) -> Mirroring {
        self.mirroring
    }

    fn irq_pending(&self) -> bool {
        self.irq_pending
    }

    fn notify_scanline(&mut self) {
        self.clock_irq_counter();
    }

    fn write_state(&self, w: &mut crate::save_state::StateWriter) {
        w.write_bytes(&self.prg_ram);
        w.write_u8(crate::save_state::encode_mirroring(self.mirroring));
        w.write_bool(self.fixed_four_screen);

        w.write_u8(self.bank_select);
        w.write_bytes(&self.bank_registers);
        w.write_bool(self.prg_ram_enable);
        w.write_bool(self.prg_ram_write_protect);

        w.write_u8(self.irq_latch);
        w.write_u8(self.irq_counter);
        w.write_bool(self.irq_reload);
        w.write_bool(self.irq_enabled);
        w.write_bool(self.irq_pending);

        w.write_bytes(&self.chr_ram);
    }

    fn read_state(&mut self, r: &mut crate::save_state::StateReader) -> anyhow::Result<()> {
        r.read_exact(&mut self.prg_ram)?;
        self.mirroring = crate::save_state::decode_mirroring(r.read_u8()?)?;
        self.fixed_four_screen = r.read_bool()?;

        self.bank_select = r.read_u8()?;
        r.read_exact(&mut self.bank_registers)?;
        self.prg_ram_enable = r.read_bool()?;
        self.prg_ram_write_protect = r.read_bool()?;

        self.irq_latch = r.read_u8()?;
        self.irq_counter = r.read_u8()?;
        self.irq_reload = r.read_bool()?;
        self.irq_enabled = r.read_bool()?;
        self.irq_pending = r.read_bool()?;

        r.read_exact(&mut self.chr_ram)?;
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
    fn uses_chr_rom_when_bit6_clear_and_chr_ram_when_set() {
        let mut mapper = Tqrom::new(prg_banks(16), chr_banks(64), Mirroring::Horizontal);

        mapper.cpu_write(0x8000, 0x02);
        mapper.cpu_write(0x8001, 0x05);
        assert_eq!(mapper.chr_read(0x1000), 0x05);

        mapper.cpu_write(0x8001, 0x45);
        mapper.chr_write(0x1000, 0xA5);
        assert_eq!(mapper.chr_read(0x1000), 0xA5);

        mapper.cpu_write(0x8001, 0x05);
        assert_eq!(mapper.chr_read(0x1000), 0x05);
    }

    #[test]
    fn preserves_mmc3_prg_and_irq_behavior() {
        let mut mapper = Tqrom::new(prg_banks(16), chr_banks(64), Mirroring::Horizontal);

        mapper.cpu_write(0x8000, 0x06);
        mapper.cpu_write(0x8001, 0x03);
        mapper.cpu_write(0x8000, 0x07);
        mapper.cpu_write(0x8001, 0x04);

        assert_eq!(mapper.cpu_peek(0x8000), 0x03);
        assert_eq!(mapper.cpu_peek(0xA000), 0x04);
        assert_eq!(mapper.cpu_peek(0xC000), 0x0E);
        assert_eq!(mapper.cpu_peek(0xE000), 0x0F);

        mapper.cpu_write(0xC000, 0x02);
        mapper.cpu_write(0xC001, 0x00);
        mapper.cpu_write(0xE001, 0x00);
        mapper.notify_scanline();
        mapper.notify_scanline();
        mapper.notify_scanline();
        assert!(mapper.irq_pending());
    }
}
