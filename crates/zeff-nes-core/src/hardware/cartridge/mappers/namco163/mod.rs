pub(crate) mod audio;

use crate::hardware::cartridge::{Mapper, Mirroring};
use audio::N163Audio;

pub struct Namco163 {
    prg_rom: Vec<u8>,
    chr: Vec<u8>,
    prg_ram: Vec<u8>,
    has_battery: bool,
    mirroring: Mirroring,

    prg_banks: [u8; 3],
    chr_banks: [u8; 8],
    nt_banks: [u8; 4],

    chr_ram_low: bool,
    chr_ram_high: bool,

    sound_enabled: bool,

    irq_counter: u16,
    irq_enable: bool,
    irq_pending: bool,

    audio: N163Audio,
}

impl Namco163 {
    pub fn new(
        prg_rom: Vec<u8>,
        chr: Vec<u8>,
        mirroring: Mirroring,
        prg_ram_size: usize,
        has_battery: bool,
    ) -> Self {
        let ram_size = if prg_ram_size > 0 {
            prg_ram_size
        } else {
            0x2000
        };
        Self {
            prg_rom,
            chr,
            prg_ram: vec![0; ram_size],
            has_battery,
            mirroring,
            prg_banks: [0; 3],
            chr_banks: [0; 8],
            nt_banks: [0; 4],
            chr_ram_low: false,
            chr_ram_high: false,
            sound_enabled: true,
            irq_counter: 0,
            irq_enable: false,
            irq_pending: false,
            audio: N163Audio::new(),
        }
    }

    fn prg_bank_count_8k(&self) -> usize {
        (self.prg_rom.len() / 0x2000).max(1)
    }

    fn chr_bank_count_1k(&self) -> usize {
        (self.chr.len() / 0x0400).max(1)
    }

    fn read_prg_bank(&self, bank: u8, offset: usize) -> u8 {
        let bank = (bank as usize) % self.prg_bank_count_8k();
        self.prg_rom[bank * 0x2000 + offset]
    }

    fn read_chr_1k(&self, bank: u8, offset: usize) -> u8 {
        if self.chr.is_empty() {
            return 0;
        }
        let bank = (bank as usize) % self.chr_bank_count_1k();
        self.chr[bank * 0x0400 + offset]
    }

    fn is_chr_ram_bank(&self, bank_index: usize) -> bool {
        if bank_index < 4 {
            self.chr_ram_low
        } else {
            self.chr_ram_high
        }
    }

    fn write_chr_1k(&mut self, bank: u8, offset: usize, val: u8) {
        if self.chr.is_empty() {
            return;
        }
        let bank = (bank as usize) % self.chr_bank_count_1k();
        self.chr[bank * 0x0400 + offset] = val;
    }
}

impl Mapper for Namco163 {
    fn cpu_peek(&self, addr: u16) -> u8 {
        match addr {
            0x4800..=0x4FFF => self.audio.ram[self.audio.ram[0x7F].min(0x7F) as usize],
            0x5000..=0x57FF => self.irq_counter as u8,
            0x5800..=0x5FFF => {
                (self.irq_counter >> 8) as u8 | if self.irq_enable { 0x80 } else { 0 }
            }
            0x6000..=0x7FFF => {
                if self.prg_ram.is_empty() {
                    0
                } else {
                    let offset = (addr as usize - 0x6000) % self.prg_ram.len();
                    self.prg_ram[offset]
                }
            }
            0x8000..=0x9FFF => {
                let bank = self.prg_banks[0] & 0x3F;
                self.read_prg_bank(bank, (addr as usize) & 0x1FFF)
            }
            0xA000..=0xBFFF => {
                let bank = self.prg_banks[1] & 0x3F;
                self.read_prg_bank(bank, (addr as usize) & 0x1FFF)
            }
            0xC000..=0xDFFF => {
                let bank = self.prg_banks[2] & 0x3F;
                self.read_prg_bank(bank, (addr as usize) & 0x1FFF)
            }
            0xE000..=0xFFFF => {
                let last = self.prg_bank_count_8k().saturating_sub(1) as u8;
                self.read_prg_bank(last, (addr as usize) & 0x1FFF)
            }
            _ => 0,
        }
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        match addr {
            0x4800..=0x4FFF => self.audio.read_data(),
            0x5000..=0x57FF => {
                let val = self.irq_counter as u8;
                self.irq_pending = false;
                val
            }
            0x5800..=0x5FFF => {
                let val = (self.irq_counter >> 8) as u8 | if self.irq_enable { 0x80 } else { 0 };
                self.irq_pending = false;
                val
            }
            _ => self.cpu_peek(addr),
        }
    }

    fn cpu_write(&mut self, addr: u16, val: u8) {
        match addr {
            0x4800..=0x4FFF => {
                self.audio.write_data(val);
            }
            0x5000..=0x57FF => {
                self.irq_counter = (self.irq_counter & 0xFF00) | val as u16;
                self.irq_pending = false;
            }
            0x5800..=0x5FFF => {
                self.irq_counter = (self.irq_counter & 0x00FF) | ((val as u16 & 0x7F) << 8);
                self.irq_enable = val & 0x80 != 0;
                self.irq_pending = false;
            }
            0x6000..=0x7FFF if !self.prg_ram.is_empty() => {
                let offset = (addr as usize - 0x6000) % self.prg_ram.len();
                self.prg_ram[offset] = val;
            }
            0x8000..=0x87FF => self.chr_banks[0] = val,
            0x8800..=0x8FFF => self.chr_banks[1] = val,
            0x9000..=0x97FF => self.chr_banks[2] = val,
            0x9800..=0x9FFF => self.chr_banks[3] = val,
            0xA000..=0xA7FF => self.chr_banks[4] = val,
            0xA800..=0xAFFF => self.chr_banks[5] = val,
            0xB000..=0xB7FF => self.chr_banks[6] = val,
            0xB800..=0xBFFF => self.chr_banks[7] = val,
            0xC000..=0xC7FF => self.nt_banks[0] = val,
            0xC800..=0xCFFF => self.nt_banks[1] = val,
            0xD000..=0xD7FF => self.nt_banks[2] = val,
            0xD800..=0xDFFF => self.nt_banks[3] = val,
            0xE000..=0xE7FF => {
                self.prg_banks[0] = val & 0x3F;
                self.sound_enabled = val & 0x40 == 0;
            }
            0xE800..=0xEFFF => {
                self.prg_banks[1] = val & 0x3F;
                self.chr_ram_low = val & 0x40 != 0;
                self.chr_ram_high = val & 0x80 != 0;
            }
            0xF000..=0xF7FF => {
                self.prg_banks[2] = val & 0x3F;
            }
            0xF800..=0xFFFF => {
                self.audio.write_addr(val);
            }
            _ => {}
        }
    }

    fn chr_read(&mut self, addr: u16) -> u8 {
        if self.chr.is_empty() {
            return 0;
        }
        let bank_idx = (addr >> 10) as usize & 7;
        let bank = self.chr_banks[bank_idx];
        let offset = (addr as usize) & 0x03FF;
        self.read_chr_1k(bank, offset)
    }

    fn chr_write(&mut self, addr: u16, val: u8) {
        let bank_idx = (addr >> 10) as usize & 7;
        let bank = self.chr_banks[bank_idx];
        let offset = (addr as usize) & 0x03FF;

        if self.is_chr_ram_bank(bank_idx) {
            self.write_chr_1k(bank, offset, val);
        }
    }

    fn ppu_nametable_read(&mut self, addr: u16, ciram: &[u8; 0x800]) -> Option<u8> {
        let nt_idx = ((addr >> 10) & 3) as usize;
        let bank = self.nt_banks[nt_idx];
        let offset = (addr as usize) & 0x03FF;

        if bank >= 0xE0 {
            let ciram_bank = (bank & 1) as usize;
            Some(ciram[ciram_bank * 0x400 + offset])
        } else {
            Some(self.read_chr_1k(bank, offset))
        }
    }

    fn ppu_nametable_write(&mut self, addr: u16, val: u8, ciram: &mut [u8; 0x800]) -> bool {
        let nt_idx = ((addr >> 10) & 3) as usize;
        let bank = self.nt_banks[nt_idx];
        let offset = (addr as usize) & 0x03FF;

        if bank >= 0xE0 {
            let ciram_bank = (bank & 1) as usize;
            ciram[ciram_bank * 0x400 + offset] = val;
            true
        } else {
            self.write_chr_1k(bank, offset, val);
            true
        }
    }

    fn mirroring(&self) -> Mirroring {
        self.mirroring
    }

    fn irq_pending(&self) -> bool {
        self.irq_pending
    }

    fn clock_cpu(&mut self) {
        if self.sound_enabled {
            self.audio.tick();
        }

        if self.irq_enable {
            if self.irq_counter == 0x7FFF {
                self.irq_pending = true;
            } else {
                self.irq_counter += 1;
            }
        }
    }

    fn audio_output(&self) -> f32 {
        if self.sound_enabled {
            self.audio.output()
        } else {
            0.0
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
        w.write_u8(crate::save_state::encode_mirroring(self.mirroring));
        w.write_bool(self.has_battery);
        w.write_bytes(&self.prg_banks);
        w.write_bytes(&self.chr_banks);
        w.write_bytes(&self.nt_banks);
        w.write_bool(self.chr_ram_low);
        w.write_bool(self.chr_ram_high);
        w.write_bool(self.sound_enabled);
        w.write_u16(self.irq_counter);
        w.write_bool(self.irq_enable);
        w.write_bool(self.irq_pending);
        w.write_bytes(&self.prg_ram);
        self.audio.write_state(w);
        crate::save_state::write_chr_state(w, &self.chr);
    }

    fn read_state(&mut self, r: &mut crate::save_state::StateReader) -> anyhow::Result<()> {
        self.mirroring = crate::save_state::decode_mirroring(r.read_u8()?)?;
        self.has_battery = r.read_bool()?;
        r.read_exact(&mut self.prg_banks)?;
        r.read_exact(&mut self.chr_banks)?;
        r.read_exact(&mut self.nt_banks)?;
        self.chr_ram_low = r.read_bool()?;
        self.chr_ram_high = r.read_bool()?;
        self.sound_enabled = r.read_bool()?;
        self.irq_counter = r.read_u16()?;
        self.irq_enable = r.read_bool()?;
        self.irq_pending = r.read_bool()?;

        let ram = r.read_vec(128 * 1024)?;
        if ram.len() != self.prg_ram.len() {
            anyhow::bail!(
                "N163 PRG-RAM size mismatch: expected {}, got {}",
                self.prg_ram.len(),
                ram.len()
            );
        }
        self.prg_ram = ram;

        self.audio.read_state(r)?;

        crate::save_state::read_chr_state(r, &mut self.chr, "N163")?;
        Ok(())
    }
}

#[cfg(test)]
mod tests;
