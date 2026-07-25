use crate::hardware::cartridge::{Mapper, Mirroring};

pub struct Vrc3 {
    prg_rom: Vec<u8>,
    chr: Vec<u8>,
    prg_ram: [u8; 0x2000],
    mirroring: Mirroring,

    prg_bank: u8,

    irq_latch: u16,
    irq_counter: u16,
    irq_enabled: bool,
    irq_enabled_after_ack: bool,
    irq_8bit_mode: bool,
    irq_pending: bool,
}

impl Vrc3 {
    pub fn new(prg_rom: Vec<u8>, chr: Vec<u8>, mirroring: Mirroring) -> Self {
        Self {
            prg_rom,
            chr,
            prg_ram: [0; 0x2000],
            mirroring,
            prg_bank: 0,
            irq_latch: 0,
            irq_counter: 0,
            irq_enabled: false,
            irq_enabled_after_ack: false,
            irq_8bit_mode: false,
            irq_pending: false,
        }
    }

    fn prg_bank_count_16k(&self) -> usize {
        (self.prg_rom.len() / 0x4000).max(1)
    }

    fn read_prg_bank(&self, bank: usize, addr: u16) -> u8 {
        let bank = bank % self.prg_bank_count_16k();
        let offset = (addr as usize) & 0x3FFF;
        self.prg_rom[(bank * 0x4000 + offset) % self.prg_rom.len()]
    }

    fn set_irq_latch_nibble(&mut self, nibble: u16, val: u8) {
        let shift = nibble * 4;
        self.irq_latch = (self.irq_latch & !(0x000F << shift)) | (((val as u16) & 0x000F) << shift);
    }
}

impl Mapper for Vrc3 {
    fn cpu_peek(&self, addr: u16) -> u8 {
        match addr {
            0x6000..=0x7FFF => self.prg_ram[(addr - 0x6000) as usize],
            0x8000..=0xBFFF => self.read_prg_bank(self.prg_bank as usize, addr),
            0xC000..=0xFFFF => self.read_prg_bank(self.prg_bank_count_16k() - 1, addr),
            _ => 0,
        }
    }

    fn cpu_write(&mut self, addr: u16, val: u8) {
        match addr {
            0x6000..=0x7FFF => self.prg_ram[(addr - 0x6000) as usize] = val,
            0x8000..=0x8FFF => self.set_irq_latch_nibble(0, val),
            0x9000..=0x9FFF => self.set_irq_latch_nibble(1, val),
            0xA000..=0xAFFF => self.set_irq_latch_nibble(2, val),
            0xB000..=0xBFFF => self.set_irq_latch_nibble(3, val),
            0xC000..=0xCFFF => {
                self.irq_pending = false;
                self.irq_enabled_after_ack = val & 0x01 != 0;
                self.irq_enabled = val & 0x02 != 0;
                self.irq_8bit_mode = val & 0x04 != 0;
                if self.irq_enabled {
                    self.irq_counter = self.irq_latch;
                }
            }
            0xD000..=0xDFFF => {
                self.irq_pending = false;
                self.irq_enabled = self.irq_enabled_after_ack;
            }
            0xF000..=0xFFFF => self.prg_bank = val & 0x07,
            _ => {}
        }
    }

    fn chr_read(&mut self, addr: u16) -> u8 {
        if self.chr.is_empty() {
            return 0;
        }
        self.chr[addr as usize % self.chr.len()]
    }

    fn chr_write(&mut self, addr: u16, val: u8) {
        let len = self.chr.len();
        if len > 0 {
            self.chr[addr as usize % len] = val;
        }
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

        if self.irq_8bit_mode {
            let low = self.irq_counter as u8;
            if low == 0xFF {
                self.irq_counter = (self.irq_counter & 0xFF00) | (self.irq_latch & 0x00FF);
                self.irq_pending = true;
            } else {
                self.irq_counter = (self.irq_counter & 0xFF00) | low.wrapping_add(1) as u16;
            }
        } else if self.irq_counter == 0xFFFF {
            self.irq_counter = self.irq_latch;
            self.irq_pending = true;
        } else {
            self.irq_counter = self.irq_counter.wrapping_add(1);
        }
    }

    fn write_state(&self, w: &mut crate::save_state::StateWriter) {
        w.write_bytes(&self.prg_ram);
        w.write_u8(crate::save_state::encode_mirroring(self.mirroring));
        w.write_u8(self.prg_bank);
        w.write_u16(self.irq_latch);
        w.write_u16(self.irq_counter);
        w.write_bool(self.irq_enabled);
        w.write_bool(self.irq_enabled_after_ack);
        w.write_bool(self.irq_8bit_mode);
        w.write_bool(self.irq_pending);
        crate::save_state::write_chr_state(w, &self.chr);
    }

    fn read_state(&mut self, r: &mut crate::save_state::StateReader) -> anyhow::Result<()> {
        r.read_exact(&mut self.prg_ram)?;
        self.mirroring = crate::save_state::decode_mirroring(r.read_u8()?)?;
        self.prg_bank = r.read_u8()? & 0x07;
        self.irq_latch = r.read_u16()?;
        self.irq_counter = r.read_u16()?;
        self.irq_enabled = r.read_bool()?;
        self.irq_enabled_after_ack = r.read_bool()?;
        self.irq_8bit_mode = r.read_bool()?;
        self.irq_pending = r.read_bool()?;
        crate::save_state::read_chr_state(r, &mut self.chr, "VRC3")?;
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

    #[test]
    fn switches_prg_bank_and_counts_irq() {
        let mut mapper = Vrc3::new(prg_banks(8), vec![0; 0x2000], Mirroring::Horizontal);

        mapper.cpu_write(0xF000, 0x03);
        assert_eq!(mapper.cpu_peek(0x8000), 0x03);
        assert_eq!(mapper.cpu_peek(0xC000), 0x07);

        mapper.cpu_write(0x8000, 0x0E);
        mapper.cpu_write(0x9000, 0x0F);
        mapper.cpu_write(0xA000, 0x0F);
        mapper.cpu_write(0xB000, 0x0F);
        mapper.cpu_write(0xC000, 0x02);

        assert!(!mapper.irq_pending());
        mapper.clock_cpu();
        assert!(!mapper.irq_pending());
        mapper.clock_cpu();
        assert!(mapper.irq_pending());
    }

    #[test]
    fn acknowledge_moves_a_bit_to_enable() {
        let mut mapper = Vrc3::new(prg_banks(8), vec![0; 0x2000], Mirroring::Horizontal);

        mapper.cpu_write(0x8000, 0x0F);
        mapper.cpu_write(0x9000, 0x0F);
        mapper.cpu_write(0xC000, 0x05);
        mapper.cpu_write(0xD000, 0x00);

        for _ in 0..=0x100 {
            mapper.clock_cpu();
        }
        assert!(mapper.irq_pending());
    }
}
