use crate::hardware::cartridge::{Mapper, Mirroring};

pub struct ConyYoko {
    prg_rom: Vec<u8>,
    chr: Vec<u8>,
    mirroring: Mirroring,
    prg_base: u8,
    mode: u8,
    prg_banks: [u8; 4],
    chr_banks: [u8; 8],
    scratch: [u8; 4],
    solder_pad: u8,
    irq_counter: u16,
    irq_enable_on_reload: bool,
    irq_enabled: bool,
    irq_decrement: bool,
    irq_source_pa12: bool,
    irq_pending: bool,
}

impl ConyYoko {
    pub fn new(prg_rom: Vec<u8>, chr: Vec<u8>, mirroring: Mirroring) -> Self {
        Self {
            prg_rom,
            chr,
            mirroring,
            prg_base: 0,
            mode: 0,
            prg_banks: [0, 1, 2, 3],
            chr_banks: [0, 1, 2, 3, 4, 5, 6, 7],
            scratch: [0; 4],
            solder_pad: 0,
            irq_counter: 0,
            irq_enable_on_reload: false,
            irq_enabled: false,
            irq_decrement: false,
            irq_source_pa12: false,
            irq_pending: false,
        }
    }

    fn prg_bank_count_8k(&self) -> usize {
        (self.prg_rom.len() / 0x2000).max(1)
    }

    fn large_chr_variant(&self) -> bool {
        self.chr.len() > 256 * 1024
    }

    fn prg_outer_base(&self) -> usize {
        let count = self.prg_bank_count_8k();
        if count > 32 {
            usize::from(self.prg_base & !0x1F)
        } else {
            0
        }
    }

    fn prg_bank_mode(&self) -> u8 {
        (self.mode >> 3) & 0x03
    }

    fn map_prg_bank(&self, addr: u16) -> usize {
        let count = self.prg_bank_count_8k();
        let outer = self.prg_outer_base();
        let slot = usize::from((addr - 0x8000) / 0x2000);
        let bank = match self.prg_bank_mode() {
            0 => {
                if addr < 0xC000 {
                    (usize::from(self.prg_base) & 0x1E) + usize::from(addr >= 0xA000)
                } else {
                    0x1E + usize::from(addr >= 0xE000)
                }
            }
            1 => (usize::from(self.prg_base) & 0x1C) + slot,
            _ => {
                if slot == 3 {
                    0x1F
                } else {
                    usize::from(self.prg_banks[slot] & 0x1F)
                }
            }
        };
        (outer + bank) % count
    }

    fn map_chr_bank_1k(&self, addr: u16) -> usize {
        let count = (self.chr.len() / 0x0400).max(1);
        usize::from(self.chr_banks[usize::from(addr / 0x0400)]) % count
    }

    fn map_chr_bank_2k(&self, addr: u16) -> usize {
        let count = (self.chr.len() / 0x0800).max(1);
        let reg = match addr {
            0x0000..=0x07FF => 0,
            0x0800..=0x0FFF => 1,
            0x1000..=0x17FF => 6,
            0x1800..=0x1FFF => 7,
            _ => 0,
        };
        usize::from(self.chr_banks[reg]) % count
    }

    fn clock_irq(&mut self) {
        if !self.irq_enabled || self.irq_counter == 0 || self.irq_pending {
            return;
        }
        if self.irq_decrement {
            self.irq_counter = self.irq_counter.wrapping_sub(1);
        } else {
            self.irq_counter = self.irq_counter.wrapping_add(1);
        }
        if self.irq_counter == 0 {
            self.irq_enabled = false;
            self.irq_pending = true;
        }
    }
}

impl Mapper for ConyYoko {
    fn cpu_peek(&self, addr: u16) -> u8 {
        match addr {
            0x5000..=0x50FF if (addr & 0xF100) == 0x5000 => self.solder_pad & 0x03,
            0x5100..=0x5103 => self.scratch[usize::from(addr & 0x0003)],
            0x6000..=0x7FFF if self.mode & 0x20 != 0 => {
                let bank = usize::from(self.prg_banks[3] & 0x1F) % self.prg_bank_count_8k();
                let offset = addr as usize & 0x1FFF;
                self.prg_rom[(bank * 0x2000 + offset) % self.prg_rom.len()]
            }
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
            0x5100..=0x5103 => self.scratch[usize::from(addr & 0x0003)] = val,
            _ if (addr & 0x8300) == 0x8000 => self.prg_base = val,
            _ if (addr & 0x8300) == 0x8100 => {
                self.mode = val;
                self.irq_enable_on_reload = val & 0x80 != 0;
                self.irq_decrement = val & 0x40 != 0;
                self.mirroring = match val & 0x03 {
                    0 => Mirroring::Vertical,
                    1 => Mirroring::Horizontal,
                    2 => Mirroring::SingleScreenLower,
                    _ => Mirroring::SingleScreenUpper,
                };
            }
            _ if (addr & 0x8301) == 0x8200 => {
                self.irq_counter = (self.irq_counter & 0xFF00) | u16::from(val);
                self.irq_pending = false;
            }
            _ if (addr & 0x8301) == 0x8201 => {
                self.irq_counter = (self.irq_counter & 0x00FF) | (u16::from(val) << 8);
                if self.irq_enable_on_reload {
                    self.irq_enabled = true;
                }
            }
            _ if (addr & 0x8310) == 0x8300 => {
                self.prg_banks[usize::from(addr & 0x0003)] = val & 0x1F;
            }
            _ if (addr & 0x8318) == 0x8310 => {
                self.chr_banks[usize::from(addr & 0x0007)] = val;
            }
            _ if (addr & 0x8318) == 0x8318 => self.irq_source_pa12 = val != 0,
            _ => {}
        }
    }

    fn chr_read(&mut self, addr: u16) -> u8 {
        if self.chr.is_empty() {
            return 0;
        }
        if self.large_chr_variant() {
            let bank = self.map_chr_bank_2k(addr);
            let offset = addr as usize & 0x07FF;
            self.chr[(bank * 0x0800 + offset) % self.chr.len()]
        } else {
            let bank = self.map_chr_bank_1k(addr);
            let offset = addr as usize & 0x03FF;
            self.chr[(bank * 0x0400 + offset) % self.chr.len()]
        }
    }

    fn chr_write(&mut self, _addr: u16, _val: u8) {}

    fn mirroring(&self) -> Mirroring {
        self.mirroring
    }

    fn irq_pending(&self) -> bool {
        self.irq_pending
    }

    fn notify_scanline(&mut self) {
        if self.irq_source_pa12 {
            for _ in 0..8 {
                self.clock_irq();
            }
        }
    }

    fn clock_cpu(&mut self) {
        if !self.irq_source_pa12 {
            self.clock_irq();
        }
    }

    fn write_state(&self, w: &mut crate::save_state::StateWriter) {
        w.write_u8(crate::save_state::encode_mirroring(self.mirroring));
        w.write_u8(self.prg_base);
        w.write_u8(self.mode);
        w.write_bytes(&self.prg_banks);
        w.write_bytes(&self.chr_banks);
        w.write_bytes(&self.scratch);
        w.write_u8(self.solder_pad);
        w.write_u16(self.irq_counter);
        w.write_bool(self.irq_enable_on_reload);
        w.write_bool(self.irq_enabled);
        w.write_bool(self.irq_decrement);
        w.write_bool(self.irq_source_pa12);
        w.write_bool(self.irq_pending);
        crate::save_state::write_chr_state(w, &self.chr);
    }

    fn read_state(&mut self, r: &mut crate::save_state::StateReader) -> anyhow::Result<()> {
        self.mirroring = crate::save_state::decode_mirroring(r.read_u8()?)?;
        self.prg_base = r.read_u8()?;
        self.mode = r.read_u8()?;
        r.read_exact(&mut self.prg_banks)?;
        r.read_exact(&mut self.chr_banks)?;
        r.read_exact(&mut self.scratch)?;
        self.solder_pad = r.read_u8()? & 0x03;
        self.irq_counter = r.read_u16()?;
        self.irq_enable_on_reload = r.read_bool()?;
        self.irq_enabled = r.read_bool()?;
        self.irq_decrement = r.read_bool()?;
        self.irq_source_pa12 = r.read_bool()?;
        self.irq_pending = r.read_bool()?;
        crate::save_state::read_chr_state(r, &mut self.chr, "Cony/Yoko")?;
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

    fn chr_1k_banks(count: usize) -> Vec<u8> {
        let mut chr = Vec::new();
        for bank in 0..count {
            chr.extend(vec![bank as u8; 0x0400]);
        }
        chr
    }

    #[test]
    fn switches_prg_register_mode_and_mirroring() {
        let mut mapper = ConyYoko::new(prg_banks(32), chr_1k_banks(256), Mirroring::Vertical);

        mapper.cpu_write(0x8100, 0x10 | 0x01);
        mapper.cpu_write(0x8300, 3);
        mapper.cpu_write(0x8301, 4);
        mapper.cpu_write(0x8302, 5);

        assert_eq!(mapper.cpu_peek(0x8000), 3);
        assert_eq!(mapper.cpu_peek(0xA000), 4);
        assert_eq!(mapper.cpu_peek(0xC000), 5);
        assert_eq!(mapper.cpu_peek(0xE000), 31);
        assert_eq!(mapper.mirroring(), Mirroring::Horizontal);
    }

    #[test]
    fn large_chr_variant_uses_four_2k_registers() {
        let mut mapper = ConyYoko::new(prg_banks(32), chr_1k_banks(512), Mirroring::Vertical);

        mapper.cpu_write(0x8310, 2);
        mapper.cpu_write(0x8311, 3);
        mapper.cpu_write(0x8316, 4);
        mapper.cpu_write(0x8317, 5);

        assert_eq!(mapper.chr_read(0x0000), 4);
        assert_eq!(mapper.chr_read(0x0800), 6);
        assert_eq!(mapper.chr_read(0x1000), 8);
        assert_eq!(mapper.chr_read(0x1800), 10);
    }

    #[test]
    fn irq_counts_to_zero_and_raises() {
        let mut mapper = ConyYoko::new(prg_banks(32), chr_1k_banks(256), Mirroring::Vertical);

        mapper.cpu_write(0x8100, 0x80 | 0x40);
        mapper.cpu_write(0x8200, 1);
        mapper.cpu_write(0x8201, 0);
        mapper.clock_cpu();
        assert!(mapper.irq_pending());
    }
}
