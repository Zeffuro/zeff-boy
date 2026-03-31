use crate::hardware::cartridge::{Mapper, Mirroring};

pub struct Vrc4 {
    prg_rom: Vec<u8>,
    chr: Vec<u8>,
    prg_ram: [u8; 0x2000],
    mirroring: Mirroring,

    a0_mask: u16,
    a1_mask: u16,

    prg_bank_0: u8,
    prg_bank_1: u8,
    prg_swap_mode: bool,

    chr_banks: [u16; 8],

    irq_latch: u8,
    irq_counter: u8,
    irq_prescaler: i32,
    irq_enabled: bool,
    irq_enabled_after_ack: bool,
    irq_cycle_mode: bool,
    irq_pending: bool,
}

impl Vrc4 {
    pub fn new(
        prg_rom: Vec<u8>,
        chr: Vec<u8>,
        mirroring: Mirroring,
        a0_mask: u16,
        a1_mask: u16,
    ) -> Self {
        Self {
            prg_rom,
            chr,
            prg_ram: [0; 0x2000],
            mirroring,
            a0_mask,
            a1_mask,
            prg_bank_0: 0,
            prg_bank_1: 0,
            prg_swap_mode: false,
            chr_banks: [0; 8],
            irq_latch: 0,
            irq_counter: 0,
            irq_prescaler: 341,
            irq_enabled: false,
            irq_enabled_after_ack: false,
            irq_cycle_mode: false,
            irq_pending: false,
        }
    }

    fn prg_bank_count_8k(&self) -> usize {
        (self.prg_rom.len() / 0x2000).max(1)
    }

    fn chr_bank_count_1k(&self) -> usize {
        (self.chr.len() / 0x0400).max(1)
    }

    fn decode_reg(&self, addr: u16) -> u8 {
        let b0 = if addr & self.a0_mask != 0 { 1 } else { 0 };
        let b1 = if addr & self.a1_mask != 0 { 2 } else { 0 };
        b0 | b1
    }

    fn clock_irq_counter(&mut self) {
        if self.irq_counter == 0xFF {
            self.irq_counter = self.irq_latch;
            self.irq_pending = true;
        } else {
            self.irq_counter += 1;
        }
    }
}

impl Mapper for Vrc4 {
    fn cpu_read(&self, addr: u16) -> u8 {
        match addr {
            0x6000..=0x7FFF => self.prg_ram[(addr - 0x6000) as usize],
            0x8000..=0x9FFF => {
                let bank = if self.prg_swap_mode {
                    self.prg_bank_count_8k().saturating_sub(2)
                } else {
                    self.prg_bank_0 as usize
                } % self.prg_bank_count_8k();
                let offset = (addr as usize) & 0x1FFF;
                self.prg_rom[bank * 0x2000 + offset]
            }
            0xA000..=0xBFFF => {
                let bank = (self.prg_bank_1 as usize) % self.prg_bank_count_8k();
                let offset = (addr as usize) & 0x1FFF;
                self.prg_rom[bank * 0x2000 + offset]
            }
            0xC000..=0xDFFF => {
                let bank = if self.prg_swap_mode {
                    self.prg_bank_0 as usize
                } else {
                    self.prg_bank_count_8k().saturating_sub(2)
                } % self.prg_bank_count_8k();
                let offset = (addr as usize) & 0x1FFF;
                self.prg_rom[bank * 0x2000 + offset]
            }
            0xE000..=0xFFFF => {
                let bank = self.prg_bank_count_8k().saturating_sub(1);
                let offset = (addr as usize) & 0x1FFF;
                self.prg_rom[bank * 0x2000 + offset]
            }
            _ => 0,
        }
    }

    fn cpu_write(&mut self, addr: u16, val: u8) {
        match addr {
            0x6000..=0x7FFF => {
                self.prg_ram[(addr - 0x6000) as usize] = val;
            }
            0x8000..=0x8FFF => {
                self.prg_bank_0 = val & 0x1F;
            }
            0x9000..=0x9FFF => {
                let reg = self.decode_reg(addr);
                match reg {
                    0 | 1 => {
                        self.mirroring = match val & 0x03 {
                            0 => Mirroring::Vertical,
                            1 => Mirroring::Horizontal,
                            2 => Mirroring::SingleScreenLower,
                            3 => Mirroring::SingleScreenUpper,
                            _ => unreachable!(),
                        };
                    }
                    2 | 3 => {
                        self.prg_swap_mode = val & 0x02 != 0;
                    }
                    _ => {}
                }
            }
            0xA000..=0xAFFF => {
                self.prg_bank_1 = val & 0x1F;
            }
            0xB000..=0xEFFF => {
                let base = ((addr as usize) - 0xB000) >> 12; // 0..3
                let reg = self.decode_reg(addr);

                let chr_index = base * 2 + (reg >> 1) as usize; // 0..7
                if chr_index >= 8 {
                    return;
                }

                if reg & 1 == 0 {
                    self.chr_banks[chr_index] =
                        (self.chr_banks[chr_index] & 0x1F0) | (val as u16 & 0x0F);
                } else {
                    self.chr_banks[chr_index] =
                        (self.chr_banks[chr_index] & 0x00F) | ((val as u16 & 0x1F) << 4);
                }
            }
            0xF000..=0xFFFF => {
                let reg = self.decode_reg(addr);
                match reg {
                    0 => {
                        self.irq_latch = (self.irq_latch & 0xF0) | (val & 0x0F);
                    }
                    1 => {
                        self.irq_latch = (self.irq_latch & 0x0F) | ((val & 0x0F) << 4);
                    }
                    2 => {
                        self.irq_pending = false;
                        self.irq_enabled_after_ack = val & 0x01 != 0;
                        self.irq_enabled = val & 0x02 != 0;
                        self.irq_cycle_mode = val & 0x04 != 0;

                        if self.irq_enabled {
                            self.irq_counter = self.irq_latch;
                            self.irq_prescaler = 341;
                        }
                    }
                    3 => {
                        self.irq_pending = false;
                        self.irq_enabled = self.irq_enabled_after_ack;
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }

    fn chr_read(&self, addr: u16) -> u8 {
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

        if self.irq_cycle_mode {
            self.clock_irq_counter();
        } else {
            self.irq_prescaler -= 3;
            if self.irq_prescaler <= 0 {
                self.irq_prescaler += 341;
                self.clock_irq_counter();
            }
        }
    }

    fn write_state(&self, w: &mut crate::save_state::StateWriter) {
        w.write_bytes(&self.prg_ram);
        w.write_u8(crate::save_state::encode_mirroring(self.mirroring));

        w.write_u8(self.prg_bank_0);
        w.write_u8(self.prg_bank_1);
        w.write_bool(self.prg_swap_mode);

        for &bank in &self.chr_banks {
            w.write_u16(bank);
        }

        w.write_u8(self.irq_latch);
        w.write_u8(self.irq_counter);
        w.write_u32(self.irq_prescaler as u32);
        w.write_bool(self.irq_enabled);
        w.write_bool(self.irq_enabled_after_ack);
        w.write_bool(self.irq_cycle_mode);
        w.write_bool(self.irq_pending);

        w.write_vec(&self.chr);
    }

    fn read_state(&mut self, r: &mut crate::save_state::StateReader) -> anyhow::Result<()> {
        r.read_exact(&mut self.prg_ram)?;
        self.mirroring = crate::save_state::decode_mirroring(r.read_u8()?)?;

        self.prg_bank_0 = r.read_u8()?;
        self.prg_bank_1 = r.read_u8()?;
        self.prg_swap_mode = r.read_bool()?;

        for bank in &mut self.chr_banks {
            *bank = r.read_u16()?;
        }

        self.irq_latch = r.read_u8()?;
        self.irq_counter = r.read_u8()?;
        self.irq_prescaler = r.read_u32()? as i32;
        self.irq_enabled = r.read_bool()?;
        self.irq_enabled_after_ack = r.read_bool()?;
        self.irq_cycle_mode = r.read_bool()?;
        self.irq_pending = r.read_bool()?;

        let chr = r.read_vec(512 * 1024)?;
        if chr.len() != self.chr.len() {
            anyhow::bail!(
                "VRC4 CHR size mismatch: expected {}, got {}",
                self.chr.len(),
                chr.len()
            );
        }
        self.chr = chr;
        Ok(())
    }
}
