pub(crate) mod audio;

use crate::hardware::cartridge::{Mapper, Mirroring};
use audio::Vrc7Audio;

pub struct Vrc7 {
    prg_rom: Vec<u8>,
    chr: Vec<u8>,
    prg_ram: Vec<u8>,
    has_battery: bool,
    mirroring: Mirroring,

    prg_banks: [u8; 3],
    chr_banks: [u8; 8],

    irq_latch: u8,
    irq_counter: u8,
    irq_prescaler: i32,
    irq_enabled: bool,
    irq_enabled_after_ack: bool,
    irq_cycle_mode: bool,
    irq_pending: bool,

    audio: Vrc7Audio,

    wram_enable: bool,
}

impl Vrc7 {
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
            irq_latch: 0,
            irq_counter: 0,
            irq_prescaler: 341,
            irq_enabled: false,
            irq_enabled_after_ack: false,
            irq_cycle_mode: false,
            irq_pending: false,
            audio: Vrc7Audio::new(),
            wram_enable: false,
        }
    }

    fn prg_8k_count(&self) -> usize {
        (self.prg_rom.len() / 0x2000).max(1)
    }

    fn chr_1k_count(&self) -> usize {
        (self.chr.len() / 0x0400).max(1)
    }

    fn read_prg_8k(&self, bank: usize, offset: usize) -> u8 {
        let b = bank % self.prg_8k_count();
        self.prg_rom[b * 0x2000 + (offset & 0x1FFF)]
    }

    fn read_chr_1k(&self, bank: usize, offset: usize) -> u8 {
        let b = bank % self.chr_1k_count();
        self.chr[(b * 0x0400 + (offset & 0x03FF)) % self.chr.len()]
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

impl Mapper for Vrc7 {
    fn cpu_peek(&self, addr: u16) -> u8 {
        match addr {
            0x6000..=0x7FFF if self.wram_enable && !self.prg_ram.is_empty() => {
                self.prg_ram[(addr as usize - 0x6000) % self.prg_ram.len()]
            }
            0x8000..=0x9FFF => self.read_prg_8k(self.prg_banks[0] as usize, addr as usize),
            0xA000..=0xBFFF => self.read_prg_8k(self.prg_banks[1] as usize, addr as usize),
            0xC000..=0xDFFF => self.read_prg_8k(self.prg_banks[2] as usize, addr as usize),
            0xE000..=0xFFFF => self.read_prg_8k(self.prg_8k_count() - 1, addr as usize),
            _ => 0,
        }
    }

    fn cpu_write(&mut self, addr: u16, val: u8) {
        match addr {
            0x6000..=0x7FFF if self.wram_enable && !self.prg_ram.is_empty() => {
                let idx = (addr as usize - 0x6000) % self.prg_ram.len();
                self.prg_ram[idx] = val;
            }
            0x8000 => self.prg_banks[0] = val & 0x3F,
            0x8010 => self.prg_banks[1] = val & 0x3F,
            0x9000 => self.prg_banks[2] = val & 0x3F,
            0x9010 => self.audio.write_addr(val),
            0x9030 => self.audio.write_data(val),
            0xA000 => self.chr_banks[0] = val,
            0xA010 => self.chr_banks[1] = val,
            0xB000 => self.chr_banks[2] = val,
            0xB010 => self.chr_banks[3] = val,
            0xC000 => self.chr_banks[4] = val,
            0xC010 => self.chr_banks[5] = val,
            0xD000 => self.chr_banks[6] = val,
            0xD010 => self.chr_banks[7] = val,
            0xE000 => {
                self.mirroring = match val & 0x03 {
                    0 => Mirroring::Vertical,
                    1 => Mirroring::Horizontal,
                    2 => Mirroring::SingleScreenLower,
                    3 => Mirroring::SingleScreenUpper,
                    _ => unreachable!(),
                };
                self.wram_enable = val & 0x80 != 0;
            }
            0xE010 => self.irq_latch = val,
            0xF000 => {
                self.irq_pending = false;
                self.irq_enabled_after_ack = val & 0x01 != 0;
                self.irq_enabled = val & 0x02 != 0;
                self.irq_cycle_mode = val & 0x04 != 0;
                if self.irq_enabled {
                    self.irq_counter = self.irq_latch;
                    self.irq_prescaler = 341;
                }
            }
            0xF010 => {
                self.irq_pending = false;
                self.irq_enabled = self.irq_enabled_after_ack;
            }
            _ => {}
        }
    }

    fn chr_read(&mut self, addr: u16) -> u8 {
        let slot = ((addr as usize) >> 10) & 0x07;
        self.read_chr_1k(self.chr_banks[slot] as usize, addr as usize)
    }

    fn chr_write(&mut self, addr: u16, val: u8) {
        let slot = ((addr as usize) >> 10) & 0x07;
        let bank = (self.chr_banks[slot] as usize) % self.chr_1k_count();
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
        self.audio.tick();

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

    fn audio_output(&self) -> f32 {
        self.audio.output()
    }

    fn dump_battery_data(&self) -> Option<Vec<u8>> {
        if self.has_battery {
            Some(self.prg_ram.clone())
        } else {
            None
        }
    }

    fn load_battery_data(&mut self, bytes: &[u8]) -> anyhow::Result<()> {
        if bytes.len() <= self.prg_ram.len() {
            self.prg_ram[..bytes.len()].copy_from_slice(bytes);
        }
        Ok(())
    }

    fn write_state(&self, w: &mut crate::save_state::StateWriter) {
        w.write_u8(crate::save_state::encode_mirroring(self.mirroring));
        w.write_bool(self.has_battery);
        w.write_bool(self.wram_enable);
        w.write_bytes(&self.prg_banks);
        w.write_bytes(&self.chr_banks);
        w.write_u8(self.irq_latch);
        w.write_u8(self.irq_counter);
        w.write_u32(self.irq_prescaler as u32);
        w.write_bool(self.irq_enabled);
        w.write_bool(self.irq_enabled_after_ack);
        w.write_bool(self.irq_cycle_mode);
        w.write_bool(self.irq_pending);
        w.write_vec(&self.prg_ram);
        self.audio.write_state(w);
        crate::save_state::write_chr_state(w, &self.chr);
    }

    fn read_state(&mut self, r: &mut crate::save_state::StateReader) -> anyhow::Result<()> {
        self.mirroring = crate::save_state::decode_mirroring(r.read_u8()?)?;
        self.has_battery = r.read_bool()?;
        self.wram_enable = r.read_bool()?;
        r.read_exact(&mut self.prg_banks)?;
        r.read_exact(&mut self.chr_banks)?;
        self.irq_latch = r.read_u8()?;
        self.irq_counter = r.read_u8()?;
        self.irq_prescaler = r.read_u32()? as i32;
        self.irq_enabled = r.read_bool()?;
        self.irq_enabled_after_ack = r.read_bool()?;
        self.irq_cycle_mode = r.read_bool()?;
        self.irq_pending = r.read_bool()?;
        let ram = r.read_vec(256 * 1024)?;
        if ram.len() != self.prg_ram.len() {
            anyhow::bail!(
                "VRC7 PRG-RAM size mismatch: expected {}, got {}",
                self.prg_ram.len(),
                ram.len()
            );
        }
        self.prg_ram = ram;
        self.audio.read_state(r)?;
        crate::save_state::read_chr_state(r, &mut self.chr, "VRC7")?;
        Ok(())
    }
}

#[cfg(test)]
mod tests;
