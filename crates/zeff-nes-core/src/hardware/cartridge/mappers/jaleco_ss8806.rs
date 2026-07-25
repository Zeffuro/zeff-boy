use crate::hardware::cartridge::{Mapper, Mirroring};

pub struct JalecoSs8806 {
    prg_rom: Vec<u8>,
    chr: Vec<u8>,
    prg_ram: [u8; 0x2000],
    mirroring: Mirroring,

    prg_banks: [u8; 3],
    chr_banks: [u8; 8],

    prg_ram_enable: bool,
    prg_ram_writable: bool,

    irq_reload: u16,
    irq_counter: u16,
    irq_mask: u16,
    irq_enabled: bool,
    irq_pending: bool,
}

impl JalecoSs8806 {
    pub fn new(prg_rom: Vec<u8>, chr: Vec<u8>, mirroring: Mirroring) -> Self {
        Self {
            prg_rom,
            chr,
            prg_ram: [0; 0x2000],
            mirroring,
            prg_banks: [0, 1, 2],
            chr_banks: [0, 1, 2, 3, 4, 5, 6, 7],
            prg_ram_enable: true,
            prg_ram_writable: true,
            irq_reload: 0,
            irq_counter: 0,
            irq_mask: 0xFFFF,
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

    fn read_prg_bank(&self, bank: usize, addr: u16) -> u8 {
        let bank = bank % self.prg_bank_count_8k();
        let offset = (addr as usize) & 0x1FFF;
        self.prg_rom[(bank * 0x2000 + offset) % self.prg_rom.len()]
    }

    fn set_prg_nibble(&mut self, slot: usize, high: bool, val: u8) {
        let val = val & 0x0F;
        if high {
            self.prg_banks[slot] = (self.prg_banks[slot] & 0x0F) | ((val & 0x03) << 4);
        } else {
            self.prg_banks[slot] = (self.prg_banks[slot] & 0x30) | val;
        }
    }

    fn set_chr_nibble(&mut self, slot: usize, high: bool, val: u8) {
        let val = val & 0x0F;
        if high {
            self.chr_banks[slot] = (self.chr_banks[slot] & 0x0F) | (val << 4);
        } else {
            self.chr_banks[slot] = (self.chr_banks[slot] & 0xF0) | val;
        }
    }

    fn set_irq_reload_nibble(&mut self, nibble: u16, val: u8) {
        let shift = nibble * 4;
        self.irq_reload =
            (self.irq_reload & !(0x000F << shift)) | (((val as u16) & 0x000F) << shift);
    }

    fn set_irq_control(&mut self, val: u8) {
        self.irq_pending = false;
        self.irq_enabled = val & 0x01 != 0;
        self.irq_mask = if val & 0x08 != 0 {
            0x000F
        } else if val & 0x04 != 0 {
            0x00FF
        } else if val & 0x02 != 0 {
            0x0FFF
        } else {
            0xFFFF
        };
    }
}

impl Mapper for JalecoSs8806 {
    fn cpu_peek(&self, addr: u16) -> u8 {
        match addr {
            0x6000..=0x7FFF if self.prg_ram_enable => self.prg_ram[(addr - 0x6000) as usize],
            0x8000..=0x9FFF => self.read_prg_bank(self.prg_banks[0] as usize, addr),
            0xA000..=0xBFFF => self.read_prg_bank(self.prg_banks[1] as usize, addr),
            0xC000..=0xDFFF => self.read_prg_bank(self.prg_banks[2] as usize, addr),
            0xE000..=0xFFFF => self.read_prg_bank(self.prg_bank_count_8k() - 1, addr),
            _ => 0,
        }
    }

    fn cpu_write(&mut self, addr: u16, val: u8) {
        match addr {
            0x6000..=0x7FFF if self.prg_ram_enable && self.prg_ram_writable => {
                self.prg_ram[(addr - 0x6000) as usize] = val;
            }
            0x8000..=0xFFFF => match addr & 0xF003 {
                0x8000 => self.set_prg_nibble(0, false, val),
                0x8001 => self.set_prg_nibble(0, true, val),
                0x8002 => self.set_prg_nibble(1, false, val),
                0x8003 => self.set_prg_nibble(1, true, val),
                0x9000 => self.set_prg_nibble(2, false, val),
                0x9001 => self.set_prg_nibble(2, true, val),
                0x9002 => {
                    self.prg_ram_enable = val & 0x01 != 0;
                    self.prg_ram_writable = val & 0x02 != 0;
                }
                0xA000 => self.set_chr_nibble(0, false, val),
                0xA001 => self.set_chr_nibble(0, true, val),
                0xA002 => self.set_chr_nibble(1, false, val),
                0xA003 => self.set_chr_nibble(1, true, val),
                0xB000 => self.set_chr_nibble(2, false, val),
                0xB001 => self.set_chr_nibble(2, true, val),
                0xB002 => self.set_chr_nibble(3, false, val),
                0xB003 => self.set_chr_nibble(3, true, val),
                0xC000 => self.set_chr_nibble(4, false, val),
                0xC001 => self.set_chr_nibble(4, true, val),
                0xC002 => self.set_chr_nibble(5, false, val),
                0xC003 => self.set_chr_nibble(5, true, val),
                0xD000 => self.set_chr_nibble(6, false, val),
                0xD001 => self.set_chr_nibble(6, true, val),
                0xD002 => self.set_chr_nibble(7, false, val),
                0xD003 => self.set_chr_nibble(7, true, val),
                0xE000..=0xE003 => self.set_irq_reload_nibble(addr & 0x0003, val),
                0xF000 => {
                    self.irq_counter = self.irq_reload;
                    self.irq_pending = false;
                }
                0xF001 => self.set_irq_control(val),
                0xF002 => {
                    self.mirroring = match val & 0x03 {
                        0 => Mirroring::Horizontal,
                        1 => Mirroring::Vertical,
                        2 => Mirroring::SingleScreenLower,
                        3 => Mirroring::SingleScreenUpper,
                        _ => unreachable!(),
                    };
                }
                _ => {}
            },
            _ => {}
        }
    }

    fn chr_read(&mut self, addr: u16) -> u8 {
        if self.chr.is_empty() {
            return 0;
        }
        let slot = ((addr as usize) >> 10) & 0x07;
        let bank = (self.chr_banks[slot] as usize) % self.chr_bank_count_1k();
        let offset = (addr as usize) & 0x03FF;
        self.chr[(bank * 0x0400 + offset) % self.chr.len()]
    }

    fn chr_write(&mut self, addr: u16, val: u8) {
        if self.chr.is_empty() {
            return;
        }
        let slot = ((addr as usize) >> 10) & 0x07;
        let bank = (self.chr_banks[slot] as usize) % self.chr_bank_count_1k();
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

    fn clock_cpu(&mut self) {
        if !self.irq_enabled {
            return;
        }

        let low = self.irq_counter & self.irq_mask;
        let high = self.irq_counter & !self.irq_mask;
        if low == 0 {
            self.irq_counter = high | self.irq_mask;
            self.irq_pending = true;
        } else {
            self.irq_counter = high | (low - 1);
        }
    }

    fn write_state(&self, w: &mut crate::save_state::StateWriter) {
        w.write_bytes(&self.prg_ram);
        w.write_u8(crate::save_state::encode_mirroring(self.mirroring));

        w.write_bytes(&self.prg_banks);
        w.write_bytes(&self.chr_banks);

        w.write_bool(self.prg_ram_enable);
        w.write_bool(self.prg_ram_writable);

        w.write_u16(self.irq_reload);
        w.write_u16(self.irq_counter);
        w.write_u16(self.irq_mask);
        w.write_bool(self.irq_enabled);
        w.write_bool(self.irq_pending);

        crate::save_state::write_chr_state(w, &self.chr);
    }

    fn read_state(&mut self, r: &mut crate::save_state::StateReader) -> anyhow::Result<()> {
        r.read_exact(&mut self.prg_ram)?;
        self.mirroring = crate::save_state::decode_mirroring(r.read_u8()?)?;

        r.read_exact(&mut self.prg_banks)?;
        r.read_exact(&mut self.chr_banks)?;

        self.prg_ram_enable = r.read_bool()?;
        self.prg_ram_writable = r.read_bool()?;

        self.irq_reload = r.read_u16()?;
        self.irq_counter = r.read_u16()?;
        self.irq_mask = r.read_u16()?;
        self.irq_enabled = r.read_bool()?;
        self.irq_pending = r.read_bool()?;

        crate::save_state::read_chr_state(r, &mut self.chr, "Jaleco SS8806")?;
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
        let mut mapper = JalecoSs8806::new(prg_banks(64), chr_banks(256), Mirroring::Vertical);

        mapper.cpu_write(0x8000, 0x03);
        mapper.cpu_write(0x8001, 0x02);
        mapper.cpu_write(0x8002, 0x04);
        mapper.cpu_write(0x8003, 0x01);
        mapper.cpu_write(0x9000, 0x05);
        mapper.cpu_write(0x9001, 0x00);
        mapper.cpu_write(0xA002, 0x0A);
        mapper.cpu_write(0xA003, 0x01);
        mapper.cpu_write(0xF002, 0x03);

        assert_eq!(mapper.cpu_peek(0x8000), 0x23);
        assert_eq!(mapper.cpu_peek(0xA000), 0x14);
        assert_eq!(mapper.cpu_peek(0xC000), 0x05);
        assert_eq!(mapper.cpu_peek(0xE000), 0x3F);
        assert_eq!(mapper.chr_read(0x0400), 0x1A);
        assert_eq!(mapper.mirroring(), Mirroring::SingleScreenUpper);
    }

    #[test]
    fn prg_ram_protect_controls_writes() {
        let mut mapper = JalecoSs8806::new(prg_banks(4), chr_banks(8), Mirroring::Horizontal);

        mapper.cpu_write(0x6000, 0x11);
        assert_eq!(mapper.cpu_peek(0x6000), 0x11);

        mapper.cpu_write(0x9002, 0x01);
        mapper.cpu_write(0x6000, 0x22);
        assert_eq!(mapper.cpu_peek(0x6000), 0x11);

        mapper.cpu_write(0x9002, 0x03);
        mapper.cpu_write(0x6000, 0x33);
        assert_eq!(mapper.cpu_peek(0x6000), 0x33);

        mapper.cpu_write(0x9002, 0x00);
        assert_eq!(mapper.cpu_peek(0x6000), 0x00);
    }

    #[test]
    fn irq_counts_down_with_selected_width() {
        let mut mapper = JalecoSs8806::new(prg_banks(4), chr_banks(8), Mirroring::Horizontal);

        mapper.cpu_write(0xE000, 0x02);
        mapper.cpu_write(0xE001, 0x03);
        mapper.cpu_write(0xE002, 0x04);
        mapper.cpu_write(0xE003, 0x05);
        mapper.cpu_write(0xF000, 0x00);
        mapper.cpu_write(0xF001, 0x09);

        for _ in 0..2 {
            mapper.clock_cpu();
        }
        assert!(!mapper.irq_pending());
        mapper.clock_cpu();
        assert!(mapper.irq_pending());
        assert_eq!(mapper.irq_counter, 0x543F);
    }
}
