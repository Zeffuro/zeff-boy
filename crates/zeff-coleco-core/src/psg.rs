use zeff_emu_common::save_state::{StateReader, StateWriter};

use crate::constants::{CPU_CLOCK_HZ, DEFAULT_SAMPLE_RATE};

pub const COLECO_PSG_INPUT_CLOCK_HZ: u32 = CPU_CLOCK_HZ as u32;
pub const DEFAULT_HOST_SAMPLE_RATE_HZ: u32 = DEFAULT_SAMPLE_RATE;
pub const PSG_CHANNEL_COUNT: usize = 4;
pub const PSG_TONE_CHANNEL_COUNT: usize = 3;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PsgDebugSnapshot {
    pub last_write: Option<u8>,
    pub write_count: u64,
    pub latched_register: u8,
    pub tone_periods: [u16; PSG_TONE_CHANNEL_COUNT],
    pub effective_tone_periods: [u16; PSG_TONE_CHANNEL_COUNT],
    pub tone_counters: [u16; PSG_TONE_CHANNEL_COUNT],
    pub tone_output_high: [bool; PSG_TONE_CHANNEL_COUNT],
    pub volumes: [u8; PSG_CHANNEL_COUNT],
    pub noise_control: u8,
    pub noise_counter: u16,
    pub noise_lfsr: u16,
    pub ready: bool,
    pub ready_clocks_remaining: u8,
    pub sample_rate: u32,
    pub sample_generation_enabled: bool,
    pub muted: bool,
    pub channel_mutes: [bool; PSG_CHANNEL_COUNT],
    pub buffered_sample_count: usize,
}

const GENERATOR_CLOCK_DIVIDER: u8 = 16;
const READY_LOW_CLOCKS: u8 = 32;
const NOISE_MODE_WHITE: u8 = 0x04;
const NOISE_PERIOD_MASK: u8 = 0x03;
const TI_LFSR_MASK: u16 = 0x7FFF;
const TI_LFSR_RESET: u16 = 0x4000;
const MIX_GAIN: f32 = 0.25;
const MAX_BUFFERED_SAMPLES: usize = 262_144;

const VOLUME_TABLE: [f32; 16] = [
    1.0000000, 0.7943282, 0.6309574, 0.5011872, 0.3981072, 0.3162278, 0.2511886, 0.1995262,
    0.1584893, 0.1258925, 0.1, 0.0794328, 0.0630957, 0.0501187, 0.0398107, 0.0,
];

#[derive(Clone, Debug)]
pub struct Psg {
    last_write: Option<u8>,
    write_count: u64,
    latched_register: u8,
    tone_period: [u16; PSG_TONE_CHANNEL_COUNT],
    volume: [u8; PSG_CHANNEL_COUNT],
    noise_control: u8,
    tone_counter: [u16; PSG_TONE_CHANNEL_COUNT],
    tone_output_high: [bool; PSG_TONE_CHANNEL_COUNT],
    noise_counter: u16,
    noise_lfsr: u16,
    generator_clocks_remaining: u8,
    ready_low_clocks_remaining: u8,
    sample_cycle_accumulator: u32,

    sample_rate: u32,
    sample_generation_enabled: bool,
    muted: bool,
    channel_mutes: [bool; PSG_CHANNEL_COUNT],
    sample_buffer: Vec<f32>,
}

impl Default for Psg {
    fn default() -> Self {
        Self::new()
    }
}

impl Psg {
    pub fn new() -> Self {
        Self::new_with_sample_rate(DEFAULT_HOST_SAMPLE_RATE_HZ)
    }

    pub fn new_with_sample_rate(sample_rate: u32) -> Self {
        Self {
            last_write: None,
            write_count: 0,
            latched_register: 0,
            tone_period: [0; PSG_TONE_CHANNEL_COUNT],
            volume: [0; PSG_CHANNEL_COUNT],
            noise_control: 0,
            tone_counter: [0; PSG_TONE_CHANNEL_COUNT],
            tone_output_high: [false; PSG_TONE_CHANNEL_COUNT],
            noise_counter: 0,
            noise_lfsr: TI_LFSR_RESET,
            generator_clocks_remaining: GENERATOR_CLOCK_DIVIDER,
            ready_low_clocks_remaining: 0,
            sample_cycle_accumulator: 0,
            sample_rate: normalized_sample_rate(sample_rate),
            sample_generation_enabled: true,
            muted: false,
            channel_mutes: [false; PSG_CHANNEL_COUNT],
            sample_buffer: Vec::new(),
        }
    }

    pub fn reset(&mut self) {
        let sample_rate = self.sample_rate;
        let sample_generation_enabled = self.sample_generation_enabled;
        let muted = self.muted;
        let channel_mutes = self.channel_mutes;
        *self = Self::new_with_sample_rate(sample_rate);
        self.sample_generation_enabled = sample_generation_enabled;
        self.muted = muted;
        self.channel_mutes = channel_mutes;
    }

    pub fn debug_snapshot(&self) -> PsgDebugSnapshot {
        PsgDebugSnapshot {
            last_write: self.last_write,
            write_count: self.write_count,
            latched_register: self.latched_register,
            tone_periods: self.tone_period,
            effective_tone_periods: self.tone_period.map(effective_tone_period),
            tone_counters: self.tone_counter,
            tone_output_high: self.tone_output_high,
            volumes: self.volume,
            noise_control: self.noise_control,
            noise_counter: self.noise_counter,
            noise_lfsr: self.noise_lfsr,
            ready: self.ready(),
            ready_clocks_remaining: self.ready_low_clocks_remaining,
            sample_rate: self.sample_rate,
            sample_generation_enabled: self.sample_generation_enabled,
            muted: self.muted,
            channel_mutes: self.channel_mutes,
            buffered_sample_count: self.sample_buffer.len(),
        }
    }

    pub fn write(&mut self, value: u8) {
        self.begin_write();
        self.complete_write(value);
    }

    pub(crate) fn begin_write(&mut self) {
        self.ready_low_clocks_remaining = READY_LOW_CLOCKS;
    }

    pub(crate) fn complete_write(&mut self, value: u8) {
        self.last_write = Some(value);
        self.write_count = self.write_count.wrapping_add(1);

        if value & 0x80 != 0 {
            self.latched_register = (value >> 4) & 0x07;
            self.write_latched_value(value & 0x0F);
        } else {
            self.write_data_value(value);
        }
    }

    pub fn write_data(&mut self, value: u8) {
        self.write(value);
    }

    pub fn step_cycles(&mut self, cycles: u32) {
        let mut remaining = cycles;
        while remaining != 0 {
            let chunk = remaining
                .min(u32::from(self.generator_clocks_remaining))
                .min(self.clocks_until_next_sample().max(1));

            self.generator_clocks_remaining -= chunk as u8;
            self.ready_low_clocks_remaining = self
                .ready_low_clocks_remaining
                .saturating_sub(chunk.min(u32::from(u8::MAX)) as u8);

            if self.generator_clocks_remaining == 0 {
                self.clock_generators();
                self.generator_clocks_remaining = GENERATOR_CLOCK_DIVIDER;
            }
            self.advance_sample_clock(chunk);

            remaining -= chunk;
        }
    }

    pub fn ready(&self) -> bool {
        self.ready_low_clocks_remaining == 0
    }

    pub fn ready_clocks_remaining(&self) -> u8 {
        self.ready_low_clocks_remaining
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    pub fn set_sample_rate(&mut self, sample_rate: u32) {
        self.sample_rate = normalized_sample_rate(sample_rate);
        self.sample_buffer.clear();
    }

    pub fn sample_generation_enabled(&self) -> bool {
        self.sample_generation_enabled
    }

    pub fn set_sample_generation_enabled(&mut self, enabled: bool) {
        self.sample_generation_enabled = enabled;
        if !enabled {
            self.sample_buffer.clear();
        }
    }

    pub fn muted(&self) -> bool {
        self.muted
    }

    pub fn set_muted(&mut self, muted: bool) {
        self.muted = muted;
    }

    pub fn channel_mutes(&self) -> [bool; PSG_CHANNEL_COUNT] {
        self.channel_mutes
    }

    pub fn set_channel_mutes(&mut self, mutes: [bool; PSG_CHANNEL_COUNT]) {
        self.channel_mutes = mutes;
    }

    pub fn buffered_sample_count(&self) -> usize {
        self.sample_buffer.len()
    }

    pub fn drain_audio_samples_into(&mut self, out: &mut Vec<f32>) {
        out.append(&mut self.sample_buffer);
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

    pub fn effective_tone_periods(&self) -> [u16; PSG_TONE_CHANNEL_COUNT] {
        self.tone_period.map(effective_tone_period)
    }

    pub fn volumes(&self) -> [u8; PSG_CHANNEL_COUNT] {
        self.volume
    }

    pub fn noise_control(&self) -> u8 {
        self.noise_control
    }

    pub(crate) fn write_state(&self, w: &mut StateWriter) {
        w.write_bool(self.last_write.is_some());
        w.write_u8(self.last_write.unwrap_or(0));
        w.write_u64(self.write_count);
        w.write_u8(self.latched_register);
        for value in self.tone_period {
            w.write_u16(value);
        }
        for value in self.volume {
            w.write_u8(value);
        }
        w.write_u8(self.noise_control);
        for value in self.tone_counter {
            w.write_u16(value);
        }
        for value in self.tone_output_high {
            w.write_bool(value);
        }
        w.write_u16(self.noise_counter);
        w.write_u16(self.noise_lfsr);
        w.write_u8(self.generator_clocks_remaining);
        w.write_u8(self.ready_low_clocks_remaining);
    }

    pub(crate) fn read_state(&mut self, r: &mut StateReader<'_>) -> anyhow::Result<()> {
        let has_last_write = r.read_bool()?;
        let saved_last_write = r.read_u8()?;
        let last_write = has_last_write.then_some(saved_last_write);
        let write_count = r.read_u64()?;
        let latched_register = r.read_u8()?;
        if latched_register > 7 {
            anyhow::bail!("invalid Coleco PSG latched register: {latched_register}");
        }

        let mut tone_period = [0; PSG_TONE_CHANNEL_COUNT];
        for value in &mut tone_period {
            *value = r.read_u16()?;
            if *value > 0x03FF {
                anyhow::bail!("invalid Coleco PSG tone period: {value}");
            }
        }

        let mut volume = [0; PSG_CHANNEL_COUNT];
        for value in &mut volume {
            *value = r.read_u8()?;
            if *value > 0x0F {
                anyhow::bail!("invalid Coleco PSG attenuation: {value}");
            }
        }

        let noise_control = r.read_u8()?;
        if noise_control > 0x07 {
            anyhow::bail!("invalid Coleco PSG noise control: {noise_control}");
        }

        let mut tone_counter = [0; PSG_TONE_CHANNEL_COUNT];
        for value in &mut tone_counter {
            *value = r.read_u16()?;
            if *value > 0x0400 {
                anyhow::bail!("invalid Coleco PSG tone counter: {value}");
            }
        }

        let mut tone_output_high = [false; PSG_TONE_CHANNEL_COUNT];
        for value in &mut tone_output_high {
            *value = r.read_bool()?;
        }

        let noise_counter = r.read_u16()?;
        if noise_counter > 0x0800 {
            anyhow::bail!("invalid Coleco PSG noise counter: {noise_counter}");
        }
        let noise_lfsr = r.read_u16()?;
        if noise_lfsr == 0 || noise_lfsr > TI_LFSR_MASK {
            anyhow::bail!("invalid Coleco PSG noise LFSR: {noise_lfsr:#06x}");
        }
        let generator_clocks_remaining = r.read_u8()?;
        if !(1..=GENERATOR_CLOCK_DIVIDER).contains(&generator_clocks_remaining) {
            anyhow::bail!("invalid Coleco PSG generator divider: {generator_clocks_remaining}");
        }
        let ready_low_clocks_remaining = r.read_u8()?;
        if ready_low_clocks_remaining > READY_LOW_CLOCKS {
            anyhow::bail!("invalid Coleco PSG READY countdown: {ready_low_clocks_remaining}");
        }
        self.last_write = last_write;
        self.write_count = write_count;
        self.latched_register = latched_register;
        self.tone_period = tone_period;
        self.volume = volume;
        self.noise_control = noise_control;
        self.tone_counter = tone_counter;
        self.tone_output_high = tone_output_high;
        self.noise_counter = noise_counter;
        self.noise_lfsr = noise_lfsr;
        self.generator_clocks_remaining = generator_clocks_remaining;
        self.ready_low_clocks_remaining = ready_low_clocks_remaining;
        self.sample_cycle_accumulator = 0;
        self.sample_buffer.clear();
        Ok(())
    }

    fn write_latched_value(&mut self, value: u8) {
        match self.latched_register {
            0 | 2 | 4 => {
                let channel = usize::from(self.latched_register >> 1);
                self.tone_period[channel] =
                    (self.tone_period[channel] & 0x03F0) | u16::from(value & 0x0F);
            }
            1 | 3 | 5 | 7 => {
                let channel = usize::from(self.latched_register >> 1);
                self.volume[channel] = value & 0x0F;
            }
            6 => self.write_noise_control(value),
            _ => unreachable!(),
        }
    }

    fn write_data_value(&mut self, value: u8) {
        match self.latched_register {
            0 | 2 | 4 => {
                let channel = usize::from(self.latched_register >> 1);
                self.tone_period[channel] =
                    (self.tone_period[channel] & 0x000F) | (u16::from(value & 0x3F) << 4);
            }
            1 | 3 | 5 | 7 => {
                let channel = usize::from(self.latched_register >> 1);
                self.volume[channel] = value & 0x0F;
            }
            6 => self.write_noise_control(value),
            _ => unreachable!(),
        }
    }

    fn write_noise_control(&mut self, value: u8) {
        self.noise_control = value & 0x07;
        self.noise_lfsr = TI_LFSR_RESET;
        self.noise_counter = 0;
    }

    fn clocks_until_next_sample(&self) -> u32 {
        let remaining = u64::from(COLECO_PSG_INPUT_CLOCK_HZ - self.sample_cycle_accumulator);
        let rate = u64::from(self.sample_rate);
        remaining.div_ceil(rate).min(u64::from(u32::MAX)) as u32
    }

    fn advance_sample_clock(&mut self, cycles: u32) {
        let mut accumulator = u64::from(self.sample_cycle_accumulator)
            + u64::from(cycles) * u64::from(self.sample_rate);
        while accumulator >= u64::from(COLECO_PSG_INPUT_CLOCK_HZ) {
            accumulator -= u64::from(COLECO_PSG_INPUT_CLOCK_HZ);
            if self.sample_generation_enabled
                && self.sample_buffer.len() <= MAX_BUFFERED_SAMPLES.saturating_sub(2)
            {
                let mono = self.mix_current_sample();
                self.sample_buffer.push(mono);
                self.sample_buffer.push(mono);
            }
        }
        self.sample_cycle_accumulator = accumulator as u32;
    }

    fn clock_generators(&mut self) {
        let mut tone_three_rising = false;
        for channel in 0..PSG_TONE_CHANNEL_COUNT {
            if self.tone_counter[channel] <= 1 {
                self.tone_output_high[channel] = !self.tone_output_high[channel];
                tone_three_rising = channel == 2 && self.tone_output_high[channel];
                self.tone_counter[channel] = effective_tone_period(self.tone_period[channel]);
            } else {
                self.tone_counter[channel] -= 1;
            }
        }

        if self.noise_control & NOISE_PERIOD_MASK == 3 {
            self.noise_counter = 0;
            if tone_three_rising {
                self.clock_noise_lfsr();
            }
        } else if self.noise_counter <= 1 {
            self.clock_noise_lfsr();
            self.noise_counter = self.noise_period();
        } else {
            self.noise_counter -= 1;
        }
    }

    fn noise_period(&self) -> u16 {
        match self.noise_control & NOISE_PERIOD_MASK {
            0 => 32,
            1 => 64,
            2 => 128,
            _ => effective_tone_period(self.tone_period[2]) * 2,
        }
    }

    fn clock_noise_lfsr(&mut self) {
        let feedback = if self.noise_control & NOISE_MODE_WHITE != 0 {
            (self.noise_lfsr ^ (self.noise_lfsr >> 1)) & 1
        } else {
            self.noise_lfsr & 1
        };
        self.noise_lfsr = ((self.noise_lfsr >> 1) | (feedback << 14)) & TI_LFSR_MASK;
        if self.noise_lfsr == 0 {
            self.noise_lfsr = TI_LFSR_RESET;
        }
    }

    fn mix_current_sample(&self) -> f32 {
        if self.muted {
            return 0.0;
        }

        let mut mixed = 0.0;
        for channel in 0..PSG_CHANNEL_COUNT {
            if self.channel_mutes[channel] {
                continue;
            }
            let attenuation = VOLUME_TABLE[usize::from(self.volume[channel])];
            let high = if channel < PSG_TONE_CHANNEL_COUNT {
                self.tone_output_high[channel]
            } else {
                self.noise_lfsr & 1 != 0
            };
            mixed += if high { attenuation } else { -attenuation };
        }
        mixed * MIX_GAIN
    }
}

fn normalized_sample_rate(sample_rate: u32) -> u32 {
    if sample_rate == 0 {
        DEFAULT_HOST_SAMPLE_RATE_HZ
    } else {
        sample_rate
    }
}

fn effective_tone_period(period: u16) -> u16 {
    if period == 0 { 0x0400 } else { period }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stereo_pairs_are_mono(samples: &[f32]) -> bool {
        samples
            .as_chunks::<2>()
            .0
            .iter()
            .all(|pair| pair[0].to_bits() == pair[1].to_bits())
    }

    #[test]
    fn latch_and_data_protocol_programs_all_register_shapes() {
        let mut psg = Psg::new();

        psg.write(0x84);
        psg.write(0x23);
        psg.write(0xB7);
        psg.write(0xE5);
        psg.write(0x02);

        assert_eq!(psg.tone_periods()[0], 0x234);
        assert_eq!(psg.volumes()[1], 7);
        assert_eq!(psg.noise_control(), 2);
        assert_eq!(psg.last_write(), Some(0x02));
        assert_eq!(psg.write_count(), 5);
    }

    #[test]
    fn ti_tone_period_zero_is_effectively_0x400() {
        let mut psg = Psg::new();
        psg.write(0x80);
        psg.write(0x00);

        assert_eq!(psg.tone_periods()[0], 0);
        assert_eq!(psg.effective_tone_periods()[0], 0x400);

        psg.step_cycles(u32::from(GENERATOR_CLOCK_DIVIDER));
        assert_eq!(psg.tone_counter[0], 0x400);
    }

    #[test]
    fn white_noise_uses_known_ti_15_bit_sequence() {
        let mut psg = Psg::new();
        psg.write(0xE4);

        let expected = [
            0x2000, 0x1000, 0x0800, 0x0400, 0x0200, 0x0100, 0x0080, 0x0040, 0x0020, 0x0010, 0x0008,
            0x0004, 0x0002, 0x4001, 0x6000, 0x3000,
        ];
        for state in expected {
            psg.clock_noise_lfsr();
            assert_eq!(psg.noise_lfsr, state);
        }
    }

    #[test]
    fn tone_three_noise_shifts_only_on_tone_rising_edges() {
        let mut psg = Psg::new();
        psg.write(0xC2);
        psg.write(0x00);
        psg.write(0xE3);

        psg.clock_generators();
        assert_eq!(psg.noise_lfsr, 0x2000);
        psg.clock_generators();
        psg.clock_generators();
        assert_eq!(psg.noise_lfsr, 0x2000);
        psg.clock_generators();
        psg.clock_generators();
        assert_eq!(psg.noise_lfsr, 0x1000);
    }

    #[test]
    fn fixed_noise_rate_zero_clocks_at_input_clock_divided_by_512() {
        let mut psg = Psg::new();
        psg.write(0xE0);

        psg.step_cycles(15);
        assert_eq!(psg.noise_lfsr, TI_LFSR_RESET);
        psg.step_cycles(1);
        assert_eq!(psg.noise_lfsr, 0x2000);
        psg.step_cycles(511);
        assert_eq!(psg.noise_lfsr, 0x2000);
        psg.step_cycles(1);
        assert_eq!(psg.noise_lfsr, 0x1000);
    }

    #[test]
    fn ready_remains_low_for_exactly_32_input_clocks() {
        let mut psg = Psg::new();
        assert!(psg.ready());

        psg.write(0x9F);
        assert!(!psg.ready());
        assert_eq!(psg.ready_clocks_remaining(), 32);

        psg.step_cycles(31);
        assert!(!psg.ready());
        assert_eq!(psg.ready_clocks_remaining(), 1);

        psg.step_cycles(1);
        assert!(psg.ready());
    }

    #[test]
    fn debug_snapshot_reports_machine_and_host_audio_state() {
        let mut psg = Psg::new_with_sample_rate(32_000);
        psg.set_muted(true);
        psg.set_channel_mutes([true, false, true, false]);
        psg.write(0x85);
        psg.write(0x12);

        let snapshot = psg.debug_snapshot();
        assert_eq!(snapshot.tone_periods[0], 0x125);
        assert_eq!(snapshot.effective_tone_periods[0], 0x125);
        assert_eq!(snapshot.latched_register, 0);
        assert_eq!(snapshot.write_count, 2);
        assert!(!snapshot.ready);
        assert_eq!(snapshot.ready_clocks_remaining, 32);
        assert_eq!(snapshot.sample_rate, 32_000);
        assert!(snapshot.muted);
        assert_eq!(snapshot.channel_mutes, [true, false, true, false]);
    }

    #[test]
    fn audio_is_bounded_drainable_and_duplicated_to_stereo() {
        let mut psg = Psg::new_with_sample_rate(48_000);
        psg.write(0x80);
        psg.write(0x04);
        psg.write(0x90);
        psg.write(0xBF);
        psg.write(0xDF);
        psg.write(0xFF);

        psg.step_cycles(59_736);
        let mut samples = Vec::new();
        psg.drain_audio_samples_into(&mut samples);

        assert!((1_600..=1_604).contains(&samples.len()));
        assert_eq!(samples.len() % 2, 0);
        assert!(stereo_pairs_are_mono(&samples));
        assert!(samples.iter().any(|sample| sample.abs() > 0.01));
        assert_eq!(psg.buffered_sample_count(), 0);

        psg.set_sample_rate(COLECO_PSG_INPUT_CLOCK_HZ);
        psg.step_cycles((MAX_BUFFERED_SAMPLES as u32) * 2);
        assert_eq!(psg.buffered_sample_count(), MAX_BUFFERED_SAMPLES);
    }

    #[test]
    fn host_sampling_uses_the_exact_input_clock_ratio() {
        let mut psg = Psg::new_with_sample_rate(48_000);
        let cycles = 59_736;
        psg.step_cycles(cycles);

        let mut samples = Vec::new();
        psg.drain_audio_samples_into(&mut samples);
        let expected_frames = u64::from(cycles) * 48_000 / u64::from(COLECO_PSG_INPUT_CLOCK_HZ);
        assert_eq!(samples.len(), (expected_frames * 2) as usize);
    }

    #[test]
    fn state_roundtrip_continues_exact_machine_state_and_resets_host_audio_phase() {
        let mut original = Psg::new_with_sample_rate(48_000);
        original.write(0x87);
        original.write(0x12);
        original.write(0x92);
        original.write(0xE4);
        original.write(0xF3);
        original.step_cycles(12_345);

        let mut writer = StateWriter::new();
        original.write_state(&mut writer);
        let bytes = writer.into_bytes();
        let mut restored = Psg::new_with_sample_rate(48_000);
        let mut reader = StateReader::new(&bytes);
        restored.read_state(&mut reader).unwrap();
        assert!(reader.is_exhausted());
        assert_eq!(restored.buffered_sample_count(), 0);

        assert_eq!(original.tone_counter, restored.tone_counter);
        assert_eq!(original.tone_output_high, restored.tone_output_high);
        assert_eq!(original.noise_counter, restored.noise_counter);
        assert_eq!(original.noise_lfsr, restored.noise_lfsr);
        assert_eq!(restored.sample_cycle_accumulator, 0);
    }

    #[test]
    fn machine_state_bytes_are_independent_of_host_sample_rate() {
        let mut low_rate = Psg::new_with_sample_rate(32_000);
        let mut high_rate = Psg::new_with_sample_rate(96_000);
        for psg in [&mut low_rate, &mut high_rate] {
            psg.write(0x87);
            psg.write(0x12);
            psg.write(0x92);
            psg.step_cycles(12_345);
        }

        let mut low_writer = StateWriter::new();
        low_rate.write_state(&mut low_writer);
        let mut high_writer = StateWriter::new();
        high_rate.write_state(&mut high_writer);
        assert_eq!(low_writer.into_bytes(), high_writer.into_bytes());
    }

    #[test]
    fn loading_state_preserves_host_controls_and_discards_host_queue() {
        let source = Psg::new();
        let mut writer = StateWriter::new();
        source.write_state(&mut writer);
        let bytes = writer.into_bytes();

        let mut restored = Psg::new_with_sample_rate(32_000);
        restored.set_sample_generation_enabled(false);
        restored.set_muted(true);
        restored.set_channel_mutes([true, false, true, false]);
        let mut reader = StateReader::new(&bytes);
        restored.read_state(&mut reader).unwrap();

        assert_eq!(restored.sample_rate(), 32_000);
        assert!(!restored.sample_generation_enabled());
        assert!(restored.muted());
        assert_eq!(restored.channel_mutes(), [true, false, true, false]);
        assert_eq!(restored.buffered_sample_count(), 0);
    }
}
