use std::collections::VecDeque;

use super::constants::SEGA8_DEFAULT_HOST_SAMPLE_RATE_HZ;
use super::timing::Sega8VideoStandard;

pub const PSG_CHANNEL_COUNT: usize = 4;
pub const PSG_TONE_CHANNEL_COUNT: usize = 3;

const TONE_CLOCK_DIVIDER: i32 = 16;
const MIX_GAIN: f32 = 0.75 / PSG_CHANNEL_COUNT as f32;
const DEFAULT_STEREO_CONTROL: u8 = 0xFF;
const NOISE_LFSR_RESET: u16 = 0x8000;
const NOISE_LFSR_FEEDBACK_BIT: u16 = 0x8000;
const NOISE_MODE_WHITE: u8 = 0x04;
const NOISE_PERIOD_MASK: u8 = 0x03;
const MAX_SAVED_SAMPLE_BUFFER_LEN: usize = 262_144;
const DEBUG_WAVEFORM_SAMPLE_COUNT: usize = 512;
const CANONICAL_SAVED_SAMPLE_RATE: u32 = SEGA8_DEFAULT_HOST_SAMPLE_RATE_HZ;

// SN76489 volume registers use 2 dB attenuation steps; 15 is silent.
const VOLUME_TABLE: [f32; 16] = [
    1.0000000, 0.7943282, 0.6309574, 0.5011872, 0.3981072, 0.3162278, 0.2511886, 0.1995262,
    0.1584893, 0.1258925, 0.1, 0.0794328, 0.0630957, 0.0501187, 0.0398107, 0.0,
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LatchedRegister {
    Tone(usize),
    Volume(usize),
    Noise,
}

impl Default for LatchedRegister {
    fn default() -> Self {
        Self::Tone(0)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PsgDebugSnapshot {
    pub tone_period: [u16; PSG_TONE_CHANNEL_COUNT],
    pub volume: [u8; PSG_CHANNEL_COUNT],
    pub noise_control: u8,
    pub stereo_control: u8,
    pub latched_register: &'static str,
    pub sample_rate: u32,
    pub sample_generation_enabled: bool,
    pub buffered_samples: usize,
    pub channel_mutes: [bool; PSG_CHANNEL_COUNT],
    pub last_write: Option<u8>,
    pub write_count: u64,
}

#[derive(Clone, Debug)]
pub struct Psg {
    last_write: Option<u8>,
    write_count: u64,
    latched_register: LatchedRegister,
    tone_period: [u16; PSG_TONE_CHANNEL_COUNT],
    volume: [u8; PSG_CHANNEL_COUNT],
    noise_control: u8,
    stereo_control: u8,
    tone_counter_cycles: [i32; PSG_TONE_CHANNEL_COUNT],
    tone_output_high: [bool; PSG_TONE_CHANNEL_COUNT],
    noise_counter_cycles: i32,
    noise_lfsr: u16,
    clock_hz: u32,
    sample_rate: u32,
    sample_generation_enabled: bool,
    sample_cycle_accumulator: u32,
    sample_buffer: Vec<f32>,
    debug_master_samples: VecDeque<f32>,
    debug_channel_samples: [VecDeque<f32>; PSG_CHANNEL_COUNT],
    channel_mutes: [bool; PSG_CHANNEL_COUNT],
}

pub type Apu = Psg;
pub type ApuDebugSnapshot = PsgDebugSnapshot;

impl Default for Psg {
    fn default() -> Self {
        Self::new()
    }
}

impl Psg {
    pub fn new() -> Self {
        Self::new_with_sample_rate(SEGA8_DEFAULT_HOST_SAMPLE_RATE_HZ)
    }

    pub fn new_with_sample_rate(sample_rate: u32) -> Self {
        Self::new_with_sample_rate_and_clock_hz(
            sample_rate,
            Sega8VideoStandard::Ntsc.clock_hz_approx(),
        )
    }

    pub fn new_with_sample_rate_and_clock_hz(sample_rate: u32, clock_hz: u32) -> Self {
        let mut psg = Self {
            last_write: None,
            write_count: 0,
            latched_register: LatchedRegister::default(),
            tone_period: [0; PSG_TONE_CHANNEL_COUNT],
            volume: [15; PSG_CHANNEL_COUNT],
            noise_control: 0,
            stereo_control: DEFAULT_STEREO_CONTROL,
            tone_counter_cycles: [TONE_CLOCK_DIVIDER; PSG_TONE_CHANNEL_COUNT],
            tone_output_high: [true; PSG_TONE_CHANNEL_COUNT],
            noise_counter_cycles: 512,
            noise_lfsr: NOISE_LFSR_RESET,
            clock_hz: clock_hz.max(1),
            sample_rate: sample_rate.max(1),
            sample_generation_enabled: true,
            sample_cycle_accumulator: 0,
            sample_buffer: Vec::new(),
            debug_master_samples: VecDeque::with_capacity(DEBUG_WAVEFORM_SAMPLE_COUNT),
            debug_channel_samples: std::array::from_fn(|_| {
                VecDeque::with_capacity(DEBUG_WAVEFORM_SAMPLE_COUNT)
            }),
            channel_mutes: [false; PSG_CHANNEL_COUNT],
        };
        if sample_rate == 0 {
            psg.sample_rate = SEGA8_DEFAULT_HOST_SAMPLE_RATE_HZ;
        }
        psg
    }

    pub fn reset(&mut self) {
        let sample_rate = self.sample_rate;
        let clock_hz = self.clock_hz;
        *self = Self::new_with_sample_rate_and_clock_hz(sample_rate, clock_hz);
    }

    pub fn write_data(&mut self, value: u8) {
        self.last_write = Some(value);
        self.write_count = self.write_count.wrapping_add(1);

        if value & 0x80 != 0 {
            self.write_latch_byte(value);
        } else {
            self.write_data_byte(value);
        }
    }

    pub fn write_stereo_control(&mut self, value: u8) {
        self.stereo_control = value;
    }

    pub fn step_cycles(&mut self, cycles: u32) {
        let mut remaining = cycles;
        while remaining > 0 {
            let chunk = remaining.min(self.clocks_until_next_sample().max(1));
            self.advance_generators(chunk);
            self.advance_sample_clock(chunk);
            remaining -= chunk;
        }
    }

    pub fn drain_audio_samples_into(&mut self, out: &mut Vec<f32>) {
        out.append(&mut self.sample_buffer);
    }

    pub fn set_sample_rate(&mut self, sample_rate: u32) {
        self.sample_rate = sample_rate.max(1);
        if sample_rate == 0 {
            self.sample_rate = SEGA8_DEFAULT_HOST_SAMPLE_RATE_HZ;
        }
        self.sample_cycle_accumulator %= self.clock_hz;
        self.sample_buffer.clear();
        self.clear_debug_sample_history();
    }

    pub fn set_clock_hz(&mut self, clock_hz: u32) {
        let clock_hz = clock_hz.max(1);
        if self.clock_hz == clock_hz {
            return;
        }
        self.sample_cycle_accumulator = (u64::from(self.sample_cycle_accumulator)
            * u64::from(clock_hz)
            / u64::from(self.clock_hz)) as u32;
        self.clock_hz = clock_hz;
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    pub fn set_sample_generation_enabled(&mut self, enabled: bool) {
        self.sample_generation_enabled = enabled;
        if !enabled {
            self.sample_buffer.clear();
            self.clear_debug_sample_history();
        }
    }

    pub fn sample_generation_enabled(&self) -> bool {
        self.sample_generation_enabled
    }

    pub fn set_channel_mutes(&mut self, mutes: [bool; PSG_CHANNEL_COUNT]) {
        self.channel_mutes = mutes;
    }

    pub fn channel_mutes(&self) -> [bool; PSG_CHANNEL_COUNT] {
        self.channel_mutes
    }

    pub fn last_write(&self) -> Option<u8> {
        self.last_write
    }

    pub fn write_count(&self) -> u64 {
        self.write_count
    }

    pub fn tone_periods(&self) -> [u16; PSG_TONE_CHANNEL_COUNT] {
        self.tone_period
    }

    pub fn volumes(&self) -> [u8; PSG_CHANNEL_COUNT] {
        self.volume
    }

    pub fn noise_control(&self) -> u8 {
        self.noise_control
    }

    pub fn stereo_control(&self) -> u8 {
        self.stereo_control
    }

    pub fn buffered_sample_count(&self) -> usize {
        self.sample_buffer.len()
    }

    pub fn master_debug_samples_ordered(&self) -> Vec<f32> {
        self.debug_master_samples.iter().copied().collect()
    }

    pub fn channel_debug_samples_ordered(&self, channel: usize) -> Vec<f32> {
        self.debug_channel_samples
            .get(channel)
            .map(|samples| samples.iter().copied().collect())
            .unwrap_or_default()
    }

    pub fn debug_snapshot(&self) -> PsgDebugSnapshot {
        PsgDebugSnapshot {
            tone_period: self.tone_period,
            volume: self.volume,
            noise_control: self.noise_control,
            stereo_control: self.stereo_control,
            latched_register: self.latched_register_name(),
            sample_rate: self.sample_rate,
            sample_generation_enabled: self.sample_generation_enabled,
            buffered_samples: self.sample_buffer.len(),
            channel_mutes: self.channel_mutes,
            last_write: self.last_write,
            write_count: self.write_count,
        }
    }

    pub(crate) fn write_state(&self, w: &mut zeff_emu_common::save_state::StateWriter) {
        match self.last_write {
            Some(value) => {
                w.write_bool(true);
                w.write_u8(value);
            }
            None => {
                w.write_bool(false);
                w.write_u8(0);
            }
        }
        w.write_u64(self.write_count);
        write_latched_register(w, self.latched_register);
        for period in self.tone_period {
            w.write_u16(period);
        }
        for volume in self.volume {
            w.write_u8(volume);
        }
        w.write_u8(self.noise_control);
        w.write_u8(self.stereo_control);
        for counter in self.tone_counter_cycles {
            write_i32(w, counter);
        }
        for high in self.tone_output_high {
            w.write_bool(high);
        }
        write_i32(w, self.noise_counter_cycles);
        w.write_u16(self.noise_lfsr);
        w.write_u32(CANONICAL_SAVED_SAMPLE_RATE);
        w.write_bool(true);
        w.write_u32(0);
        w.write_u32(0);
        for _ in 0..PSG_CHANNEL_COUNT {
            w.write_bool(false);
        }
    }

    pub(crate) fn read_state(
        &mut self,
        r: &mut zeff_emu_common::save_state::StateReader<'_>,
    ) -> anyhow::Result<()> {
        let runtime_sample_rate = self.sample_rate;
        let runtime_sample_generation_enabled = self.sample_generation_enabled;
        let runtime_channel_mutes = self.channel_mutes;

        self.last_write = if r.read_bool()? {
            Some(r.read_u8()?)
        } else {
            let _unused = r.read_u8()?;
            None
        };
        self.write_count = r.read_u64()?;
        self.latched_register = read_latched_register(r)?;
        for period in &mut self.tone_period {
            *period = r.read_u16()?;
        }
        for volume in &mut self.volume {
            *volume = r.read_u8()? & 0x0F;
        }
        self.noise_control = r.read_u8()?;
        self.stereo_control = r.read_u8()?;
        for counter in &mut self.tone_counter_cycles {
            *counter = read_i32(r)?;
        }
        for high in &mut self.tone_output_high {
            *high = r.read_bool()?;
        }
        self.noise_counter_cycles = read_i32(r)?;
        self.noise_lfsr = r.read_u16()?;
        let _saved_sample_rate = r.read_u32()?;
        let _saved_sample_generation_enabled = r.read_bool()?;
        let _saved_sample_cycle_accumulator = r.read_u32()?;
        let sample_count = r.read_u32()? as usize;
        if sample_count > MAX_SAVED_SAMPLE_BUFFER_LEN {
            anyhow::bail!("Sega 8-bit save-state PSG sample buffer too large: {sample_count}");
        }
        self.sample_buffer.clear();
        self.sample_buffer.reserve(sample_count);
        for _ in 0..sample_count {
            self.sample_buffer.push(read_f32(r)?);
        }
        for _ in 0..PSG_CHANNEL_COUNT {
            let _saved_mute = r.read_bool()?;
        }
        self.sample_rate = runtime_sample_rate;
        self.sample_generation_enabled = runtime_sample_generation_enabled;
        self.sample_cycle_accumulator = 0;
        self.channel_mutes = runtime_channel_mutes;
        self.sample_buffer.clear();
        self.clear_debug_sample_history();
        Ok(())
    }

    fn write_latch_byte(&mut self, value: u8) {
        let channel = usize::from((value >> 5) & 0x03);
        let is_volume = value & 0x10 != 0;
        let data = value & 0x0F;

        self.latched_register = if is_volume {
            LatchedRegister::Volume(channel)
        } else if channel == 3 {
            LatchedRegister::Noise
        } else {
            LatchedRegister::Tone(channel)
        };

        match self.latched_register {
            LatchedRegister::Tone(channel) => {
                self.tone_period[channel] = (self.tone_period[channel] & 0x03F0) | u16::from(data);
                self.clamp_tone_counter(channel);
            }
            LatchedRegister::Volume(channel) => {
                self.volume[channel] = data;
            }
            LatchedRegister::Noise => {
                self.write_noise_control(data);
            }
        }
    }

    fn write_data_byte(&mut self, value: u8) {
        match self.latched_register {
            LatchedRegister::Tone(channel) => {
                self.tone_period[channel] =
                    (self.tone_period[channel] & 0x000F) | (u16::from(value & 0x3F) << 4);
                self.clamp_tone_counter(channel);
            }
            LatchedRegister::Volume(channel) => {
                self.volume[channel] = value & 0x0F;
            }
            LatchedRegister::Noise => {
                self.write_noise_control(value & 0x0F);
            }
        }
    }

    fn write_noise_control(&mut self, value: u8) {
        self.noise_control = value & 0x07;
        self.noise_lfsr = NOISE_LFSR_RESET;
        self.noise_counter_cycles = self.noise_reload_cycles();
    }

    fn clamp_tone_counter(&mut self, channel: usize) {
        if self.tone_period[channel] <= 1 {
            self.tone_output_high[channel] = true;
            self.tone_counter_cycles[channel] = TONE_CLOCK_DIVIDER;
            return;
        }
        let reload = self.tone_reload_cycles(channel);
        self.tone_counter_cycles[channel] = self.tone_counter_cycles[channel].clamp(1, reload);
    }

    fn clocks_until_next_sample(&self) -> u32 {
        let remaining = u64::from(self.clock_hz - self.sample_cycle_accumulator);
        let rate = u64::from(self.sample_rate);
        remaining.div_ceil(rate).min(u64::from(u32::MAX)) as u32
    }

    fn advance_sample_clock(&mut self, cycles: u32) {
        let mut accumulator = u64::from(self.sample_cycle_accumulator)
            + u64::from(cycles) * u64::from(self.sample_rate);
        while accumulator >= u64::from(self.clock_hz) {
            accumulator -= u64::from(self.clock_hz);
            if self.sample_generation_enabled {
                let (left, right, channels) = self.mix_current_sample();
                self.sample_buffer.push(left);
                self.sample_buffer.push(right);
                self.push_debug_samples((left + right) * 0.5, channels);
            }
        }
        self.sample_cycle_accumulator = accumulator as u32;
    }

    fn advance_generators(&mut self, cycles: u32) {
        let cycles = cycles as i32;
        for channel in 0..PSG_TONE_CHANNEL_COUNT {
            if self.tone_period[channel] <= 1 {
                self.tone_output_high[channel] = true;
                self.tone_counter_cycles[channel] = TONE_CLOCK_DIVIDER;
                continue;
            }
            self.tone_counter_cycles[channel] -= cycles;
            let reload = self.tone_reload_cycles(channel);
            while self.tone_counter_cycles[channel] <= 0 {
                self.tone_output_high[channel] = !self.tone_output_high[channel];
                self.tone_counter_cycles[channel] += reload;
            }
        }

        self.noise_counter_cycles -= cycles;
        let noise_reload = self.noise_reload_cycles();
        while self.noise_counter_cycles <= 0 {
            self.clock_noise_lfsr();
            self.noise_counter_cycles += noise_reload;
        }
    }

    fn tone_reload_cycles(&self, channel: usize) -> i32 {
        i32::from(self.tone_period[channel].max(1)) * TONE_CLOCK_DIVIDER
    }

    fn noise_reload_cycles(&self) -> i32 {
        match self.noise_control & NOISE_PERIOD_MASK {
            0 => 512,
            1 => 1024,
            2 => 2048,
            _ => self.tone_reload_cycles(2).max(1),
        }
    }

    fn clock_noise_lfsr(&mut self) {
        let bit0 = self.noise_lfsr & 1;
        let feedback = if self.noise_control & NOISE_MODE_WHITE != 0 {
            bit0 ^ ((self.noise_lfsr >> 3) & 1)
        } else {
            bit0
        };
        self.noise_lfsr >>= 1;
        if feedback != 0 {
            self.noise_lfsr |= NOISE_LFSR_FEEDBACK_BIT;
        }
        if self.noise_lfsr == 0 {
            self.noise_lfsr = NOISE_LFSR_RESET;
        }
    }

    fn mix_current_sample(&self) -> (f32, f32, [f32; PSG_CHANNEL_COUNT]) {
        let mut left = 0.0;
        let mut right = 0.0;
        let channels = self.current_channel_samples();

        for (channel, sample) in channels.iter().copied().enumerate() {
            if self.stereo_control & (1 << (channel + 4)) != 0 {
                left += sample;
            }
            if self.stereo_control & (1 << channel) != 0 {
                right += sample;
            }
        }

        (left * MIX_GAIN, right * MIX_GAIN, channels)
    }

    fn current_channel_samples(&self) -> [f32; PSG_CHANNEL_COUNT] {
        std::array::from_fn(|channel| {
            if self.channel_mutes[channel] {
                return 0.0;
            }

            let attenuation = VOLUME_TABLE[usize::from(self.volume[channel])];
            if attenuation == 0.0 {
                return 0.0;
            }

            let wave = if channel < PSG_TONE_CHANNEL_COUNT {
                if self.tone_output_high[channel] {
                    1.0
                } else {
                    -1.0
                }
            } else if self.noise_lfsr & 1 == 0 {
                1.0
            } else {
                -1.0
            };

            wave * attenuation
        })
    }

    fn push_debug_samples(&mut self, master: f32, channels: [f32; PSG_CHANNEL_COUNT]) {
        push_debug_sample(&mut self.debug_master_samples, master);
        for (history, sample) in self.debug_channel_samples.iter_mut().zip(channels) {
            push_debug_sample(history, sample);
        }
    }

    fn clear_debug_sample_history(&mut self) {
        self.debug_master_samples.clear();
        for channel in &mut self.debug_channel_samples {
            channel.clear();
        }
    }

    fn latched_register_name(&self) -> &'static str {
        match self.latched_register {
            LatchedRegister::Tone(0) => "tone0",
            LatchedRegister::Tone(1) => "tone1",
            LatchedRegister::Tone(2) => "tone2",
            LatchedRegister::Tone(_) => "tone?",
            LatchedRegister::Volume(0) => "volume0",
            LatchedRegister::Volume(1) => "volume1",
            LatchedRegister::Volume(2) => "volume2",
            LatchedRegister::Volume(3) => "volume3",
            LatchedRegister::Volume(_) => "volume?",
            LatchedRegister::Noise => "noise",
        }
    }
}

fn write_i32(w: &mut zeff_emu_common::save_state::StateWriter, value: i32) {
    w.write_u32(value as u32);
}

fn push_debug_sample(history: &mut VecDeque<f32>, sample: f32) {
    if history.len() == DEBUG_WAVEFORM_SAMPLE_COUNT {
        history.pop_front();
    }
    history.push_back(sample);
}

fn read_i32(r: &mut zeff_emu_common::save_state::StateReader<'_>) -> anyhow::Result<i32> {
    Ok(r.read_u32()? as i32)
}

fn read_f32(r: &mut zeff_emu_common::save_state::StateReader<'_>) -> anyhow::Result<f32> {
    Ok(f32::from_bits(r.read_u32()?))
}

fn write_latched_register(
    w: &mut zeff_emu_common::save_state::StateWriter,
    register: LatchedRegister,
) {
    match register {
        LatchedRegister::Tone(channel) => {
            w.write_u8(0);
            w.write_u8(channel as u8);
        }
        LatchedRegister::Volume(channel) => {
            w.write_u8(1);
            w.write_u8(channel as u8);
        }
        LatchedRegister::Noise => {
            w.write_u8(2);
            w.write_u8(0);
        }
    }
}

fn read_latched_register(
    r: &mut zeff_emu_common::save_state::StateReader<'_>,
) -> anyhow::Result<LatchedRegister> {
    let tag = r.read_u8()?;
    let channel = usize::from(r.read_u8()?);
    match tag {
        0 if channel < PSG_TONE_CHANNEL_COUNT => Ok(LatchedRegister::Tone(channel)),
        1 if channel < PSG_CHANNEL_COUNT => Ok(LatchedRegister::Volume(channel)),
        2 => Ok(LatchedRegister::Noise),
        _ => anyhow::bail!(
            "invalid Sega 8-bit PSG latched register in save-state: tag={tag} channel={channel}"
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hardware::timing::Sega8VideoStandard;

    fn peak(samples: &[f32]) -> f32 {
        samples
            .iter()
            .copied()
            .fold(0.0f32, |peak, sample| peak.max(sample.abs()))
    }

    fn ntsc_cycles_per_frame() -> u32 {
        Sega8VideoStandard::Ntsc.cycles_per_frame()
    }

    #[test]
    fn records_last_write_and_count() {
        let mut psg = Psg::new();

        psg.write_data(0x9F);
        psg.write_data(0x20);

        assert_eq!(psg.last_write(), Some(0x20));
        assert_eq!(psg.write_count(), 2);
    }

    #[test]
    fn latch_and_data_bytes_program_tone_period_and_volume() {
        let mut psg = Psg::new();

        psg.write_data(0x84);
        psg.write_data(0x23);
        psg.write_data(0x90);

        assert_eq!(psg.tone_periods()[0], 0x234);
        assert_eq!(psg.volumes()[0], 0);
    }

    #[test]
    fn noise_latch_resets_noise_lfsr_and_records_control() {
        let mut psg = Psg::new();

        psg.write_data(0xE7);
        psg.step_cycles(4096);
        assert_ne!(psg.noise_lfsr, NOISE_LFSR_RESET);

        psg.write_data(0xE5);

        assert_eq!(psg.noise_control(), 0x05);
        assert_eq!(psg.noise_lfsr, NOISE_LFSR_RESET);
    }

    #[test]
    fn step_cycles_generates_stereo_samples_at_configured_rate() {
        let mut psg = Psg::new_with_sample_rate(48_000);
        psg.write_data(0x80);
        psg.write_data(0x04);
        psg.write_data(0x90);

        psg.step_cycles(ntsc_cycles_per_frame());
        let mut samples = Vec::new();
        psg.drain_audio_samples_into(&mut samples);

        assert_eq!(samples.len() % 2, 0);
        assert!(
            (1598..=1602).contains(&samples.len()),
            "expected about 800 stereo pairs per frame, got {} samples",
            samples.len()
        );
        assert!(peak(&samples) > 0.05);
        assert!(psg.buffered_sample_count() == 0);
    }

    #[test]
    fn master_debug_samples_expose_recent_mono_waveform_without_draining_audio() {
        let mut psg = Psg::new_with_sample_rate(48_000);
        psg.write_data(0x80);
        psg.write_data(0x04);
        psg.write_data(0x90);

        psg.step_cycles(ntsc_cycles_per_frame());
        let buffered_before = psg.buffered_sample_count();
        let debug_samples = psg.master_debug_samples_ordered();

        assert_eq!(psg.buffered_sample_count(), buffered_before);
        assert_eq!(debug_samples.len(), DEBUG_WAVEFORM_SAMPLE_COUNT);
        assert!(peak(&debug_samples) > 0.05);
    }

    #[test]
    fn debug_samples_survive_audio_drain() {
        let mut psg = Psg::new_with_sample_rate(48_000);
        psg.write_data(0x80);
        psg.write_data(0x04);
        psg.write_data(0x90);

        psg.step_cycles(ntsc_cycles_per_frame());
        let mut drained = Vec::new();
        psg.drain_audio_samples_into(&mut drained);

        assert_eq!(psg.buffered_sample_count(), 0);
        assert_eq!(
            psg.master_debug_samples_ordered().len(),
            DEBUG_WAVEFORM_SAMPLE_COUNT
        );
        assert_eq!(
            psg.channel_debug_samples_ordered(0).len(),
            DEBUG_WAVEFORM_SAMPLE_COUNT
        );
        assert!(peak(&psg.master_debug_samples_ordered()) > 0.05);
        assert!(peak(&psg.channel_debug_samples_ordered(0)) > 0.05);
    }

    #[test]
    fn tone_period_zero_or_one_outputs_constant_high() {
        for period in [0u8, 1] {
            let mut psg = Psg::new_with_sample_rate(48_000);
            psg.write_data(0x80 | period);
            psg.write_data(0x90);

            psg.step_cycles(ntsc_cycles_per_frame() / 4);
            let mut samples = Vec::new();
            psg.drain_audio_samples_into(&mut samples);

            assert!(!samples.is_empty());
            assert!(psg.tone_output_high[0]);
            assert!(
                samples
                    .iter()
                    .all(|sample| (*sample - MIX_GAIN).abs() < f32::EPSILON)
            );
        }
    }

    #[test]
    fn sample_generation_can_be_disabled_without_freezing_register_state() {
        let mut psg = Psg::new();
        psg.write_data(0x80);
        psg.write_data(0x04);
        psg.write_data(0x90);
        psg.set_sample_generation_enabled(false);

        psg.step_cycles(ntsc_cycles_per_frame());

        assert_eq!(psg.buffered_sample_count(), 0);
        assert_eq!(psg.tone_periods()[0], 0x040);
    }

    #[test]
    fn game_gear_stereo_control_pans_channels() {
        let mut psg = Psg::new();
        psg.write_data(0x80);
        psg.write_data(0x04);
        psg.write_data(0x90);
        psg.write_stereo_control(0x10);

        psg.step_cycles(ntsc_cycles_per_frame() / 10);
        let mut samples = Vec::new();
        psg.drain_audio_samples_into(&mut samples);

        let left_peak = samples
            .as_chunks::<2>()
            .0
            .iter()
            .map(|lr| lr[0].abs())
            .fold(0.0, f32::max);
        let right_peak = samples
            .as_chunks::<2>()
            .0
            .iter()
            .map(|lr| lr[1].abs())
            .fold(0.0, f32::max);

        assert!(left_peak > 0.05);
        assert_eq!(right_peak, 0.0);
    }

    #[test]
    fn channel_mutes_remove_selected_channel_from_mix() {
        let mut psg = Psg::new();
        psg.write_data(0x80);
        psg.write_data(0x04);
        psg.write_data(0x90);
        psg.set_channel_mutes([true, false, false, false]);

        psg.step_cycles(ntsc_cycles_per_frame() / 10);
        let mut samples = Vec::new();
        psg.drain_audio_samples_into(&mut samples);

        assert_eq!(peak(&samples), 0.0);
    }
}
