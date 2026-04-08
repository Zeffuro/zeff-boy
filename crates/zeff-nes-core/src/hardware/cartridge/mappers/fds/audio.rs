use crate::save_state::{StateReader, StateWriter};

const WAVE_TABLE_SIZE: usize = 64;
const MOD_TABLE_SIZE: usize = 32;

static MASTER_VOL_MUL: [u32; 4] = [0, 2, 2, 2];
static MASTER_VOL_DIV: [u32; 4] = [1, 3, 4, 5];
static MOD_ADJUSTMENTS: [i32; 8] = [0, 1, 2, 4, 0, -4, -2, -1];

pub struct FdsAudio {
    wave_table: [u8; WAVE_TABLE_SIZE],
    mod_table: [u8; MOD_TABLE_SIZE],

    wave_write_enable: bool,
    master_volume: u8,

    vol_env_speed: u8,
    vol_env_direction: bool,
    vol_env_disable: bool,
    vol_gain: u8,

    freq: u16,
    halt: bool,
    env_disable: bool,

    mod_env_speed: u8,
    mod_env_direction: bool,
    mod_env_disable: bool,
    mod_gain: u8,

    mod_freq: u16,
    mod_halt: bool,
    mod_counter: i8,
    mod_table_pos: u8,

    env_speed_mul: u8,

    wave_phase: u32,
    mod_phase: u32,
    wave_pos: u8,

    env_timer: u16,
    mod_output: i32,

    output: f32,
}

impl FdsAudio {
    pub fn new() -> Self {
        Self {
            wave_table: [0; WAVE_TABLE_SIZE],
            mod_table: [0; MOD_TABLE_SIZE],
            wave_write_enable: false,
            master_volume: 0,
            vol_env_speed: 0,
            vol_env_direction: false,
            vol_env_disable: true,
            vol_gain: 0,
            freq: 0,
            halt: true,
            env_disable: true,
            mod_env_speed: 0,
            mod_env_direction: false,
            mod_env_disable: true,
            mod_gain: 0,
            mod_freq: 0,
            mod_halt: true,
            mod_counter: 0,
            mod_table_pos: 0,
            env_speed_mul: 0xFF,
            wave_phase: 0,
            mod_phase: 0,
            wave_pos: 0,
            env_timer: 0,
            mod_output: 0,
            output: 0.0,
        }
    }

    pub fn read(&self, addr: u16) -> u8 {
        match addr {
            0x4040..=0x407F => {
                let idx = (addr - 0x4040) as usize;
                self.wave_table[idx]
            }
            0x4090 => self.vol_gain | 0x40,
            0x4092 => self.mod_gain | 0x40,
            _ => 0,
        }
    }

    pub fn write(&mut self, addr: u16, val: u8) {
        match addr {
            0x4040..=0x407F => {
                if self.wave_write_enable {
                    self.wave_table[(addr - 0x4040) as usize] = val & 0x3F;
                }
            }
            0x4080 => {
                self.vol_env_disable = val & 0x80 != 0;
                self.vol_env_direction = val & 0x40 != 0;
                self.vol_env_speed = val & 0x3F;
                if self.vol_env_disable {
                    self.vol_gain = self.vol_env_speed;
                }
            }
            0x4082 => {
                self.freq = (self.freq & 0xF00) | val as u16;
            }
            0x4083 => {
                self.freq = (self.freq & 0x0FF) | ((val as u16 & 0x0F) << 8);
                self.halt = val & 0x80 != 0;
                self.env_disable = val & 0x40 != 0;
                if self.halt {
                    self.wave_phase = 0;
                    self.wave_pos = 0;
                }
                if self.env_disable {
                    self.env_timer = 0;
                }
            }
            0x4084 => {
                self.mod_env_disable = val & 0x80 != 0;
                self.mod_env_direction = val & 0x40 != 0;
                self.mod_env_speed = val & 0x3F;
                if self.mod_env_disable {
                    self.mod_gain = self.mod_env_speed;
                }
            }
            0x4085 => {
                self.mod_counter = ((val & 0x7F) as i8) | if val & 0x40 != 0 { -64 } else { 0 };
            }
            0x4086 => {
                self.mod_freq = (self.mod_freq & 0xF00) | val as u16;
            }
            0x4087 => {
                self.mod_freq = (self.mod_freq & 0x0FF) | ((val as u16 & 0x0F) << 8);
                self.mod_halt = val & 0x80 != 0;
                if self.mod_halt {
                    self.mod_phase = 0;
                }
            }
            0x4088 => {
                if self.mod_halt {
                    let idx = (self.mod_table_pos & 0x3F) as usize % MOD_TABLE_SIZE;
                    self.mod_table[idx] = val & 0x07;
                    self.mod_table_pos = self.mod_table_pos.wrapping_add(1) & 0x3F;
                }
            }
            0x4089 => {
                self.wave_write_enable = val & 0x80 != 0;
                self.master_volume = val & 0x03;
            }
            0x408A => {
                self.env_speed_mul = val;
            }
            _ => {}
        }
    }

    pub fn tick(&mut self) {
        self.clock_modulator();
        self.clock_main_wave();
        self.clock_envelopes();
    }

    fn clock_modulator(&mut self) {
        if self.mod_halt || self.mod_freq == 0 {
            return;
        }
        self.mod_phase = self.mod_phase.wrapping_add(self.mod_freq as u32);
        if self.mod_phase >= 0x10000 {
            self.mod_phase -= 0x10000;
            let idx = (self.mod_table_pos & 0x1F) as usize;
            let adj = MOD_ADJUSTMENTS[self.mod_table[idx] as usize & 0x07];
            if adj == 0 && self.mod_table[idx] == 4 {
                self.mod_counter = 0;
            } else {
                let new_counter = self.mod_counter as i32 + adj;
                self.mod_counter = new_counter.clamp(-64, 63) as i8;
            }
            self.mod_table_pos = (self.mod_table_pos + 1) & 0x3F;
        }

        let gain = self.mod_gain as i32;
        let counter = self.mod_counter as i32;
        let mut temp = counter * gain;
        let remainder = temp & 0x0F;
        temp >>= 4;
        if remainder > 0 && (temp & 0x80) == 0 {
            temp += if counter < 0 { -1 } else { 2 };
        }
        if temp >= 192 {
            temp -= 256;
        } else if temp < -64 {
            temp += 256;
        }
        self.mod_output = self.freq as i32 * temp / 64;
    }

    fn clock_main_wave(&mut self) {
        if self.halt || self.freq == 0 || self.wave_write_enable {
            return;
        }
        let effective_freq = (self.freq as i32 + self.mod_output).clamp(0, 0xFFF) as u32;
        self.wave_phase = self.wave_phase.wrapping_add(effective_freq);
        if self.wave_phase >= 0x10000 {
            self.wave_phase -= 0x10000;
            self.wave_pos = (self.wave_pos + 1) & 0x3F;
        }

        let sample = self.wave_table[self.wave_pos as usize] as u32;
        let vol = (self.vol_gain as u32).min(32);
        let raw = sample * vol;

        let mul = MASTER_VOL_MUL[self.master_volume as usize];
        let div = MASTER_VOL_DIV[self.master_volume as usize];
        let scaled = if self.master_volume == 0 {
            raw
        } else {
            raw * mul / div
        };

        self.output = scaled as f32 / (63.0 * 32.0) * 0.5;
    }

    fn clock_envelopes(&mut self) {
        if self.env_disable || self.env_speed_mul == 0 || self.halt {
            return;
        }
        self.env_timer = self.env_timer.wrapping_add(1);

        if !self.vol_env_disable {
            let vol_period =
                8u16.saturating_mul((self.vol_env_speed as u16 + 1) * self.env_speed_mul as u16);
            if vol_period > 0 && self.env_timer.is_multiple_of(vol_period) {
                if self.vol_env_direction {
                    if self.vol_gain < 32 {
                        self.vol_gain += 1;
                    }
                } else if self.vol_gain > 0 {
                    self.vol_gain -= 1;
                }
            }
        }

        if !self.mod_env_disable {
            let mod_period =
                8u16.saturating_mul((self.mod_env_speed as u16 + 1) * self.env_speed_mul as u16);
            if mod_period > 0 && self.env_timer.is_multiple_of(mod_period) {
                if self.mod_env_direction {
                    if self.mod_gain < 32 {
                        self.mod_gain += 1;
                    }
                } else if self.mod_gain > 0 {
                    self.mod_gain -= 1;
                }
            }
        }
    }

    pub fn output(&self) -> f32 {
        self.output
    }

    pub fn write_state(&self, w: &mut StateWriter) {
        w.write_bytes(&self.wave_table);
        w.write_bytes(&self.mod_table);
        w.write_bool(self.wave_write_enable);
        w.write_u8(self.master_volume);
        w.write_u8(self.vol_env_speed);
        w.write_bool(self.vol_env_direction);
        w.write_bool(self.vol_env_disable);
        w.write_u8(self.vol_gain);
        w.write_u16(self.freq);
        w.write_bool(self.halt);
        w.write_bool(self.env_disable);
        w.write_u8(self.mod_env_speed);
        w.write_bool(self.mod_env_direction);
        w.write_bool(self.mod_env_disable);
        w.write_u8(self.mod_gain);
        w.write_u16(self.mod_freq);
        w.write_bool(self.mod_halt);
        w.write_u8(self.mod_counter as u8);
        w.write_u8(self.mod_table_pos);
        w.write_u8(self.env_speed_mul);
        w.write_u32(self.wave_phase);
        w.write_u32(self.mod_phase);
        w.write_u8(self.wave_pos);
        w.write_u16(self.env_timer);
        w.write_u32(self.mod_output as u32);
    }

    pub fn read_state(&mut self, r: &mut StateReader) -> anyhow::Result<()> {
        r.read_exact(&mut self.wave_table)?;
        r.read_exact(&mut self.mod_table)?;
        self.wave_write_enable = r.read_bool()?;
        self.master_volume = r.read_u8()?;
        self.vol_env_speed = r.read_u8()?;
        self.vol_env_direction = r.read_bool()?;
        self.vol_env_disable = r.read_bool()?;
        self.vol_gain = r.read_u8()?;
        self.freq = r.read_u16()?;
        self.halt = r.read_bool()?;
        self.env_disable = r.read_bool()?;
        self.mod_env_speed = r.read_u8()?;
        self.mod_env_direction = r.read_bool()?;
        self.mod_env_disable = r.read_bool()?;
        self.mod_gain = r.read_u8()?;
        self.mod_freq = r.read_u16()?;
        self.mod_halt = r.read_bool()?;
        self.mod_counter = r.read_u8()? as i8;
        self.mod_table_pos = r.read_u8()?;
        self.env_speed_mul = r.read_u8()?;
        self.wave_phase = r.read_u32()?;
        self.mod_phase = r.read_u32()?;
        self.wave_pos = r.read_u8()?;
        self.env_timer = r.read_u16()?;
        self.mod_output = r.read_u32()? as i32;
        self.output = 0.0;
        Ok(())
    }
}
