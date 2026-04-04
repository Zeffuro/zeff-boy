use crate::hardware::cartridge::{Mapper, Mirroring};

pub struct Fme7 {
    prg_rom: Vec<u8>,
    chr: Vec<u8>,
    prg_ram: Vec<u8>,
    has_battery: bool,

    command: u8,
    chr_banks: [u8; 8],
    prg_6000_bank: u8,
    prg_8000_bank: u8,
    prg_a000_bank: u8,
    prg_c000_bank: u8,
    prg_ram_select: bool,
    prg_ram_enable: bool,

    mirroring: Mirroring,

    irq_counter_enable: bool,
    irq_enable: bool,
    irq_counter: u16,
    irq_pending: bool,
}

impl Fme7 {
    pub fn new(
        prg_rom: Vec<u8>,
        chr: Vec<u8>,
        mirroring: Mirroring,
        prg_ram_size: usize,
        has_battery: bool,
    ) -> Self {
        let ram_len = if prg_ram_size == 0 {
            0x2000
        } else {
            prg_ram_size
        };
        Self {
            prg_rom,
            chr,
            prg_ram: vec![0; ram_len],
            has_battery,
            command: 0,
            chr_banks: [0; 8],
            prg_6000_bank: 0,
            prg_8000_bank: 0,
            prg_a000_bank: 1,
            prg_c000_bank: 2,
            prg_ram_select: false,
            prg_ram_enable: false,
            mirroring,
            irq_counter_enable: false,
            irq_enable: false,
            irq_counter: 0,
            irq_pending: false,
        }
    }

    fn prg_bank_count_8k(&self) -> usize {
        (self.prg_rom.len() / 0x2000).max(1)
    }

    fn chr_bank_count_1k(&self) -> usize {
        (self.chr.len() / 0x0400).max(1)
    }

    fn map_chr_bank(&self, addr: u16) -> usize {
        let slot = ((addr as usize) >> 10) & 0x07;
        (self.chr_banks[slot] as usize) % self.chr_bank_count_1k()
    }

    fn prg_rom_read_8k(&self, bank: u8, addr: u16) -> u8 {
        let bank = (bank as usize) % self.prg_bank_count_8k();
        let offset = (addr as usize) & 0x1FFF;
        self.prg_rom[bank * 0x2000 + offset]
    }

    fn prg_ram_read(&self, bank: u8, addr: u16) -> u8 {
        if self.prg_ram.is_empty() {
            return 0;
        }
        let bank_count = (self.prg_ram.len() / 0x2000).max(1);
        let bank = (bank as usize) % bank_count;
        let offset = (addr as usize) & 0x1FFF;
        self.prg_ram[bank * 0x2000 + offset]
    }

    fn prg_ram_write(&mut self, bank: u8, addr: u16, val: u8) {
        if self.prg_ram.is_empty() {
            return;
        }
        let bank_count = (self.prg_ram.len() / 0x2000).max(1);
        let bank = (bank as usize) % bank_count;
        let offset = (addr as usize) & 0x1FFF;
        self.prg_ram[bank * 0x2000 + offset] = val;
    }

    fn write_parameter(&mut self, val: u8) {
        match self.command & 0x0F {
            0x0..=0x7 => self.chr_banks[(self.command & 0x07) as usize] = val,
            0x8 => {
                self.prg_ram_enable = val & 0x80 != 0;
                self.prg_ram_select = val & 0x40 != 0;
                self.prg_6000_bank = val & 0x3F;
            }
            0x9 => self.prg_8000_bank = val & 0x3F,
            0xA => self.prg_a000_bank = val & 0x3F,
            0xB => self.prg_c000_bank = val & 0x3F,
            0xC => {
                self.mirroring = match val & 0x03 {
                    0 => Mirroring::Vertical,
                    1 => Mirroring::Horizontal,
                    2 => Mirroring::SingleScreenLower,
                    3 => Mirroring::SingleScreenUpper,
                    _ => Mirroring::Horizontal,
                };
            }
            0xD => {
                self.irq_pending = false;
                self.irq_enable = val & 0x01 != 0;
                self.irq_counter_enable = val & 0x80 != 0;
            }
            0xE => {
                self.irq_counter = (self.irq_counter & 0xFF00) | (val as u16);
            }
            0xF => {
                self.irq_counter = (self.irq_counter & 0x00FF) | ((val as u16) << 8);
            }
            _ => {}
        }
    }
}

impl Mapper for Fme7 {
    fn cpu_peek(&self, addr: u16) -> u8 {
        match addr {
            0x6000..=0x7FFF => {
                if self.prg_ram_select {
                    if self.prg_ram_enable {
                        self.prg_ram_read(self.prg_6000_bank, addr)
                    } else {
                        0
                    }
                } else {
                    self.prg_rom_read_8k(self.prg_6000_bank, addr)
                }
            }
            0x8000..=0x9FFF => self.prg_rom_read_8k(self.prg_8000_bank, addr),
            0xA000..=0xBFFF => self.prg_rom_read_8k(self.prg_a000_bank, addr),
            0xC000..=0xDFFF => self.prg_rom_read_8k(self.prg_c000_bank, addr),
            0xE000..=0xFFFF => {
                let last = (self.prg_bank_count_8k() - 1) as u8;
                self.prg_rom_read_8k(last, addr)
            }
            _ => 0,
        }
    }

    fn cpu_write(&mut self, addr: u16, val: u8) {
        match addr {
            0x6000..=0x7FFF => {
                if self.prg_ram_select && self.prg_ram_enable {
                    self.prg_ram_write(self.prg_6000_bank, addr, val);
                }
            }
            0x8000..=0x9FFF => {
                self.command = val & 0x0F;
            }
            0xA000..=0xBFFF => {
                self.write_parameter(val);
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

    fn clock_cpu(&mut self) {
        if !self.irq_counter_enable {
            return;
        }

        let prev = self.irq_counter;
        self.irq_counter = self.irq_counter.wrapping_sub(1);
        if prev == 0 && self.irq_enable {
            self.irq_pending = true;
        }
    }

    fn dump_battery_data(&self) -> Option<Vec<u8>> {
        if self.has_battery && !self.prg_ram.is_empty() {
            Some(self.prg_ram.clone())
        } else {
            None
        }
    }

    fn load_battery_data(&mut self, bytes: &[u8]) -> anyhow::Result<()> {
        if self.prg_ram.is_empty() {
            return Ok(());
        }
        let copy_len = self.prg_ram.len().min(bytes.len());
        self.prg_ram[..copy_len].copy_from_slice(&bytes[..copy_len]);
        if copy_len < self.prg_ram.len() {
            self.prg_ram[copy_len..].fill(0);
        }
        Ok(())
    }

    fn write_state(&self, w: &mut crate::save_state::StateWriter) {
        w.write_u8(self.command);
        w.write_bytes(&self.chr_banks);
        w.write_u8(self.prg_6000_bank);
        w.write_u8(self.prg_8000_bank);
        w.write_u8(self.prg_a000_bank);
        w.write_u8(self.prg_c000_bank);
        w.write_bool(self.prg_ram_select);
        w.write_bool(self.prg_ram_enable);

        w.write_u8(crate::save_state::encode_mirroring(self.mirroring));

        w.write_bool(self.irq_counter_enable);
        w.write_bool(self.irq_enable);
        w.write_u16(self.irq_counter);
        w.write_bool(self.irq_pending);

        w.write_bool(self.has_battery);
        w.write_vec(&self.prg_ram);
        w.write_vec(&self.chr);
    }

    fn read_state(&mut self, r: &mut crate::save_state::StateReader) -> anyhow::Result<()> {
        self.command = r.read_u8()?;
        r.read_exact(&mut self.chr_banks)?;
        self.prg_6000_bank = r.read_u8()?;
        self.prg_8000_bank = r.read_u8()?;
        self.prg_a000_bank = r.read_u8()?;
        self.prg_c000_bank = r.read_u8()?;
        self.prg_ram_select = r.read_bool()?;
        self.prg_ram_enable = r.read_bool()?;

        self.mirroring = crate::save_state::decode_mirroring(r.read_u8()?)?;

        self.irq_counter_enable = r.read_bool()?;
        self.irq_enable = r.read_bool()?;
        self.irq_counter = r.read_u16()?;
        self.irq_pending = r.read_bool()?;

        self.has_battery = r.read_bool()?;

        let prg_ram = r.read_vec(512 * 1024)?;
        if prg_ram.len() != self.prg_ram.len() {
            anyhow::bail!(
                "FME-7 PRG RAM size mismatch: expected {}, got {}",
                self.prg_ram.len(),
                prg_ram.len()
            );
        }
        self.prg_ram = prg_ram;

        let chr = r.read_vec(512 * 1024)?;
        if chr.len() != self.chr.len() {
            anyhow::bail!(
                "FME-7 CHR size mismatch: expected {}, got {}",
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
