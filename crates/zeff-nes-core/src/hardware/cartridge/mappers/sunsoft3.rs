use crate::hardware::cartridge::{Mapper, Mirroring};

pub struct Sunsoft3 {
    prg_rom: Vec<u8>,
    chr: Vec<u8>,
    mirroring: Mirroring,

    prg_bank: u8,
    chr_banks: [u8; 4],

    irq_counter: u16,
    irq_high_next: bool,
    irq_enabled: bool,
    irq_pending: bool,
}

impl Sunsoft3 {
    pub fn new(prg_rom: Vec<u8>, chr: Vec<u8>, mirroring: Mirroring) -> Self {
        Self {
            prg_rom,
            chr,
            mirroring,
            prg_bank: 0,
            chr_banks: [0, 1, 2, 3],
            irq_counter: 0,
            irq_high_next: true,
            irq_enabled: false,
            irq_pending: false,
        }
    }

    fn prg_bank_count_16k(&self) -> usize {
        (self.prg_rom.len() / 0x4000).max(1)
    }

    fn chr_bank_count_2k(&self) -> usize {
        (self.chr.len() / 0x0800).max(1)
    }

    fn prg_read_bank(&self, bank: usize, addr: u16) -> u8 {
        let bank = bank % self.prg_bank_count_16k();
        let offset = (addr as usize) & 0x3FFF;
        self.prg_rom[(bank * 0x4000 + offset) % self.prg_rom.len()]
    }

    fn chr_addr(&self, addr: u16) -> usize {
        let slot = ((addr as usize) >> 11) & 0x03;
        let bank = self.chr_banks[slot] as usize % self.chr_bank_count_2k();
        let offset = (addr as usize) & 0x07FF;
        (bank * 0x0800 + offset) % self.chr.len()
    }
}

impl Mapper for Sunsoft3 {
    fn cpu_peek(&self, addr: u16) -> u8 {
        match addr {
            0x8000..=0xBFFF => self.prg_read_bank(self.prg_bank as usize, addr),
            0xC000..=0xFFFF => self.prg_read_bank(self.prg_bank_count_16k() - 1, addr),
            _ => 0,
        }
    }

    fn cpu_write(&mut self, addr: u16, val: u8) {
        if (0x8000..=0xFFFF).contains(&addr) && (addr & 0x8800) == 0x8000 {
            self.irq_pending = false;
        }

        match addr & 0xF800 {
            0x8800 => self.chr_banks[0] = val & 0x3F,
            0x9800 => self.chr_banks[1] = val & 0x3F,
            0xA800 => self.chr_banks[2] = val & 0x3F,
            0xB800 => self.chr_banks[3] = val & 0x3F,
            0xC800 => {
                if self.irq_high_next {
                    self.irq_counter = (self.irq_counter & 0x00FF) | ((val as u16) << 8);
                    self.irq_high_next = false;
                } else {
                    self.irq_counter = (self.irq_counter & 0xFF00) | val as u16;
                    self.irq_high_next = true;
                }
            }
            0xD800 => {
                self.irq_enabled = val & 0x10 != 0;
                self.irq_high_next = true;
            }
            0xE800 => {
                self.mirroring = match val & 0x03 {
                    0 => Mirroring::Vertical,
                    1 => Mirroring::Horizontal,
                    2 => Mirroring::SingleScreenLower,
                    3 => Mirroring::SingleScreenUpper,
                    _ => unreachable!(),
                };
            }
            0xF800 => self.prg_bank = val & 0x0F,
            _ => {}
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

    fn irq_pending(&self) -> bool {
        self.irq_pending
    }

    fn clock_cpu(&mut self) {
        if !self.irq_enabled {
            return;
        }

        if self.irq_counter == 0 {
            self.irq_counter = 0xFFFF;
            self.irq_enabled = false;
            self.irq_pending = true;
        } else {
            self.irq_counter -= 1;
        }
    }

    fn write_state(&self, w: &mut crate::save_state::StateWriter) {
        w.write_u8(crate::save_state::encode_mirroring(self.mirroring));
        w.write_u8(self.prg_bank);
        w.write_bytes(&self.chr_banks);
        w.write_u16(self.irq_counter);
        w.write_bool(self.irq_high_next);
        w.write_bool(self.irq_enabled);
        w.write_bool(self.irq_pending);
        crate::save_state::write_chr_state(w, &self.chr);
    }

    fn read_state(&mut self, r: &mut crate::save_state::StateReader) -> anyhow::Result<()> {
        self.mirroring = crate::save_state::decode_mirroring(r.read_u8()?)?;
        self.prg_bank = r.read_u8()? & 0x0F;
        r.read_exact(&mut self.chr_banks)?;
        self.irq_counter = r.read_u16()?;
        self.irq_high_next = r.read_bool()?;
        self.irq_enabled = r.read_bool()?;
        self.irq_pending = r.read_bool()?;
        crate::save_state::read_chr_state(r, &mut self.chr, "Sunsoft-3")?;
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

    fn chr_banks(count: usize) -> Vec<u8> {
        let mut chr = Vec::new();
        for bank in 0..count {
            chr.extend(vec![bank as u8; 0x0800]);
        }
        chr
    }

    #[test]
    fn switches_prg_chr_and_mirroring() {
        let mut mapper = Sunsoft3::new(prg_banks(16), chr_banks(64), Mirroring::Vertical);

        mapper.cpu_write(0xF800, 0x05);
        mapper.cpu_write(0x9800, 0x0A);
        mapper.cpu_write(0xE800, 0x03);

        assert_eq!(mapper.cpu_peek(0x8000), 0x05);
        assert_eq!(mapper.cpu_peek(0xC000), 0x0F);
        assert_eq!(mapper.chr_read(0x0800), 0x0A);
        assert_eq!(mapper.mirroring(), Mirroring::SingleScreenUpper);
    }

    #[test]
    fn irq_write_twice_counter_wraps_and_pauses() {
        let mut mapper = Sunsoft3::new(prg_banks(4), chr_banks(8), Mirroring::Horizontal);

        mapper.cpu_write(0xC800, 0x00);
        mapper.cpu_write(0xC800, 0x02);
        mapper.cpu_write(0xD800, 0x10);

        mapper.clock_cpu();
        mapper.clock_cpu();
        assert!(!mapper.irq_pending());
        mapper.clock_cpu();
        assert!(mapper.irq_pending());
        assert!(!mapper.irq_enabled);

        mapper.cpu_write(0x8000, 0x00);
        assert!(!mapper.irq_pending());
    }
}
