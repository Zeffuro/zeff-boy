use crate::hardware::cartridge::{Mapper, Mirroring};

pub struct Mmc3 {
    prg_rom: Vec<u8>,
    chr: Vec<u8>,
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

impl Mmc3 {
    pub fn new(prg_rom: Vec<u8>, chr: Vec<u8>, mirroring: Mirroring) -> Self {
        Self {
            prg_rom,
            chr,
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

    fn chr_bank_count_1k(&self) -> usize {
        (self.chr.len() / 0x0400).max(1)
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

    fn map_chr_bank(&self, addr: u16) -> usize {
        let chr_mode = (self.bank_select >> 7) & 1;
        let bank_count = self.chr_bank_count_1k();

        let bank_0 = (self.bank_registers[0] & !1) as usize;
        let bank_1 = (self.bank_registers[1] & !1) as usize;
        let bank_2 = self.bank_registers[2] as usize;
        let bank_3 = self.bank_registers[3] as usize;
        let bank_4 = self.bank_registers[4] as usize;
        let bank_5 = self.bank_registers[5] as usize;

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

        bank % bank_count
    }
}

impl Mapper for Mmc3 {
    fn cpu_peek(&self, addr: u16) -> u8 {
        match addr {
            0x6000..=0x7FFF => {
                if self.prg_ram_enable {
                    self.prg_ram[(addr - 0x6000) as usize]
                } else {
                    0
                }
            }
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
            0x6000..=0x7FFF => {
                if self.prg_ram_enable && !self.prg_ram_write_protect {
                    self.prg_ram[(addr - 0x6000) as usize] = val;
                }
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

        w.write_u8(self.bank_select);
        w.write_bytes(&self.bank_registers);
        w.write_bool(self.prg_ram_enable);
        w.write_bool(self.prg_ram_write_protect);

        w.write_u8(self.irq_latch);
        w.write_u8(self.irq_counter);
        w.write_bool(self.irq_reload);
        w.write_bool(self.irq_enabled);
        w.write_bool(self.irq_pending);

        w.write_vec(&self.chr);
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

        let chr = r.read_vec(1024 * 1024)?;
        if chr.len() != self.chr.len() {
            anyhow::bail!(
                "MMC3 CHR size mismatch: expected {}, got {}",
                self.chr.len(),
                chr.len()
            );
        }
        self.chr = chr;
        Ok(())
    }
}

#[cfg(test)]
mod tests;
