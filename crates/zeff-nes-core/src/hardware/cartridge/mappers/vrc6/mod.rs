pub(crate) mod audio;

use crate::hardware::cartridge::{Mapper, Mirroring};
use audio::Vrc6Audio;

pub struct Vrc6 {
    prg_rom: Vec<u8>,
    chr: Vec<u8>,
    prg_ram: [u8; 0x2000],
    mirroring: Mirroring,
    a0_a1_swap: bool,

    prg_bank_16k: u8,
    prg_bank_8k: u8,

    chr_banks: [u8; 8],

    irq_latch: u8,
    irq_counter: u8,
    irq_prescaler: i32,
    irq_enabled: bool,
    irq_enabled_after_ack: bool,
    irq_cycle_mode: bool,
    irq_pending: bool,

    audio: Vrc6Audio,
}

impl Vrc6 {
    pub fn new(
        prg_rom: Vec<u8>,
        chr: Vec<u8>,
        mirroring: Mirroring,
        a0_a1_swap: bool,
    ) -> Self {
        Self {
            prg_rom,
            chr,
            prg_ram: [0; 0x2000],
            mirroring,
            a0_a1_swap,
            prg_bank_16k: 0,
            prg_bank_8k: 0,
            chr_banks: [0; 8],
            irq_latch: 0,
            irq_counter: 0,
            irq_prescaler: 341,
            irq_enabled: false,
            irq_enabled_after_ack: false,
            irq_cycle_mode: false,
            irq_pending: false,
            audio: Vrc6Audio::new(),
        }
    }

    fn prg_bank_count_16k(&self) -> usize {
        (self.prg_rom.len() / 0x4000).max(1)
    }

    fn prg_bank_count_8k(&self) -> usize {
        (self.prg_rom.len() / 0x2000).max(1)
    }

    fn chr_bank_count_1k(&self) -> usize {
        (self.chr.len() / 0x0400).max(1)
    }
    
    fn decode_sub_reg(&self, addr: u16) -> u8 {
        let r = (addr & 0x03) as u8;
        if self.a0_a1_swap {
            match r {
                1 => 2,
                2 => 1,
                other => other,
            }
        } else {
            r
        }
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

impl Mapper for Vrc6 {
    fn cpu_peek(&self, addr: u16) -> u8 {
        match addr {
            0x6000..=0x7FFF => self.prg_ram[(addr - 0x6000) as usize],
            0x8000..=0xBFFF => {
                let bank = (self.prg_bank_16k as usize) % self.prg_bank_count_16k();
                let offset = (addr as usize) & 0x3FFF;
                self.prg_rom[bank * 0x4000 + offset]
            }
            0xC000..=0xDFFF => {
                let bank = (self.prg_bank_8k as usize) % self.prg_bank_count_8k();
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
        let sub = self.decode_sub_reg(addr);

        match addr & 0xF000 {
            0x6000 | 0x7000 => {
                self.prg_ram[(addr - 0x6000) as usize] = val;
            }
            0x8000 => {
                self.prg_bank_16k = val & 0x0F;
            }
            0x9000 => match sub {
                0 => self.audio.pulse1.write_control(val),
                1 => self.audio.pulse1.write_period_low(val),
                2 => self.audio.pulse1.write_period_high(val),
                3 => self.audio.write_freq_control(val),
                _ => {}
            },
            0xA000 => match sub {
                0 => self.audio.pulse2.write_control(val),
                1 => self.audio.pulse2.write_period_low(val),
                2 => self.audio.pulse2.write_period_high(val),
                _ => {}
            },
            0xB000 => match sub {
                0 => self.audio.sawtooth.write_rate(val),
                1 => self.audio.sawtooth.write_period_low(val),
                2 => self.audio.sawtooth.write_period_high(val),
                3 => {
                    self.mirroring = match (val >> 2) & 0x03 {
                        0 => Mirroring::Vertical,
                        1 => Mirroring::Horizontal,
                        2 => Mirroring::SingleScreenLower,
                        3 => Mirroring::SingleScreenUpper,
                        _ => unreachable!(),
                    };
                }
                _ => {}
            },
            0xC000 => {
                self.prg_bank_8k = val & 0x1F;
            }
            0xD000 => {
                if (sub as usize) < 4 {
                    self.chr_banks[sub as usize] = val;
                }
            }
            0xE000 => {
                if (sub as usize) < 4 {
                    self.chr_banks[4 + sub as usize] = val;
                }
            }
            0xF000 => match sub {
                0 => {
                    self.irq_latch = val;
                }
                1 => {
                    self.irq_pending = false;
                    self.irq_enabled_after_ack = val & 0x01 != 0;
                    self.irq_enabled = val & 0x02 != 0;
                    self.irq_cycle_mode = val & 0x04 != 0;
                    if self.irq_enabled {
                        self.irq_counter = self.irq_latch;
                        self.irq_prescaler = 341;
                    }
                }
                2 => {
                    self.irq_pending = false;
                    self.irq_enabled = self.irq_enabled_after_ack;
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

    fn write_state(&self, w: &mut crate::save_state::StateWriter) {
        w.write_bytes(&self.prg_ram);
        w.write_u8(crate::save_state::encode_mirroring(self.mirroring));
        w.write_bool(self.a0_a1_swap);

        w.write_u8(self.prg_bank_16k);
        w.write_u8(self.prg_bank_8k);
        w.write_bytes(&self.chr_banks);

        w.write_u8(self.irq_latch);
        w.write_u8(self.irq_counter);
        w.write_u32(self.irq_prescaler as u32);
        w.write_bool(self.irq_enabled);
        w.write_bool(self.irq_enabled_after_ack);
        w.write_bool(self.irq_cycle_mode);
        w.write_bool(self.irq_pending);

        self.audio.write_state(w);
        w.write_vec(&self.chr);
    }

    fn read_state(&mut self, r: &mut crate::save_state::StateReader) -> anyhow::Result<()> {
        r.read_exact(&mut self.prg_ram)?;
        self.mirroring = crate::save_state::decode_mirroring(r.read_u8()?)?;
        self.a0_a1_swap = r.read_bool()?;

        self.prg_bank_16k = r.read_u8()?;
        self.prg_bank_8k = r.read_u8()?;
        r.read_exact(&mut self.chr_banks)?;

        self.irq_latch = r.read_u8()?;
        self.irq_counter = r.read_u8()?;
        self.irq_prescaler = r.read_u32()? as i32;
        self.irq_enabled = r.read_bool()?;
        self.irq_enabled_after_ack = r.read_bool()?;
        self.irq_cycle_mode = r.read_bool()?;
        self.irq_pending = r.read_bool()?;

        self.audio.read_state(r)?;

        let chr = r.read_vec(512 * 1024)?;
        if chr.len() != self.chr.len() {
            anyhow::bail!(
                "VRC6 CHR size mismatch: expected {}, got {}",
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

