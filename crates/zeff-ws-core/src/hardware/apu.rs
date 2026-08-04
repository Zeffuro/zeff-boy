use super::constants::CPU_CLOCK_HZ;

const CHANNEL_COUNT: usize = 4;
const DEFAULT_SAMPLE_RATE: u32 = 48_000;
const HYPER_VOICE_CONTROL_PORT: u16 = 0x006A;
const HYPER_VOICE_CHANNEL_CONTROL_PORT: u16 = 0x006B;
const PERIOD_PORT_START: u16 = 0x0080;
const PERIOD_PORT_END: u16 = 0x0087;
const VOLUME_PORT_START: u16 = 0x0088;
const VOLUME_PORT_END: u16 = 0x008B;
const SWEEP_VALUE_PORT: u16 = 0x008C;
const SWEEP_STEP_PORT: u16 = 0x008D;
const NOISE_CONTROL_PORT: u16 = 0x008E;
const SAMPLE_RAM_POS_PORT: u16 = 0x008F;
const CONTROL_PORT: u16 = 0x0090;
const OUTPUT_CONTROL_PORT: u16 = 0x0091;
const NOISE_LFSR_LO_PORT: u16 = 0x0092;
const NOISE_LFSR_HI_PORT: u16 = 0x0093;
const VOICE_VOLUME_PORT: u16 = 0x0094;
const SOUND_TEST_PORT: u16 = 0x0095;
const CHANNEL_OUTPUT_RIGHT_PORT: u16 = 0x0096;
const CHANNEL_OUTPUT_RIGHT_HI_PORT: u16 = 0x0097;
const CHANNEL_OUTPUT_LEFT_PORT: u16 = 0x0098;
const CHANNEL_OUTPUT_LEFT_HI_PORT: u16 = 0x0099;
const CHANNEL_OUTPUT_LEFT_RIGHT_PORT: u16 = 0x009A;
const CHANNEL_OUTPUT_LEFT_RIGHT_HI_PORT: u16 = 0x009B;
const CHANNEL_OUTPUT_END_PORT: u16 = 0x009B;
const SWEEP_CLOCK_PERIOD: i32 = 8192;
const CHANNEL_2_VOICE: u8 = 0x20;
const CHANNEL_3_SWEEP: u8 = 0x40;
const CHANNEL_4_NOISE: u8 = 0x80;
const NOISE_ENABLE: u8 = 0x10;
const SOUND_TEST_FAST_SWEEP: u8 = 0x02;
const SOUND_TEST_READ_MASK: u8 = 0xE3;
const NOISE_TAPS: [u8; 8] = [14, 10, 13, 4, 8, 6, 9, 11];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum OutputSide {
    Left,
    Right,
    LeftRight,
}

#[derive(Clone, Debug)]
pub struct Apu {
    period: [u16; CHANNEL_COUNT],
    volume: [u8; CHANNEL_COUNT],
    voice_volume: u8,
    sweep_step: u8,
    sweep_value: u8,
    noise_control: u8,
    control: u8,
    output_control: u8,
    sample_ram_pos: u8,
    sweep_8192_divider: i32,
    sweep_counter: u8,
    period_counter: [i32; CHANNEL_COUNT],
    sample_pos: [u8; CHANNEL_COUNT],
    nreg: u16,
    hyper_voice_sample: u8,
    sound_test: u8,
    hyper_voice_control: u8,
    hyper_voice_channel_control: u8,
    sample_rate: u32,
    sample_generation_enabled: bool,
    sample_cycle_accumulator: u32,
    sample_buffer: Vec<f32>,
    channel_mutes: [bool; CHANNEL_COUNT],
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ApuDebugSnapshot {
    pub sample_rate: u32,
    pub sample_generation_enabled: bool,
    pub buffered_samples: usize,
    pub period: [u16; CHANNEL_COUNT],
    pub volume: [u8; CHANNEL_COUNT],
    pub voice_volume: u8,
    pub sweep_step: u8,
    pub sweep_value: u8,
    pub noise_control: u8,
    pub control: u8,
    pub output_control: u8,
    pub sample_ram_pos: u8,
    pub sample_pos: [u8; CHANNEL_COUNT],
    pub nreg: u16,
    pub hyper_voice_sample: u8,
    pub sound_test: u8,
    pub hyper_voice_control: u8,
    pub hyper_voice_channel_control: u8,
    pub channel_mutes: [bool; CHANNEL_COUNT],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ApuSaveState {
    pub(crate) period: [u16; CHANNEL_COUNT],
    pub(crate) volume: [u8; CHANNEL_COUNT],
    pub(crate) voice_volume: u8,
    pub(crate) sweep_step: u8,
    pub(crate) sweep_value: u8,
    pub(crate) noise_control: u8,
    pub(crate) control: u8,
    pub(crate) output_control: u8,
    pub(crate) sample_ram_pos: u8,
    pub(crate) sweep_8192_divider: i32,
    pub(crate) sweep_counter: u8,
    pub(crate) period_counter: [i32; CHANNEL_COUNT],
    pub(crate) sample_pos: [u8; CHANNEL_COUNT],
    pub(crate) nreg: u16,
    pub(crate) hyper_voice_sample: u8,
    pub(crate) sound_test: u8,
    pub(crate) hyper_voice_control: u8,
    pub(crate) hyper_voice_channel_control: u8,
    pub(crate) sample_cycle_accumulator: u32,
    pub(crate) channel_mutes: [bool; CHANNEL_COUNT],
}

impl Apu {
    pub fn new(sample_rate: u32) -> Self {
        let mut apu = Self {
            period: [0; CHANNEL_COUNT],
            volume: [0; CHANNEL_COUNT],
            voice_volume: 0,
            sweep_step: 0,
            sweep_value: 0,
            noise_control: 0,
            control: 0,
            output_control: 0,
            sample_ram_pos: 0,
            sweep_8192_divider: SWEEP_CLOCK_PERIOD,
            sweep_counter: 0,
            period_counter: [1; CHANNEL_COUNT],
            sample_pos: [0; CHANNEL_COUNT],
            nreg: 0,
            hyper_voice_sample: 0,
            sound_test: 0,
            hyper_voice_control: 0,
            hyper_voice_channel_control: 0,
            sample_rate: sample_rate.max(1),
            sample_generation_enabled: true,
            sample_cycle_accumulator: 0,
            sample_buffer: Vec::new(),
            channel_mutes: [false; CHANNEL_COUNT],
        };
        if sample_rate == 0 {
            apu.sample_rate = DEFAULT_SAMPLE_RATE;
        }
        apu
    }

    pub fn reset(&mut self) {
        self.period = [0; CHANNEL_COUNT];
        self.volume = [0; CHANNEL_COUNT];
        self.voice_volume = 0;
        self.sweep_step = 0;
        self.sweep_value = 0;
        self.noise_control = 0;
        self.control = 0;
        self.output_control = 0;
        self.sample_ram_pos = 0;
        self.sweep_8192_divider = SWEEP_CLOCK_PERIOD;
        self.sweep_counter = 0;
        self.period_counter = [1; CHANNEL_COUNT];
        self.sample_pos = [0; CHANNEL_COUNT];
        self.nreg = 0;
        self.hyper_voice_sample = 0;
        self.sound_test = 0;
        self.hyper_voice_control = 0;
        self.hyper_voice_channel_control = 0;
        self.sample_cycle_accumulator = 0;
        self.sample_buffer.clear();
    }

    pub fn handles_port(port: u16) -> bool {
        matches!(
            port,
            HYPER_VOICE_CONTROL_PORT | HYPER_VOICE_CHANNEL_CONTROL_PORT | PERIOD_PORT_START
                ..=CHANNEL_OUTPUT_END_PORT
        )
    }

    pub fn read8(&self, port: u16) -> u8 {
        match port {
            PERIOD_PORT_START..=PERIOD_PORT_END => {
                let channel = usize::from((port - PERIOD_PORT_START) >> 1);
                let period = self.read_period(channel);
                if port & 1 == 0 {
                    period as u8
                } else {
                    (period >> 8) as u8
                }
            }
            VOLUME_PORT_START..=VOLUME_PORT_END => {
                self.volume[usize::from(port - VOLUME_PORT_START)]
            }
            SWEEP_VALUE_PORT => self.sweep_value,
            SWEEP_STEP_PORT => self.sweep_step,
            NOISE_CONTROL_PORT => self.noise_control,
            SAMPLE_RAM_POS_PORT => self.sample_ram_pos,
            CONTROL_PORT => self.control,
            OUTPUT_CONTROL_PORT => self.output_control | 0x80,
            NOISE_LFSR_LO_PORT => self.nreg as u8,
            NOISE_LFSR_HI_PORT => (self.nreg >> 8) as u8,
            VOICE_VOLUME_PORT => self.voice_volume,
            SOUND_TEST_PORT => self.sound_test,
            CHANNEL_OUTPUT_RIGHT_PORT | CHANNEL_OUTPUT_RIGHT_HI_PORT => self
                .channel_output_word(OutputSide::Right)
                .to_le_bytes()[usize::from(port - CHANNEL_OUTPUT_RIGHT_PORT)],
            CHANNEL_OUTPUT_LEFT_PORT | CHANNEL_OUTPUT_LEFT_HI_PORT => self
                .channel_output_word(OutputSide::Left)
                .to_le_bytes()[usize::from(port - CHANNEL_OUTPUT_LEFT_PORT)],
            CHANNEL_OUTPUT_LEFT_RIGHT_PORT | CHANNEL_OUTPUT_LEFT_RIGHT_HI_PORT => self
                .channel_output_word(OutputSide::LeftRight)
                .to_le_bytes()[usize::from(port - CHANNEL_OUTPUT_LEFT_RIGHT_PORT)],
            HYPER_VOICE_CONTROL_PORT => self.hyper_voice_control,
            HYPER_VOICE_CHANNEL_CONTROL_PORT => self.hyper_voice_channel_control,
            _ => 0,
        }
    }

    pub fn write8(&mut self, port: u16, value: u8) {
        match port {
            PERIOD_PORT_START..=PERIOD_PORT_END => {
                let channel = usize::from((port - PERIOD_PORT_START) >> 1);
                if port & 1 == 0 {
                    self.period[channel] = (self.period[channel] & 0x0700) | u16::from(value);
                } else {
                    self.period[channel] =
                        (self.period[channel] & 0x00FF) | (u16::from(value & 0x07) << 8);
                }
            }
            VOLUME_PORT_START..=VOLUME_PORT_END => {
                self.volume[usize::from(port - VOLUME_PORT_START)] = value;
            }
            SWEEP_VALUE_PORT => self.sweep_value = value,
            SWEEP_STEP_PORT => {
                self.sweep_step = value;
                self.sweep_counter = self.sweep_step.wrapping_add(1);
                self.sweep_8192_divider = SWEEP_CLOCK_PERIOD;
            }
            NOISE_CONTROL_PORT => {
                if value & 0x08 != 0 {
                    self.nreg = 0;
                }
                self.noise_control = value & 0x17;
            }
            SAMPLE_RAM_POS_PORT => self.sample_ram_pos = value,
            CONTROL_PORT => self.control = value,
            OUTPUT_CONTROL_PORT => self.output_control = value & 0x0F,
            NOISE_LFSR_LO_PORT => self.nreg = (self.nreg & 0x7F00) | u16::from(value),
            NOISE_LFSR_HI_PORT => {
                self.nreg = (self.nreg & 0x00FF) | (u16::from(value & 0x7F) << 8);
            }
            VOICE_VOLUME_PORT => self.voice_volume = value & 0x0F,
            SOUND_TEST_PORT => self.sound_test = value & SOUND_TEST_READ_MASK,
            HYPER_VOICE_CONTROL_PORT => self.hyper_voice_control = value,
            HYPER_VOICE_CHANNEL_CONTROL_PORT => self.hyper_voice_channel_control = value & 0x6F,
            _ => {}
        }
    }

    pub(crate) fn write_hyper_voice_sample(&mut self, value: u8) {
        self.hyper_voice_sample = value;
    }

    pub fn step_cycles(&mut self, cycles: u32, ram: &[u8]) {
        let mut remaining = cycles;
        while remaining > 0 {
            let clocks_until_sample = self.clocks_until_next_sample().max(1);
            let chunk = remaining.min(clocks_until_sample);
            self.advance_sound_generators(chunk);
            self.advance_sample_clock(chunk, ram);
            remaining -= chunk;
        }
    }

    pub fn drain_audio_samples_into(&mut self, out: &mut Vec<f32>) {
        out.extend_from_slice(&self.sample_buffer);
        self.sample_buffer.clear();
    }

    pub fn set_sample_rate(&mut self, rate: u32) {
        self.sample_rate = rate.max(1);
        if rate == 0 {
            self.sample_rate = DEFAULT_SAMPLE_RATE;
        }
        self.sample_cycle_accumulator %= CPU_CLOCK_HZ;
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    pub fn set_sample_generation_enabled(&mut self, enabled: bool) {
        self.sample_generation_enabled = enabled;
        if !enabled {
            self.sample_buffer.clear();
        }
    }

    pub fn sample_generation_enabled(&self) -> bool {
        self.sample_generation_enabled
    }

    pub fn set_channel_mutes(&mut self, mutes: [bool; CHANNEL_COUNT]) {
        self.channel_mutes = mutes;
    }

    pub fn channel_mutes(&self) -> [bool; CHANNEL_COUNT] {
        self.channel_mutes
    }

    pub fn debug_snapshot(&self) -> ApuDebugSnapshot {
        ApuDebugSnapshot {
            sample_rate: self.sample_rate,
            sample_generation_enabled: self.sample_generation_enabled,
            buffered_samples: self.sample_buffer.len(),
            period: self.period,
            volume: self.volume,
            voice_volume: self.voice_volume,
            sweep_step: self.sweep_step,
            sweep_value: self.sweep_value,
            noise_control: self.noise_control,
            control: self.control,
            output_control: self.output_control,
            sample_ram_pos: self.sample_ram_pos,
            sample_pos: self.sample_pos,
            nreg: self.nreg,
            hyper_voice_sample: self.hyper_voice_sample,
            sound_test: self.sound_test,
            hyper_voice_control: self.hyper_voice_control,
            hyper_voice_channel_control: self.hyper_voice_channel_control,
            channel_mutes: self.channel_mutes,
        }
    }

    pub(crate) fn save_state(&self) -> ApuSaveState {
        ApuSaveState {
            period: self.period,
            volume: self.volume,
            voice_volume: self.voice_volume,
            sweep_step: self.sweep_step,
            sweep_value: self.sweep_value,
            noise_control: self.noise_control,
            control: self.control,
            output_control: self.output_control,
            sample_ram_pos: self.sample_ram_pos,
            sweep_8192_divider: self.sweep_8192_divider,
            sweep_counter: self.sweep_counter,
            period_counter: self.period_counter,
            sample_pos: self.sample_pos,
            nreg: self.nreg,
            hyper_voice_sample: self.hyper_voice_sample,
            sound_test: self.sound_test,
            hyper_voice_control: self.hyper_voice_control,
            hyper_voice_channel_control: self.hyper_voice_channel_control,
            sample_cycle_accumulator: self.sample_cycle_accumulator,
            channel_mutes: self.channel_mutes,
        }
    }

    pub(crate) fn load_state(&mut self, state: ApuSaveState) {
        self.period = state.period.map(|period| period & 0x07FF);
        self.volume = state.volume;
        self.voice_volume = state.voice_volume & 0x0F;
        self.sweep_step = state.sweep_step;
        self.sweep_value = state.sweep_value;
        self.noise_control = state.noise_control & 0x17;
        self.control = state.control;
        self.output_control = state.output_control & 0x0F;
        self.sample_ram_pos = state.sample_ram_pos;
        self.sweep_8192_divider = state.sweep_8192_divider.max(1);
        self.sweep_counter = state.sweep_counter;
        self.period_counter = state.period_counter.map(|counter| counter.max(1));
        self.sample_pos = state.sample_pos.map(|pos| pos & 0x1F);
        self.nreg = state.nreg & 0x7FFF;
        self.hyper_voice_sample = state.hyper_voice_sample;
        self.sound_test = state.sound_test & SOUND_TEST_READ_MASK;
        self.hyper_voice_control = state.hyper_voice_control;
        self.hyper_voice_channel_control = state.hyper_voice_channel_control & 0x6F;
        self.sample_cycle_accumulator = state.sample_cycle_accumulator % CPU_CLOCK_HZ;
        self.channel_mutes = state.channel_mutes;
        self.sample_buffer.clear();
    }

    fn clocks_until_next_sample(&self) -> u32 {
        let remaining = u64::from(CPU_CLOCK_HZ - self.sample_cycle_accumulator);
        let rate = u64::from(self.sample_rate);
        remaining.div_ceil(rate).min(u64::from(u32::MAX)) as u32
    }

    fn read_period(&self, channel: usize) -> u16 {
        let period = self.period[channel];
        if channel == 2
            && self.sound_test & SOUND_TEST_FAST_SWEEP != 0
            && self.control & CHANNEL_3_SWEEP != 0
        {
            period.wrapping_sub(1) & 0x07FF
        } else {
            period
        }
    }

    fn advance_sample_clock(&mut self, cycles: u32, ram: &[u8]) {
        let mut accumulator = u64::from(self.sample_cycle_accumulator)
            + u64::from(cycles) * u64::from(self.sample_rate);
        while accumulator >= u64::from(CPU_CLOCK_HZ) {
            accumulator -= u64::from(CPU_CLOCK_HZ);
            if self.sample_generation_enabled {
                let (left, right) = self.mix_current_sample(ram);
                self.sample_buffer.push(left);
                self.sample_buffer.push(right);
            }
        }
        self.sample_cycle_accumulator = accumulator as u32;
    }

    fn advance_sound_generators(&mut self, cycles: u32) {
        for channel in 0..CHANNEL_COUNT {
            let channel_enable = 1 << channel;
            if self.control & channel_enable == 0 {
                continue;
            }

            if channel == 2 && self.control & CHANNEL_3_SWEEP != 0 && self.sweep_value != 0 {
                self.advance_sweep_channel(cycles);
            } else if channel == 3
                && self.control & CHANNEL_4_NOISE != 0
                && self.noise_control & NOISE_ENABLE != 0
            {
                self.advance_noise_channel(cycles);
            } else {
                self.advance_wave_channel(channel, cycles);
            }
        }
    }

    fn advance_wave_channel(&mut self, channel: usize, cycles: u32) {
        let Some(period) = self.channel_period_clocks(channel) else {
            return;
        };
        self.period_counter[channel] -= cycles as i32;
        while self.period_counter[channel] <= 0 {
            self.sample_pos[channel] = self.sample_pos[channel].wrapping_add(1) & 0x1F;
            self.period_counter[channel] += period;
        }
    }

    fn advance_sweep_channel(&mut self, cycles: u32) {
        if self.sound_test & SOUND_TEST_FAST_SWEEP != 0 {
            for _ in 0..cycles {
                self.advance_wave_channel(2, 1);
                self.tick_sweep();
            }
            return;
        }

        let mut remaining = cycles;
        while remaining > 0 {
            let divider = self.sweep_8192_divider.max(1) as u32;
            let chunk = remaining.min(divider);
            self.advance_wave_channel(2, chunk);
            self.sweep_8192_divider -= chunk as i32;
            if self.sweep_8192_divider <= 0 {
                self.sweep_8192_divider += SWEEP_CLOCK_PERIOD;
                self.tick_sweep();
            }
            remaining -= chunk;
        }
    }

    fn advance_noise_channel(&mut self, cycles: u32) {
        let period = (2048 - i32::from(self.period[3])).max(1);
        self.period_counter[3] -= cycles as i32;
        while self.period_counter[3] <= 0 {
            self.clock_noise_lfsr();
            self.period_counter[3] += period;
        }
    }

    fn channel_period_clocks(&self, channel: usize) -> Option<i32> {
        let clocks = 2048 - i32::from(self.period[channel]);
        (clocks > 4).then_some(clocks)
    }

    fn tick_sweep(&mut self) {
        self.sweep_counter = self.sweep_counter.wrapping_sub(1);
        if self.sweep_counter == 0 {
            self.sweep_counter = self.sweep_step.wrapping_add(1);
            let delta = i16::from(self.sweep_value as i8);
            self.period[2] = ((self.period[2] as i16 + delta) as u16) & 0x07FF;
        }
    }

    fn clock_noise_lfsr(&mut self) {
        let tap = NOISE_TAPS[usize::from(self.noise_control & 0x07)];
        let feedback = 1 ^ ((self.nreg >> 7) & 1) ^ ((self.nreg >> tap) & 1);
        self.nreg = ((self.nreg << 1) | feedback) & 0x7FFF;
    }

    fn channel_output_word(&self, side: OutputSide) -> u16 {
        let left = self.channel_output_side(OutputSide::Left);
        let right = self.channel_output_side(OutputSide::Right);
        match side {
            OutputSide::Left => u16::from(left),
            OutputSide::Right => u16::from(right),
            OutputSide::LeftRight => u16::from(left) + u16::from(right),
        }
    }

    fn channel_output_side(&self, side: OutputSide) -> u8 {
        let mut output = 0u16;
        for channel in 0..CHANNEL_COUNT {
            output += u16::from(self.channel_output(channel, side));
        }
        output.min(u16::from(u8::MAX)) as u8
    }

    fn channel_output(&self, channel: usize, side: OutputSide) -> u8 {
        let side_volume = match side {
            OutputSide::Left | OutputSide::LeftRight => self.volume[channel] >> 4,
            OutputSide::Right => self.volume[channel] & 0x0F,
        };
        if side_volume == 0 {
            return 0;
        }

        if channel == 1 && self.control & CHANNEL_2_VOICE != 0 {
            return self.volume[channel];
        }

        if self.control & (1 << channel) == 0 {
            return 0;
        }

        if channel == 3 && self.control & CHANNEL_4_NOISE != 0 {
            if self.noise_control & NOISE_ENABLE == 0 {
                return 0;
            }
            return if self.nreg & 1 != 0 { side_volume } else { 0 };
        }

        self.sample_pos[channel] & 0x0F
    }

    fn mix_current_sample(&self, ram: &[u8]) -> (f32, f32) {
        let mut left = 0.0;
        let mut right = 0.0;

        for channel in 0..CHANNEL_COUNT {
            if self.control & (1 << channel) == 0 || self.channel_mutes[channel] {
                continue;
            }

            let (channel_left, channel_right) =
                if channel == 1 && self.control & CHANNEL_2_VOICE != 0 {
                    self.mix_voice_channel(channel)
                } else if channel == 3
                    && self.control & CHANNEL_4_NOISE != 0
                    && self.noise_control & NOISE_ENABLE != 0
                {
                    self.mix_noise_channel(channel)
                } else {
                    self.mix_wave_channel(channel, ram)
                };
            left += channel_left;
            right += channel_right;
        }

        if self.hyper_voice_control & 0x80 != 0 {
            let sample = f32::from(self.hyper_voice_output_sample()) / 1024.0;
            if self.hyper_voice_channel_control & 0x40 != 0 {
                left += sample;
            }
            if self.hyper_voice_channel_control & 0x20 != 0 {
                right += sample;
            }
        }

        (
            (left * 0.25).clamp(-1.0, 1.0),
            (right * 0.25).clamp(-1.0, 1.0),
        )
    }

    fn mix_wave_channel(&self, channel: usize, ram: &[u8]) -> (f32, f32) {
        let sample = f32::from(self.wave_sample(channel, ram));
        let centered = (sample - 7.5) / 7.5;
        let left = centered * f32::from(self.volume[channel] >> 4) / 15.0;
        let right = centered * f32::from(self.volume[channel] & 0x0F) / 15.0;
        (left, right)
    }

    fn mix_noise_channel(&self, channel: usize) -> (f32, f32) {
        let sample = if self.nreg & 1 != 0 { 15.0 } else { 0.0 };
        let centered = (sample - 7.5) / 7.5;
        let left = centered * f32::from(self.volume[channel] >> 4) / 15.0;
        let right = centered * f32::from(self.volume[channel] & 0x0F) / 15.0;
        (left, right)
    }

    fn mix_voice_channel(&self, channel: usize) -> (f32, f32) {
        let full = f32::from(self.volume[channel]) / 255.0;
        let half = full * 0.5;
        let left = if self.voice_volume & 0x04 != 0 {
            full
        } else if self.voice_volume & 0x08 != 0 {
            half
        } else {
            0.0
        };
        let right = if self.voice_volume & 0x01 != 0 {
            full
        } else if self.voice_volume & 0x02 != 0 {
            half
        } else {
            0.0
        };
        (left, right)
    }

    fn wave_sample(&self, channel: usize, ram: &[u8]) -> u8 {
        if ram.is_empty() {
            return 0;
        }
        let offset = (usize::from(self.sample_ram_pos) << 6)
            + (channel << 4)
            + usize::from(self.sample_pos[channel] >> 1);
        let byte = ram.get(offset % ram.len()).copied().unwrap_or(0);
        if self.sample_pos[channel] & 1 == 0 {
            byte & 0x0F
        } else {
            byte >> 4
        }
    }

    fn hyper_voice_output_sample(&self) -> i16 {
        let shift = u32::from(8 - (self.hyper_voice_control & 0x03));
        let sample = match self.hyper_voice_control & 0x0C {
            0x00 => (u16::from(self.hyper_voice_sample) << shift) as i16,
            0x04 => (i16::from(self.hyper_voice_sample) | !0x00FF) << shift,
            0x08 => (self.hyper_voice_sample as i8 as i16) << shift,
            _ => (u16::from(self.hyper_voice_sample) << 8) as i16,
        };
        sample >> 5
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn alternating_wave_ram() -> Vec<u8> {
        let mut ram = vec![0; 0x10000];
        for byte in ram.iter_mut().take(16) {
            *byte = 0xF0;
        }
        ram
    }

    #[test]
    fn wavetable_channel_generates_stereo_samples() {
        let mut apu = Apu::new(48_000);
        let ram = alternating_wave_ram();
        apu.write8(0x80, 0x00);
        apu.write8(0x81, 0x07);
        apu.write8(0x88, 0xFF);
        apu.write8(0x90, 0x01);

        apu.step_cycles(4096, &ram);

        let mut samples = Vec::new();
        apu.drain_audio_samples_into(&mut samples);
        assert!(!samples.is_empty());
        assert_eq!(samples.len() % 2, 0);
        assert!(samples.iter().any(|sample| sample.abs() > 0.001));
    }

    #[test]
    fn disabled_sample_generation_keeps_buffer_empty() {
        let mut apu = Apu::new(48_000);
        let ram = alternating_wave_ram();
        apu.write8(0x80, 0x00);
        apu.write8(0x81, 0x07);
        apu.write8(0x88, 0xFF);
        apu.write8(0x90, 0x01);
        apu.set_sample_generation_enabled(false);

        apu.step_cycles(4096, &ram);

        let mut samples = Vec::new();
        apu.drain_audio_samples_into(&mut samples);
        assert!(samples.is_empty());
        assert_ne!(apu.debug_snapshot().sample_pos[0], 0);
    }

    #[test]
    fn channel_mute_suppresses_output() {
        let mut apu = Apu::new(48_000);
        let ram = alternating_wave_ram();
        apu.write8(0x80, 0x00);
        apu.write8(0x81, 0x07);
        apu.write8(0x88, 0xFF);
        apu.write8(0x90, 0x01);
        apu.set_channel_mutes([true, false, false, false]);

        apu.step_cycles(4096, &ram);

        let mut samples = Vec::new();
        apu.drain_audio_samples_into(&mut samples);
        assert!(!samples.is_empty());
        assert!(samples.iter().all(|sample| sample.abs() <= f32::EPSILON));
    }

    #[test]
    fn sound_port_masks_match_hardware_behavior() {
        let mut apu = Apu::new(48_000);

        apu.write8(0x81, 0xFF);
        apu.write8(0x8E, 0xFF);
        apu.write8(0x91, 0xFF);
        apu.write8(0x93, 0xFF);
        apu.write8(0x94, 0xFF);
        apu.write8(0x6B, 0xFF);

        assert_eq!(apu.read8(0x81), 0x07);
        assert_eq!(apu.read8(0x8E), 0x17);
        assert_eq!(apu.read8(0x91), 0x8F);
        assert_eq!(apu.read8(0x93), 0x7F);
        assert_eq!(apu.read8(0x94), 0x0F);
        assert_eq!(apu.read8(0x6B), 0x6F);
    }
}
