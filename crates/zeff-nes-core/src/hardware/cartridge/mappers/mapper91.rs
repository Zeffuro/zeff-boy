use crate::hardware::cartridge::{Mapper, Mirroring};

pub struct Mapper91 {
    prg_rom: Vec<u8>,
    chr: Vec<u8>,
    mirroring: Mirroring,
    submapper: u8,
    prg_banks: [u8; 2],
    chr_banks: [u8; 4],
    outer_bank: u8,
    irq_counter: u16,
    irq_divider: u8,
    irq_enabled: bool,
    irq_pending: bool,
}

impl Mapper91 {
    pub fn new(prg_rom: Vec<u8>, chr: Vec<u8>, mirroring: Mirroring, submapper: u8) -> Self {
        Self {
            prg_rom,
            chr,
            mirroring,
            submapper,
            prg_banks: [0, 1],
            chr_banks: [0, 1, 2, 3],
            outer_bank: 0,
            irq_counter: 0,
            irq_divider: 0,
            irq_enabled: false,
            irq_pending: false,
        }
    }

    fn register_addr(&self, addr: u16) -> u16 {
        if self.submapper == 1 {
            addr & 0xF007
        } else {
            addr & 0xF003
        }
    }

    fn prg_bank_count_8k(&self) -> usize {
        (self.prg_rom.len() / 0x2000).max(1)
    }

    fn chr_bank_count_2k(&self) -> usize {
        (self.chr.len() / 0x0800).max(1)
    }

    fn prg_outer_base(&self) -> usize {
        usize::from((self.outer_bank >> 1) & 0x03) * 16
    }

    fn chr_outer_base(&self) -> usize {
        usize::from(self.outer_bank & 0x01) * 256
    }

    fn prg_bank(&self, addr: u16) -> usize {
        let count = self.prg_bank_count_8k();
        match addr {
            0x8000..=0x9FFF => (self.prg_outer_base() + usize::from(self.prg_banks[0])) % count,
            0xA000..=0xBFFF => (self.prg_outer_base() + usize::from(self.prg_banks[1])) % count,
            0xC000..=0xDFFF => {
                let base = self.prg_outer_base();
                if base + 15 < count {
                    base + 14
                } else {
                    count.saturating_sub(2)
                }
            }
            0xE000..=0xFFFF => {
                let base = self.prg_outer_base();
                if base + 15 < count {
                    base + 15
                } else {
                    count.saturating_sub(1)
                }
            }
            _ => 0,
        }
    }

    fn chr_bank(&self, addr: u16) -> usize {
        let slot = usize::from(addr / 0x0800);
        (self.chr_outer_base() + usize::from(self.chr_banks[slot])) % self.chr_bank_count_2k()
    }

    fn irq_start(&mut self) {
        self.irq_enabled = true;
        self.irq_pending = false;
        if self.submapper == 0 {
            self.irq_counter = 64;
        }
    }

    fn irq_stop(&mut self) {
        self.irq_enabled = false;
        self.irq_pending = false;
    }
}

impl Mapper for Mapper91 {
    fn cpu_peek(&self, addr: u16) -> u8 {
        match addr {
            0x8000..=0xFFFF => {
                let bank = self.prg_bank(addr);
                let offset = addr as usize & 0x1FFF;
                self.prg_rom[(bank * 0x2000 + offset) % self.prg_rom.len()]
            }
            _ => 0,
        }
    }

    fn cpu_write(&mut self, addr: u16, val: u8) {
        match self.register_addr(addr) {
            0x6000..=0x6003 => self.chr_banks[usize::from(addr & 0x0003)] = val,
            0x6004 if self.submapper == 1 => self.mirroring = Mirroring::Horizontal,
            0x6005 if self.submapper == 1 => self.mirroring = Mirroring::Vertical,
            0x6006 if self.submapper == 1 => {
                self.irq_counter = (self.irq_counter & 0xFF00) | u16::from(val);
            }
            0x6007 if self.submapper == 1 => {
                self.irq_counter = (self.irq_counter & 0x00FF) | (u16::from(val) << 8);
            }
            0x7000..=0x7001 => self.prg_banks[usize::from(addr & 0x0001)] = val,
            0x7002 | 0x7006 => self.irq_stop(),
            0x7003 | 0x7007 => self.irq_start(),
            0x8000..=0x9FFF if self.submapper == 0 => self.outer_bank = addr as u8 & 0x07,
            _ => {}
        }
    }

    fn chr_read(&mut self, addr: u16) -> u8 {
        if self.chr.is_empty() {
            return 0;
        }
        let bank = self.chr_bank(addr);
        let offset = addr as usize & 0x07FF;
        self.chr[(bank * 0x0800 + offset) % self.chr.len()]
    }

    fn chr_write(&mut self, addr: u16, val: u8) {
        if self.chr.is_empty() {
            return;
        }
        let bank = self.chr_bank(addr);
        let offset = addr as usize & 0x07FF;
        let idx = (bank * 0x0800 + offset) % self.chr.len();
        self.chr[idx] = val;
    }

    fn mirroring(&self) -> Mirroring {
        self.mirroring
    }

    fn irq_pending(&self) -> bool {
        self.irq_pending
    }

    fn notify_scanline(&mut self) {
        if self.submapper != 0 || !self.irq_enabled || self.irq_pending {
            return;
        }
        if self.irq_counter == 0 {
            self.irq_pending = true;
        } else {
            self.irq_counter -= 1;
            if self.irq_counter == 0 {
                self.irq_pending = true;
            }
        }
    }

    fn clock_cpu(&mut self) {
        if self.submapper != 1 || !self.irq_enabled || self.irq_pending {
            return;
        }
        self.irq_divider = self.irq_divider.wrapping_add(1) & 0x03;
        if self.irq_divider == 0 {
            self.irq_counter = self.irq_counter.saturating_sub(5);
            if self.irq_counter == 0 {
                self.irq_pending = true;
            }
        }
    }

    fn write_state(&self, w: &mut crate::save_state::StateWriter) {
        w.write_u8(crate::save_state::encode_mirroring(self.mirroring));
        w.write_bytes(&self.prg_banks);
        w.write_bytes(&self.chr_banks);
        w.write_u8(self.outer_bank);
        w.write_u16(self.irq_counter);
        w.write_u8(self.irq_divider);
        w.write_bool(self.irq_enabled);
        w.write_bool(self.irq_pending);
        crate::save_state::write_chr_state(w, &self.chr);
    }

    fn read_state(&mut self, r: &mut crate::save_state::StateReader) -> anyhow::Result<()> {
        self.mirroring = crate::save_state::decode_mirroring(r.read_u8()?)?;
        r.read_exact(&mut self.prg_banks)?;
        r.read_exact(&mut self.chr_banks)?;
        self.outer_bank = r.read_u8()? & 0x07;
        self.irq_counter = r.read_u16()?;
        self.irq_divider = r.read_u8()? & 0x03;
        self.irq_enabled = r.read_bool()?;
        self.irq_pending = r.read_bool()?;
        crate::save_state::read_chr_state(r, &mut self.chr, "Mapper 91")?;
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
            chr.extend(vec![bank as u8; 0x0800]);
        }
        chr
    }

    #[test]
    fn switches_prg_chr_and_outer_bank_with_submapper0_masks() {
        let mut mapper = Mapper91::new(prg_banks(64), chr_banks(512), Mirroring::Vertical, 0);

        mapper.cpu_write(0x6000, 3);
        mapper.cpu_write(0x6001, 4);
        mapper.cpu_write(0x7000, 5);
        mapper.cpu_write(0x7001, 6);

        assert_eq!(mapper.cpu_peek(0x8000), 5);
        assert_eq!(mapper.cpu_peek(0xA000), 6);
        assert_eq!(mapper.cpu_peek(0xC000), 14);
        assert_eq!(mapper.chr_read(0x0000), 3);
        assert_eq!(mapper.chr_read(0x0800), 4);

        mapper.cpu_write(0x8004, 0x00);
        assert_eq!(mapper.cpu_peek(0x8000), 37);
    }

    #[test]
    fn submapper1_mirroring_and_irq_counter() {
        let mut mapper = Mapper91::new(prg_banks(16), chr_banks(16), Mirroring::Horizontal, 1);

        mapper.cpu_write(0x6005, 0);
        assert_eq!(mapper.mirroring(), Mirroring::Vertical);
        mapper.cpu_write(0x6006, 5);
        mapper.cpu_write(0x6007, 0);
        mapper.cpu_write(0x7007, 0);
        for _ in 0..4 {
            mapper.clock_cpu();
        }
        assert!(mapper.irq_pending());
    }
}
