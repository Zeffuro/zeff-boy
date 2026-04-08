use crate::save_state::{StateReader, StateWriter};

const SOUND_RAM_SIZE: usize = 128;

pub struct N163Audio {
    pub ram: [u8; SOUND_RAM_SIZE],
    addr: u8,
    auto_increment: bool,
    channel_counter: u8,
    prescaler: u8,
    output: f32,
}

impl N163Audio {
    pub fn new() -> Self {
        Self {
            ram: [0; SOUND_RAM_SIZE],
            addr: 0,
            auto_increment: false,
            channel_counter: 0,
            prescaler: 0,
            output: 0.0,
        }
    }

    pub fn write_addr(&mut self, val: u8) {
        self.addr = val & 0x7F;
        self.auto_increment = val & 0x80 != 0;
    }

    pub fn read_data(&mut self) -> u8 {
        let val = self.ram[self.addr as usize];
        if self.auto_increment {
            self.addr = (self.addr + 1) & 0x7F;
        }
        val
    }

    pub fn write_data(&mut self, val: u8) {
        self.ram[self.addr as usize] = val;
        if self.auto_increment {
            self.addr = (self.addr + 1) & 0x7F;
        }
    }

    fn num_channels(&self) -> u8 {
        ((self.ram[0x7F] >> 4) & 0x07) + 1
    }

    fn channel_base(channel: u8) -> usize {
        0x40 + (channel as usize) * 8
    }

    fn read_channel_freq(&self, ch: u8) -> u32 {
        let base = Self::channel_base(ch);
        let lo = self.ram[base] as u32;
        let mid = self.ram[base + 2] as u32;
        let hi = (self.ram[base + 4] & 0x03) as u32;
        lo | (mid << 8) | (hi << 16)
    }

    fn read_channel_phase(&self, ch: u8) -> u32 {
        let base = Self::channel_base(ch);
        let lo = self.ram[base + 1] as u32;
        let mid = self.ram[base + 3] as u32;
        let hi = self.ram[base + 5] as u32;
        lo | (mid << 8) | (hi << 16)
    }

    fn write_channel_phase(&mut self, ch: u8, phase: u32) {
        let base = Self::channel_base(ch);
        self.ram[base + 1] = phase as u8;
        self.ram[base + 3] = (phase >> 8) as u8;
        self.ram[base + 5] = (phase >> 16) as u8;
    }

    fn channel_wave_length(&self, ch: u8) -> u32 {
        let base = Self::channel_base(ch);
        let l = ((self.ram[base + 4] >> 2) & 0x3F) as u32;
        256 - (l << 2)
    }

    fn channel_wave_addr(&self, ch: u8) -> u8 {
        let base = Self::channel_base(ch);
        self.ram[base + 6]
    }

    fn channel_volume(&self, ch: u8) -> u8 {
        let base = Self::channel_base(ch);
        self.ram[base + 7] & 0x0F
    }

    fn read_4bit_sample(&self, addr_4bit: u8) -> u8 {
        let byte_addr = (addr_4bit >> 1) as usize & 0x7F;
        let byte = self.ram[byte_addr];
        if addr_4bit & 1 == 0 {
            byte & 0x0F
        } else {
            (byte >> 4) & 0x0F
        }
    }

    fn update_channel(&mut self, ch: u8) -> f32 {
        let freq = self.read_channel_freq(ch);
        let mut phase = self.read_channel_phase(ch);

        phase = (phase + freq) & 0x00FF_FFFF;

        let wave_len = self.channel_wave_length(ch);
        if wave_len > 0 {
            let sample_idx = (phase >> 16) % wave_len;
            let wave_addr = self.channel_wave_addr(ch);
            let sample_pos = wave_addr.wrapping_add(sample_idx as u8);
            let sample = self.read_4bit_sample(sample_pos);
            let volume = self.channel_volume(ch);

            self.write_channel_phase(ch, phase);
            (sample as f32) * (volume as f32)
        } else {
            self.write_channel_phase(ch, phase);
            0.0
        }
    }

    pub fn tick(&mut self) {
        self.prescaler += 1;
        if self.prescaler < 15 {
            return;
        }
        self.prescaler = 0;

        let n = self.num_channels();
        let ch = 8 - n + self.channel_counter;
        let sample = self.update_channel(ch);

        self.output = (self.output + sample) * 0.5;

        self.channel_counter += 1;
        if self.channel_counter >= n {
            self.channel_counter = 0;
        }
    }

    pub fn output(&self) -> f32 {
        self.output / 225.0 * 0.5
    }

    pub fn write_state(&self, w: &mut StateWriter) {
        w.write_bytes(&self.ram);
        w.write_u8(self.addr);
        w.write_bool(self.auto_increment);
        w.write_u8(self.channel_counter);
        w.write_u8(self.prescaler);
    }

    pub fn read_state(&mut self, r: &mut StateReader) -> anyhow::Result<()> {
        r.read_exact(&mut self.ram)?;
        self.addr = r.read_u8()? & 0x7F;
        self.auto_increment = r.read_bool()?;
        self.channel_counter = r.read_u8()?;
        self.prescaler = r.read_u8()?;
        self.output = 0.0;
        Ok(())
    }
}
