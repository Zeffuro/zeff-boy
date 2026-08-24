use super::blip_buf::BlipBuf;
use super::bus::{BaseBusDevices, OPEN_BUS_VALUE, PsgPort};
use anyhow::bail;
use std::fmt;
use zeff_emu_common::save_state::{StateReader, StateWriter};

pub const PSG_CHANNEL_COUNT: usize = 6;
pub const PSG_WAVEFORM_WORDS: usize = 32;
pub const PSG_MASTER_CLOCK_DIVISOR: u64 = 6;
pub const PSG_INTERNAL_MASTER_CLOCK_DIVISOR: u64 = 3;
pub const PSG_CLOCK_NUMERATOR: u64 = 315_000_000;
pub const PSG_CLOCK_DENOMINATOR: u64 = 88;
pub const PSG_INTERNAL_CLOCK_NUMERATOR: u64 = PSG_CLOCK_NUMERATOR * 2;
pub const DEFAULT_PSG_SAMPLE_RATE: u32 = 44_100;
pub const MAX_PSG_SAMPLE_RATE: u32 = 192_000;
pub const PSG_DEBUG_WAVEFORM_SAMPLE_COUNT: usize = 512;
pub const PSG_DEBUG_WAVEFORM_RATE_HZ: u32 = 44_100;
pub const PSG_UNAVAILABLE_READ_VALUE: u8 = OPEN_BUS_VALUE;
pub const DETERMINISTIC_PSG_RESET_VALUE: u8 = 0;
pub const DETERMINISTIC_PSG_RESET_CLEARS_WAVE_RAM: bool = true;
pub const DETERMINISTIC_PSG_RESET_ATTENUATION_SLOT: u8 = 31;
pub const PSG_ZERO_FREQUENCY_PERIOD: i32 = 0x1000;
pub const PROVISIONAL_PSG_NOISE_ZERO_PERIOD: u16 = 1;
pub const PROVISIONAL_PSG_GAIN_SCAN_CLOCKS_PER_PASS: u16 = 4_096;
pub const PROVISIONAL_PSG_GAIN_LATCH_DELAY_CLOCKS: u16 = 255;
pub const PROVISIONAL_HUC6280_KEYED_WAVE_WRITE_MATCHES_HUC6280A: bool = true;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PsgRevision {
    #[default]
    HuC6280,
    HuC6280A,
}

const CHANNEL_CONTROL_MASK: u8 = 0xDF;
const NOISE_CONTROL_MASK: u8 = 0x9F;
const LFO_CONTROL_MASK: u8 = 0x83;
const PROVISIONAL_LFO_ZERO_PERIOD: i32 = 0;
const LFO_TARGET_PERIOD_MIN: i32 = 1;
const LFO_TARGET_PERIOD_MAX: i32 = 0x1FFF;
const MIX_SCALE: i64 = PSG_CHANNEL_COUNT as i64 * 31 * 1_000_000;
const BLIP_LEVEL_MAX: i64 = i16::MAX as i64;
const BLIP_BUFFER_MIN_SAMPLES: u32 = 2_048;
const BLIP_BUFFER_MARGIN: u32 = 64;
const BLIP_FRAME_CLOCKS: u32 = 65_536;
const MAX_PSG_STATE_AUDIO_SAMPLES: usize = MAX_PSG_SAMPLE_RATE as usize * 2;
pub(super) const MAX_PSG_STATE_SECTION_BYTES: usize = 1024 * 1024;
const PROVISIONAL_PSG_GAIN_COMPONENT_CLOCKS: u16 = PROVISIONAL_PSG_GAIN_LATCH_DELAY_CLOCKS + 1;
const ATTENUATION_GAIN: [i32; 32] = [
    1_000_000, 841_395, 707_946, 595_662, 501_187, 421_697, 354_813, 298_538, 251_189, 211_349,
    177_828, 149_624, 125_893, 105_925, 89_125, 74_989, 63_096, 53_088, 44_668, 37_584, 31_623,
    26_607, 22_387, 18_836, 15_849, 13_335, 11_220, 9_441, 7_943, 6_683, 0, 0,
];

#[derive(Debug)]
struct DebugWaveformHistory {
    samples: [f32; PSG_DEBUG_WAVEFORM_SAMPLE_COUNT],
    cursor: usize,
    count: usize,
}

impl Default for DebugWaveformHistory {
    fn default() -> Self {
        Self {
            samples: [0.0; PSG_DEBUG_WAVEFORM_SAMPLE_COUNT],
            cursor: 0,
            count: 0,
        }
    }
}

impl DebugWaveformHistory {
    fn push(&mut self, sample: f32) {
        self.samples[self.cursor] = sample;
        self.cursor = (self.cursor + 1) % PSG_DEBUG_WAVEFORM_SAMPLE_COUNT;
        self.count = (self.count + 1).min(PSG_DEBUG_WAVEFORM_SAMPLE_COUNT);
    }

    fn ordered(&self) -> Vec<f32> {
        let start = if self.count == PSG_DEBUG_WAVEFORM_SAMPLE_COUNT {
            self.cursor
        } else {
            0
        };
        (0..self.count)
            .map(|index| self.samples[(start + index) % PSG_DEBUG_WAVEFORM_SAMPLE_COUNT])
            .collect()
    }

    fn clear(&mut self) {
        self.cursor = 0;
        self.count = 0;
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PsgChannel {
    frequency: u16,
    control: u8,
    balance: u8,
    waveform: [u8; PSG_WAVEFORM_WORDS],
    wave_index: u8,
    dda_hold: u8,
    noise_control: u8,
    wave_counter: i32,
    noise_counter: u16,
    noise_seed: u32,
    effective_left_attenuation: u8,
    effective_right_attenuation: u8,
}

impl Default for PsgChannel {
    fn default() -> Self {
        Self::new()
    }
}

impl PsgChannel {
    pub const fn new() -> Self {
        Self {
            frequency: 0,
            control: 0,
            balance: 0,
            waveform: [0; PSG_WAVEFORM_WORDS],
            wave_index: 0,
            dda_hold: 0,
            noise_control: 0,
            wave_counter: PSG_ZERO_FREQUENCY_PERIOD,
            noise_counter: 0,
            noise_seed: 1,
            effective_left_attenuation: DETERMINISTIC_PSG_RESET_ATTENUATION_SLOT,
            effective_right_attenuation: DETERMINISTIC_PSG_RESET_ATTENUATION_SLOT,
        }
    }

    #[inline]
    pub const fn frequency(&self) -> u16 {
        self.frequency
    }

    #[inline]
    pub const fn control(&self) -> u8 {
        self.control
    }

    #[inline]
    pub const fn key_on(&self) -> bool {
        self.control & 0x80 != 0
    }

    #[inline]
    pub const fn dda_enabled(&self) -> bool {
        self.control & 0x40 != 0
    }

    #[inline]
    pub const fn amplitude(&self) -> u8 {
        self.control & 0x1F
    }

    #[inline]
    pub const fn balance(&self) -> u8 {
        self.balance
    }

    #[inline]
    pub const fn waveform(&self) -> &[u8; PSG_WAVEFORM_WORDS] {
        &self.waveform
    }

    #[inline]
    pub const fn wave_index(&self) -> u8 {
        self.wave_index
    }

    #[inline]
    pub const fn dda_hold(&self) -> u8 {
        self.dda_hold
    }

    #[inline]
    pub const fn noise_control(&self) -> u8 {
        self.noise_control
    }

    #[inline]
    pub const fn noise_enabled(&self) -> bool {
        self.noise_control & 0x80 != 0
    }

    #[inline]
    pub const fn noise_frequency(&self) -> u8 {
        self.noise_control & 0x1F
    }

    #[inline]
    pub const fn effective_left_attenuation(&self) -> u8 {
        self.effective_left_attenuation
    }

    #[inline]
    pub const fn effective_right_attenuation(&self) -> u8 {
        self.effective_right_attenuation
    }

    #[inline]
    fn write_frequency_low(&mut self, value: u8) {
        self.frequency = (self.frequency & 0x0F00) | u16::from(value);
    }

    #[inline]
    fn write_frequency_high(&mut self, value: u8) {
        self.frequency = (self.frequency & 0x00FF) | (u16::from(value & 0x0F) << 8);
    }

    #[inline]
    fn write_control(&mut self, value: u8) {
        let previous_dda = self.dda_enabled();
        let previous_key_on = self.key_on();
        self.control = value & CHANNEL_CONTROL_MASK;
        if previous_dda && !self.dda_enabled() {
            self.wave_counter = effective_period(self.frequency);
            self.wave_index = 0;
        } else if self.dda_enabled() {
            self.wave_index = 0;
        }
        if !previous_key_on && self.key_on() {
            self.wave_counter = effective_period(self.frequency);
        }
    }

    #[inline]
    fn write_wave_data(&mut self, value: u8) {
        let value = value & 0x1F;
        if self.dda_enabled() {
            self.dda_hold = value;
        } else if !self.key_on() {
            self.waveform[usize::from(self.wave_index)] = value;
            self.wave_index = self.wave_index.wrapping_add(1) & 0x1F;
        } else if PROVISIONAL_HUC6280_KEYED_WAVE_WRITE_MATCHES_HUC6280A {
            self.waveform[usize::from(self.wave_index)] = value;
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PsgChannelDebugSnapshot {
    pub frequency: u16,
    pub control: u8,
    pub balance: u8,
    pub waveform: [u8; PSG_WAVEFORM_WORDS],
    pub wave_index: u8,
    pub dda_hold: u8,
    pub noise_control: u8,
    pub wave_counter: i32,
    pub noise_counter: u16,
    pub noise_seed: u32,
    pub effective_left_attenuation: u8,
    pub effective_right_attenuation: u8,
}

impl PsgChannel {
    #[inline]
    pub const fn debug_snapshot(&self) -> PsgChannelDebugSnapshot {
        PsgChannelDebugSnapshot {
            frequency: self.frequency,
            control: self.control,
            balance: self.balance,
            waveform: self.waveform,
            wave_index: self.wave_index,
            dda_hold: self.dda_hold,
            noise_control: self.noise_control,
            wave_counter: self.wave_counter,
            noise_counter: self.noise_counter,
            noise_seed: self.noise_seed,
            effective_left_attenuation: self.effective_left_attenuation,
            effective_right_attenuation: self.effective_right_attenuation,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PsgDebugSnapshot {
    pub revision: PsgRevision,
    pub channels: [PsgChannelDebugSnapshot; PSG_CHANNEL_COUNT],
    pub selected_channel: u8,
    pub main_amplitude: u8,
    pub lfo_frequency: u8,
    pub lfo_control: u8,
    pub lfo_counter: i32,
    pub lfo_phase_valid: bool,
    pub gain_scan_clock: u16,
    pub gain_scan_active: bool,
    pub gain_scan_queued: bool,
    pub attenuation_latch: u8,
    pub master_tick_remainder: u8,
    pub sample_rate: u32,
    pub sample_generation_enabled: bool,
    pub channel_mutes: [bool; PSG_CHANNEL_COUNT],
    pub resampler_clock: u32,
    pub resampler_levels: [i32; 2],
    pub buffered_sample_frames: usize,
    pub mixed_output: [i64; 2],
    pub debug_capture_enabled: bool,
    pub debug_waveform_samples: usize,
}

#[derive(Debug)]
pub struct HuC6280Psg {
    revision: PsgRevision,
    channels: [PsgChannel; PSG_CHANNEL_COUNT],
    selected_channel: u8,
    main_amplitude: u8,
    lfo_frequency: u8,
    lfo_control: u8,
    lfo_counter: i32,
    lfo_phase_valid: bool,
    gain_scan_clock: u16,
    gain_scan_active: bool,
    gain_scan_queued: bool,
    attenuation_latch: u8,
    master_tick_remainder: u8,
    sample_rate: u32,
    sample_generation_enabled: bool,
    channel_mutes: [bool; PSG_CHANNEL_COUNT],
    resampler: StereoBlipResampler,
    audio_samples: Vec<i16>,
    debug_capture_enabled: bool,
    debug_capture_phase: u64,
    debug_master_history: DebugWaveformHistory,
    debug_channel_history: [DebugWaveformHistory; PSG_CHANNEL_COUNT],
}

struct StereoBlipResampler {
    left: BlipBuf,
    right: BlipBuf,
    clocks: u32,
    left_level: i32,
    right_level: i32,
    #[cfg(test)]
    last_delta_clock: Option<u32>,
}

impl fmt::Debug for StereoBlipResampler {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("StereoBlipResampler")
            .field("clocks", &self.clocks)
            .field("left_level", &self.left_level)
            .field("right_level", &self.right_level)
            .finish_non_exhaustive()
    }
}

impl StereoBlipResampler {
    fn new(sample_rate: u32) -> Self {
        Self::at_level(sample_rate, 0, 0)
    }

    fn at_level(sample_rate: u32, left_level: i64, right_level: i64) -> Self {
        Self::at_blip_level(sample_rate, blip_level(left_level), blip_level(right_level))
    }

    fn at_blip_level(sample_rate: u32, left_level: i32, right_level: i32) -> Self {
        let sample_rate = sample_rate.clamp(1, MAX_PSG_SAMPLE_RATE);
        let buffer_samples = blip_buffer_samples(sample_rate);
        let mut left = BlipBuf::new(buffer_samples);
        let mut right = BlipBuf::new(buffer_samples);
        let clock_rate = PSG_INTERNAL_CLOCK_NUMERATOR as f64 / PSG_CLOCK_DENOMINATOR as f64;
        left.set_rates(clock_rate, f64::from(sample_rate)).unwrap();
        right.set_rates(clock_rate, f64::from(sample_rate)).unwrap();
        Self {
            left,
            right,
            clocks: 0,
            left_level,
            right_level,
            #[cfg(test)]
            last_delta_clock: None,
        }
    }

    fn push_level(&mut self, left: i64, right: i64, output: &mut Vec<i16>) {
        self.clocks += 1;
        if self.clocks == BLIP_FRAME_CLOCKS {
            self.flush(output);
        }
        self.refresh_level(left, right);
    }

    fn refresh_level(&mut self, left: i64, right: i64) {
        let left = blip_level(left);
        let right = blip_level(right);
        let left_delta = left - self.left_level;
        let right_delta = right - self.right_level;
        #[cfg(test)]
        let changed = left_delta != 0 || right_delta != 0;
        if left_delta != 0 {
            self.left.add_delta(self.clocks, left_delta).unwrap();
            self.left_level = left;
        }
        if right_delta != 0 {
            self.right.add_delta(self.clocks, right_delta).unwrap();
            self.right_level = right;
        }
        #[cfg(test)]
        if changed {
            self.last_delta_clock = Some(self.clocks);
        }
    }

    fn flush(&mut self, output: &mut Vec<i16>) {
        if self.clocks == 0 {
            return;
        }
        flush_blip_pair(&mut self.left, &mut self.right, self.clocks, output);
        self.clocks = 0;
    }

    fn write_state(&self, writer: &mut StateWriter) {
        writer.write_u32(self.clocks);
        writer.write_u32(self.left_level as u32);
        writer.write_u32(self.right_level as u32);
        self.left.write_state(writer);
        self.right.write_state(writer);
    }

    fn read_state(sample_rate: u32, reader: &mut StateReader<'_>) -> anyhow::Result<Self> {
        let clocks = reader.read_u32()?;
        if clocks >= BLIP_FRAME_CLOCKS {
            bail!("invalid PSG resampler clock in save-state: {clocks}");
        }
        let saved_left_level = reader.read_u32()? as i32;
        let saved_right_level = reader.read_u32()? as i32;
        validate_blip_level(saved_left_level)?;
        validate_blip_level(saved_right_level)?;
        let mut restored = Self::at_blip_level(sample_rate, saved_left_level, saved_right_level);
        restored.left.read_state(reader)?;
        restored.right.read_state(reader)?;
        if !restored.left.timing_matches(&restored.right) {
            bail!("PSG stereo resampler timing differs between channels");
        }
        restored.clocks = clocks;
        Ok(restored)
    }
}

fn flush_blip_pair(
    left_buffer: &mut BlipBuf,
    right_buffer: &mut BlipBuf,
    clocks: u32,
    output: &mut Vec<i16>,
) {
    left_buffer.end_frame(clocks).unwrap();
    right_buffer.end_frame(clocks).unwrap();
    let available = left_buffer
        .samples_avail()
        .min(right_buffer.samples_avail()) as usize;
    let start = output.len();
    output.resize(start + available * 2 + 1, 0);
    let left = left_buffer.read_samples(&mut output[start..start + available * 2], true);
    let right = right_buffer.read_samples(&mut output[start + 1..start + 1 + available * 2], true);
    debug_assert_eq!(left, available);
    debug_assert_eq!(right, available);
    output.truncate(start + available * 2);
}

fn validate_blip_level(level: i32) -> anyhow::Result<()> {
    if !(-(i16::MAX as i32)..=i16::MAX as i32).contains(&level) {
        bail!("invalid PSG resampler level in save-state: {level}");
    }
    Ok(())
}

impl Default for HuC6280Psg {
    fn default() -> Self {
        Self::new()
    }
}

impl HuC6280Psg {
    pub fn new() -> Self {
        Self::with_revision(PsgRevision::HuC6280)
    }

    pub fn with_revision(revision: PsgRevision) -> Self {
        Self {
            revision,
            channels: [const { PsgChannel::new() }; PSG_CHANNEL_COUNT],
            selected_channel: DETERMINISTIC_PSG_RESET_VALUE,
            main_amplitude: DETERMINISTIC_PSG_RESET_VALUE,
            lfo_frequency: DETERMINISTIC_PSG_RESET_VALUE,
            lfo_control: DETERMINISTIC_PSG_RESET_VALUE,
            lfo_counter: PROVISIONAL_LFO_ZERO_PERIOD,
            lfo_phase_valid: false,
            gain_scan_clock: 0,
            gain_scan_active: false,
            gain_scan_queued: false,
            attenuation_latch: DETERMINISTIC_PSG_RESET_ATTENUATION_SLOT,
            master_tick_remainder: 0,
            sample_rate: DEFAULT_PSG_SAMPLE_RATE,
            sample_generation_enabled: true,
            channel_mutes: [false; PSG_CHANNEL_COUNT],
            resampler: StereoBlipResampler::new(DEFAULT_PSG_SAMPLE_RATE),
            audio_samples: Vec::new(),
            debug_capture_enabled: false,
            debug_capture_phase: 0,
            debug_master_history: DebugWaveformHistory::default(),
            debug_channel_history: std::array::from_fn(|_| DebugWaveformHistory::default()),
        }
    }

    pub fn reset(&mut self) {
        let revision = self.revision;
        let sample_rate = self.sample_rate;
        let sample_generation_enabled = self.sample_generation_enabled;
        let channel_mutes = self.channel_mutes;
        let debug_capture_enabled = self.debug_capture_enabled;
        *self = Self::with_revision(revision);
        self.sample_rate = sample_rate;
        self.sample_generation_enabled = sample_generation_enabled;
        self.channel_mutes = channel_mutes;
        self.debug_capture_enabled = debug_capture_enabled;
        self.resampler = StereoBlipResampler::new(sample_rate);
    }

    pub fn debug_snapshot(&self) -> PsgDebugSnapshot {
        let (left, right) = self.mix_output();
        PsgDebugSnapshot {
            revision: self.revision,
            channels: std::array::from_fn(|index| self.channels[index].debug_snapshot()),
            selected_channel: self.selected_channel,
            main_amplitude: self.main_amplitude,
            lfo_frequency: self.lfo_frequency,
            lfo_control: self.lfo_control,
            lfo_counter: self.lfo_counter,
            lfo_phase_valid: self.lfo_phase_valid,
            gain_scan_clock: self.gain_scan_clock,
            gain_scan_active: self.gain_scan_active,
            gain_scan_queued: self.gain_scan_queued,
            attenuation_latch: self.attenuation_latch,
            master_tick_remainder: self.master_tick_remainder,
            sample_rate: self.sample_rate,
            sample_generation_enabled: self.sample_generation_enabled,
            channel_mutes: self.channel_mutes,
            resampler_clock: self.resampler.clocks,
            resampler_levels: [self.resampler.left_level, self.resampler.right_level],
            buffered_sample_frames: self.audio_samples.len() / 2,
            mixed_output: [left, right],
            debug_capture_enabled: self.debug_capture_enabled,
            debug_waveform_samples: self.debug_master_history.count,
        }
    }

    #[inline]
    pub const fn revision(&self) -> PsgRevision {
        self.revision
    }

    #[inline]
    pub const fn channels(&self) -> &[PsgChannel; PSG_CHANNEL_COUNT] {
        &self.channels
    }

    #[inline]
    pub const fn selected_channel_id(&self) -> u8 {
        self.selected_channel
    }

    #[inline]
    pub fn selected_channel(&self) -> Option<&PsgChannel> {
        self.channels.get(usize::from(self.selected_channel))
    }

    #[inline]
    pub const fn main_amplitude(&self) -> u8 {
        self.main_amplitude
    }

    #[inline]
    pub const fn lfo_frequency(&self) -> u8 {
        self.lfo_frequency
    }

    #[inline]
    pub const fn lfo_control(&self) -> u8 {
        self.lfo_control
    }

    #[inline]
    pub const fn lfo_depth(&self) -> u8 {
        self.lfo_control & 3
    }

    #[inline]
    pub const fn lfo_halted(&self) -> bool {
        self.lfo_control & 0x80 != 0
    }

    #[inline]
    pub const fn lfo_active(&self) -> bool {
        !self.lfo_halted()
            && self.lfo_depth() != 0
            && self.channels[0].key_on()
            && self.channels[1].key_on()
    }

    pub fn set_sample_rate(&mut self, sample_rate: u32) {
        self.sample_rate = sample_rate.clamp(1, MAX_PSG_SAMPLE_RATE);
        self.resampler = self.resampler_at_current_level();
        self.audio_samples.clear();
    }

    pub fn set_sample_generation_enabled(&mut self, enabled: bool) {
        if enabled && !self.sample_generation_enabled {
            self.resampler = self.resampler_at_current_level();
        }
        self.sample_generation_enabled = enabled;
        if !enabled {
            self.audio_samples.clear();
            self.resampler = StereoBlipResampler::new(self.sample_rate);
        }
    }

    pub fn set_debug_capture_enabled(&mut self, enabled: bool) {
        self.debug_capture_enabled = enabled;
    }

    pub const fn debug_capture_enabled(&self) -> bool {
        self.debug_capture_enabled
    }

    pub fn master_debug_samples_ordered(&self) -> Vec<f32> {
        self.debug_master_history.ordered()
    }

    pub fn channel_debug_samples_ordered(&self, channel: usize) -> Vec<f32> {
        self.debug_channel_history
            .get(channel)
            .map_or_else(Vec::new, DebugWaveformHistory::ordered)
    }

    pub fn set_channel_mutes(&mut self, mutes: &[bool]) {
        self.channel_mutes =
            std::array::from_fn(|index| mutes.get(index).copied().unwrap_or(false));
        self.refresh_mixer_output();
    }

    pub(super) fn validate_v1_state(&self) -> anyhow::Result<()> {
        if self.audio_samples.len() > MAX_PSG_STATE_AUDIO_SAMPLES {
            bail!("PC Engine PSG queued audio exceeds its save-state bound");
        }
        if !self.audio_samples.len().is_multiple_of(2) {
            bail!("PC Engine PSG queued audio is not stereo-aligned");
        }
        if !self.sample_generation_enabled && !self.audio_samples.is_empty() {
            bail!("disabled PC Engine PSG has queued audio in save-state");
        }
        Ok(())
    }

    pub(super) const fn runtime_config(&self) -> (u32, bool, [bool; PSG_CHANNEL_COUNT], bool) {
        (
            self.sample_rate,
            self.sample_generation_enabled,
            self.channel_mutes,
            self.debug_capture_enabled,
        )
    }

    pub(super) fn apply_runtime_config(
        &mut self,
        sample_rate: u32,
        sample_generation_enabled: bool,
        channel_mutes: [bool; PSG_CHANNEL_COUNT],
        debug_capture_enabled: bool,
    ) {
        self.set_sample_rate(sample_rate);
        self.set_sample_generation_enabled(sample_generation_enabled);
        self.set_channel_mutes(&channel_mutes);
        self.set_debug_capture_enabled(debug_capture_enabled);
    }

    pub(super) fn write_state(&self, writer: &mut StateWriter) {
        for channel in &self.channels {
            writer.write_u16(channel.frequency);
            writer.write_u8(channel.control);
            writer.write_u8(channel.balance);
            writer.write_bytes(&channel.waveform);
            writer.write_u8(channel.wave_index);
            writer.write_u8(channel.dda_hold);
            writer.write_u8(channel.noise_control);
            writer.write_u32(channel.wave_counter as u32);
            writer.write_u16(channel.noise_counter);
            writer.write_u32(channel.noise_seed);
            writer.write_u8(channel.effective_left_attenuation);
            writer.write_u8(channel.effective_right_attenuation);
        }
        writer.write_u8(self.selected_channel);
        writer.write_u8(self.main_amplitude);
        writer.write_u8(self.lfo_frequency);
        writer.write_u8(self.lfo_control);
        writer.write_u32(self.lfo_counter as u32);
        writer.write_bool(self.lfo_phase_valid);
        writer.write_u16(self.gain_scan_clock);
        writer.write_bool(self.gain_scan_active);
        writer.write_bool(self.gain_scan_queued);
        writer.write_u8(self.attenuation_latch);
        writer.write_u8(self.master_tick_remainder);
        writer.write_u32(self.sample_rate);
        writer.write_bool(self.sample_generation_enabled);
        self.resampler.write_state(writer);
        writer.write_u32(self.audio_samples.len() as u32);
        for sample in &self.audio_samples {
            writer.write_u16(*sample as u16);
        }
    }

    pub(super) fn read_state(&mut self, reader: &mut StateReader<'_>) -> anyhow::Result<()> {
        let target_generation_enabled = self.sample_generation_enabled;
        let mut channels = [const { PsgChannel::new() }; PSG_CHANNEL_COUNT];
        for channel in &mut channels {
            channel.frequency = reader.read_u16()?;
            if channel.frequency > 0x0FFF {
                bail!("invalid PSG channel frequency in save-state");
            }
            channel.control = reader.read_u8()?;
            if channel.control & !CHANNEL_CONTROL_MASK != 0 {
                bail!("invalid PSG channel control in save-state");
            }
            channel.balance = reader.read_u8()?;
            reader.read_exact(&mut channel.waveform)?;
            if channel.waveform.iter().any(|&sample| sample > 0x1F) {
                bail!("invalid PSG waveform sample in save-state");
            }
            channel.wave_index = reader.read_u8()?;
            channel.dda_hold = reader.read_u8()?;
            if channel.wave_index > 0x1F || channel.dda_hold > 0x1F {
                bail!("invalid PSG waveform cursor or DDA value in save-state");
            }
            channel.noise_control = reader.read_u8()?;
            if channel.noise_control & !NOISE_CONTROL_MASK != 0 {
                bail!("invalid PSG noise control in save-state");
            }
            channel.wave_counter = reader.read_u32()? as i32;
            channel.noise_counter = reader.read_u16()?;
            channel.noise_seed = reader.read_u32()?;
            if channel.noise_seed == 0 || channel.noise_seed > 0x3_FFFF {
                bail!("invalid PSG noise seed in save-state");
            }
            channel.effective_left_attenuation = reader.read_u8()?;
            channel.effective_right_attenuation = reader.read_u8()?;
            if channel.effective_left_attenuation > 31 || channel.effective_right_attenuation > 31 {
                bail!("invalid PSG attenuation in save-state");
            }
        }

        let selected_channel = reader.read_u8()?;
        if selected_channel > 7 {
            bail!("invalid PSG selected channel in save-state: {selected_channel}");
        }
        let main_amplitude = reader.read_u8()?;
        let lfo_frequency = reader.read_u8()?;
        let lfo_control = reader.read_u8()?;
        if lfo_control & !LFO_CONTROL_MASK != 0 {
            bail!("invalid PSG LFO control in save-state");
        }
        let lfo_counter = reader.read_u32()? as i32;
        let lfo_phase_valid = reader.read_bool()?;
        let gain_scan_clock = reader.read_u16()?;
        if gain_scan_clock >= PROVISIONAL_PSG_GAIN_SCAN_CLOCKS_PER_PASS {
            bail!("invalid PSG gain-scan clock in save-state: {gain_scan_clock}");
        }
        let gain_scan_active = reader.read_bool()?;
        let gain_scan_queued = reader.read_bool()?;
        if gain_scan_queued && !gain_scan_active {
            bail!("queued PSG gain scan is inactive in save-state");
        }
        let attenuation_latch = reader.read_u8()?;
        if attenuation_latch > 31 {
            bail!("invalid PSG attenuation latch in save-state: {attenuation_latch}");
        }
        let master_tick_remainder = reader.read_u8()?;
        if master_tick_remainder >= PSG_MASTER_CLOCK_DIVISOR as u8 {
            bail!("invalid PSG master-clock remainder in save-state: {master_tick_remainder}");
        }
        let saved_sample_rate = reader.read_u32()?;
        if saved_sample_rate != self.sample_rate {
            bail!(
                "PC Engine PSG save-state sample rate mismatch: state is {saved_sample_rate} Hz, destination is {} Hz",
                self.sample_rate
            );
        }
        let saved_generation_enabled = reader.read_bool()?;
        let saved_resampler = StereoBlipResampler::read_state(saved_sample_rate, reader)?;
        let audio_sample_count = reader.read_u32()? as usize;
        if audio_sample_count > MAX_PSG_STATE_AUDIO_SAMPLES {
            bail!("PSG queued-audio sample count exceeds save-state bound: {audio_sample_count}");
        }
        if !audio_sample_count.is_multiple_of(2) {
            bail!("PSG queued audio is not stereo-aligned in save-state");
        }
        if !saved_generation_enabled && audio_sample_count != 0 {
            bail!("disabled PSG has queued audio in save-state");
        }
        let mut saved_audio_samples = Vec::with_capacity(audio_sample_count);
        for _ in 0..audio_sample_count {
            saved_audio_samples.push(reader.read_u16()? as i16);
        }

        self.channels = channels;
        self.selected_channel = selected_channel;
        self.main_amplitude = main_amplitude;
        self.lfo_frequency = lfo_frequency;
        self.lfo_control = lfo_control;
        self.lfo_counter = lfo_counter;
        self.lfo_phase_valid = lfo_phase_valid;
        self.gain_scan_clock = gain_scan_clock;
        self.gain_scan_active = gain_scan_active;
        self.gain_scan_queued = gain_scan_queued;
        self.attenuation_latch = attenuation_latch;
        self.master_tick_remainder = master_tick_remainder;
        if target_generation_enabled && saved_generation_enabled {
            self.resampler = saved_resampler;
            self.audio_samples = saved_audio_samples;
            self.refresh_mixer_output();
        } else if target_generation_enabled {
            self.resampler = self.resampler_at_current_level();
            self.audio_samples.clear();
        } else {
            self.resampler = StereoBlipResampler::new(self.sample_rate);
            self.audio_samples.clear();
        }
        self.clear_debug_sample_history();
        Ok(())
    }

    pub fn drain_audio_samples_into(&mut self, output: &mut Vec<f32>) {
        if self.sample_generation_enabled {
            self.resampler.flush(&mut self.audio_samples);
        }
        output.clear();
        output.extend(
            self.audio_samples
                .drain(..)
                .map(|sample| f32::from(sample) / 32_768.0),
        );
    }

    pub fn advance_master_ticks(&mut self, master_ticks: u64) {
        let previous_remainder = u64::from(self.master_tick_remainder);
        let total = previous_remainder + master_ticks;
        let internal_clocks = total / PSG_INTERNAL_MASTER_CLOCK_DIVISOR
            - previous_remainder / PSG_INTERNAL_MASTER_CLOCK_DIVISOR;
        self.master_tick_remainder = (total % PSG_MASTER_CLOCK_DIVISOR) as u8;
        let mut oscillator_clock = previous_remainder >= PSG_INTERNAL_MASTER_CLOCK_DIVISOR;
        for _ in 0..internal_clocks {
            oscillator_clock = !oscillator_clock;
            self.advance_internal_clock(!oscillator_clock);
        }
    }

    #[cfg(test)]
    pub(super) const fn master_tick_remainder(&self) -> u8 {
        self.master_tick_remainder
    }

    #[cfg(test)]
    pub(super) const fn resampler_clock(&self) -> u32 {
        self.resampler.clocks
    }

    #[cfg(test)]
    pub(super) const fn resampler_levels(&self) -> (i32, i32) {
        (self.resampler.left_level, self.resampler.right_level)
    }

    #[cfg(test)]
    pub(super) const fn gain_scan_state(&self) -> (bool, bool, u16) {
        (
            self.gain_scan_active,
            self.gain_scan_queued,
            self.gain_scan_clock,
        )
    }

    #[inline]
    pub const fn read_port(&self, _port: PsgPort) -> u8 {
        PSG_UNAVAILABLE_READ_VALUE
    }

    pub fn write_port(&mut self, port: PsgPort, value: u8) {
        let refresh_mixer = match port.offset() {
            0 => {
                self.selected_channel = value & 7;
                false
            }
            1 => {
                self.main_amplitude = value;
                self.queue_gain_scan();
                false
            }
            2 => {
                self.with_selected_channel(|channel| channel.write_frequency_low(value));
                false
            }
            3 => {
                self.with_selected_channel(|channel| channel.write_frequency_high(value));
                false
            }
            4 => {
                let valid = self.with_selected_channel(|channel| channel.write_control(value));
                if valid {
                    self.queue_gain_scan();
                }
                valid
            }
            5 => {
                let valid = self.with_selected_channel(|channel| channel.balance = value);
                if valid {
                    self.queue_gain_scan();
                }
                false
            }
            6 => {
                let refresh = self
                    .selected_channel()
                    .is_some_and(|channel| channel.dda_enabled() || channel.key_on());
                self.with_selected_channel(|channel| channel.write_wave_data(value));
                refresh
            }
            7 => {
                if matches!(self.selected_channel, 4 | 5) {
                    let channel = &mut self.channels[usize::from(self.selected_channel)];
                    channel.noise_control = value & NOISE_CONTROL_MASK;
                    true
                } else {
                    false
                }
            }
            8 => {
                self.lfo_frequency = value;
                if self.lfo_active() {
                    self.lfo_counter = lfo_period(self.channels[1].frequency, value);
                    self.lfo_phase_valid = true;
                }
                false
            }
            9 => {
                let was_active = self.lfo_active();
                self.lfo_control = value & LFO_CONTROL_MASK;
                if self.lfo_halted() {
                    self.channels[1].wave_index = 0;
                    self.lfo_counter = PROVISIONAL_LFO_ZERO_PERIOD;
                    self.lfo_phase_valid = false;
                } else if !was_active && self.lfo_active() {
                    self.channels[1].wave_index = 0;
                    self.lfo_counter = lfo_period(self.channels[1].frequency, self.lfo_frequency);
                    self.lfo_phase_valid = true;
                }
                true
            }
            _ => false,
        };
        if refresh_mixer {
            self.refresh_mixer_output();
        }
    }

    fn refresh_mixer_output(&mut self) {
        if !self.sample_generation_enabled {
            return;
        }
        let (left, right) = self.mix_output();
        self.resampler.refresh_level(left, right);
    }

    fn queue_gain_scan(&mut self) {
        if self.gain_scan_active {
            self.gain_scan_queued = true;
        } else {
            self.gain_scan_active = true;
            self.gain_scan_clock = 0;
        }
    }

    fn advance_internal_clock(&mut self, advance_oscillators: bool) {
        self.advance_gain_scan();
        if advance_oscillators {
            self.advance_oscillators();
        }
        self.advance_debug_capture();
        if !self.sample_generation_enabled {
            return;
        }
        let (left, right) = self.mix_output();
        self.resampler
            .push_level(left, right, &mut self.audio_samples);
    }

    fn advance_debug_capture(&mut self) {
        if !self.debug_capture_enabled {
            return;
        }
        self.debug_capture_phase += u64::from(PSG_DEBUG_WAVEFORM_RATE_HZ) * PSG_CLOCK_DENOMINATOR;
        if self.debug_capture_phase < PSG_INTERNAL_CLOCK_NUMERATOR {
            return;
        }
        self.debug_capture_phase -= PSG_INTERNAL_CLOCK_NUMERATOR;
        let channels = self.debug_channel_samples();
        for (history, sample) in self.debug_channel_history.iter_mut().zip(channels) {
            history.push(sample);
        }
        let (left, right) = self.mix_output();
        let master = (left + right) as f32 / (2 * MIX_SCALE) as f32;
        self.debug_master_history.push(master.clamp(-1.0, 1.0));
    }

    fn debug_channel_samples(&self) -> [f32; PSG_CHANNEL_COUNT] {
        std::array::from_fn(|index| {
            let channel = &self.channels[index];
            if !channel.key_on() || self.channel_mutes[index] {
                return 0.0;
            }
            let dac = if channel.noise_enabled() {
                if channel.noise_seed & 1 != 0 { 31 } else { 0 }
            } else if channel.dda_enabled() {
                channel.dda_hold()
            } else {
                channel.waveform[usize::from(channel.wave_index)]
            };
            let sample = match self.revision {
                PsgRevision::HuC6280 => i64::from(dac),
                PsgRevision::HuC6280A => i64::from(dac) - 16,
            };
            let left = sample
                * i64::from(ATTENUATION_GAIN[usize::from(channel.effective_left_attenuation)]);
            let right = sample
                * i64::from(ATTENUATION_GAIN[usize::from(channel.effective_right_attenuation)]);
            ((left + right) as f32 / (2 * 31_000_000) as f32).clamp(-1.0, 1.0)
        })
    }

    fn clear_debug_sample_history(&mut self) {
        self.debug_capture_phase = 0;
        self.debug_master_history.clear();
        for history in &mut self.debug_channel_history {
            history.clear();
        }
    }

    fn advance_gain_scan(&mut self) {
        if !self.gain_scan_active {
            return;
        }
        self.gain_scan_clock += 1;
        if (self.gain_scan_clock - 1).is_multiple_of(PROVISIONAL_PSG_GAIN_COMPONENT_CLOCKS) {
            let component = (self.gain_scan_clock - 1) / PROVISIONAL_PSG_GAIN_COMPONENT_CLOCKS;
            let channel = usize::from(component / 2);
            if channel < PSG_CHANNEL_COUNT {
                let channel = &self.channels[channel];
                self.attenuation_latch = if component & 1 == 0 {
                    attenuation_slot(self.main_amplitude, channel.amplitude(), channel.balance)
                } else {
                    attenuation_slot(
                        self.main_amplitude >> 4,
                        channel.amplitude(),
                        channel.balance >> 4,
                    )
                };
            }
        }
        if self
            .gain_scan_clock
            .is_multiple_of(PROVISIONAL_PSG_GAIN_COMPONENT_CLOCKS)
        {
            let component = self.gain_scan_clock / PROVISIONAL_PSG_GAIN_COMPONENT_CLOCKS - 1;
            let channel = usize::from(component / 2);
            if channel < PSG_CHANNEL_COUNT {
                if component & 1 == 0 {
                    self.channels[channel].effective_right_attenuation = self.attenuation_latch;
                } else {
                    self.channels[channel].effective_left_attenuation = self.attenuation_latch;
                }
            }
        }
        if self.gain_scan_clock == PROVISIONAL_PSG_GAIN_SCAN_CLOCKS_PER_PASS {
            self.gain_scan_clock = 0;
            if self.gain_scan_queued {
                self.gain_scan_queued = false;
            } else {
                self.gain_scan_active = false;
            }
        }
    }

    fn advance_oscillators(&mut self) {
        let lfo_active = self.lfo_active();
        let lfo_halted = self.lfo_halted();
        if !lfo_active {
            self.lfo_phase_valid = false;
        }
        for (index, channel) in self.channels.iter_mut().enumerate() {
            if index >= 4 {
                if channel.noise_counter <= 1 {
                    channel.noise_counter = noise_period(channel.noise_frequency());
                    let seed = channel.noise_seed;
                    let feedback =
                        (seed ^ (seed >> 1) ^ (seed >> 11) ^ (seed >> 12) ^ (seed >> 17)) & 1;
                    channel.noise_seed = (seed >> 1) | (feedback << 17);
                } else {
                    channel.noise_counter -= 1;
                }
            }
            if lfo_active && index == 0 {
                continue;
            }
            if lfo_active && index == 1 {
                continue;
            }
            if lfo_halted && index == 1 {
                continue;
            }
            if channel.key_on() && !channel.noise_enabled() && !channel.dda_enabled() {
                if channel.wave_counter <= 1 {
                    channel.wave_counter = effective_period(channel.frequency);
                    channel.wave_index = channel.wave_index.wrapping_add(1) & 0x1F;
                } else {
                    channel.wave_counter -= 1;
                }
            }
        }
        if lfo_active {
            self.advance_lfo_tick();
        }
    }

    #[cfg(test)]
    fn advance_psg_tick(&mut self) {
        self.advance_internal_clock(false);
        self.advance_internal_clock(true);
    }

    fn mix_output(&self) -> (i64, i64) {
        let mut left = 0_i64;
        let mut right = 0_i64;
        for (index, channel) in self.channels.iter().enumerate() {
            if !channel.key_on() || self.channel_mutes[index] {
                continue;
            }
            let dac = if channel.noise_enabled() {
                if channel.noise_seed & 1 != 0 { 31 } else { 0 }
            } else if channel.dda_enabled() {
                channel.dda_hold()
            } else {
                channel.waveform[usize::from(channel.wave_index)]
            };
            let sample = match self.revision {
                PsgRevision::HuC6280 => i64::from(dac),
                PsgRevision::HuC6280A => i64::from(dac) - 16,
            };
            left += sample
                * i64::from(ATTENUATION_GAIN[usize::from(channel.effective_left_attenuation)]);
            right += sample
                * i64::from(ATTENUATION_GAIN[usize::from(channel.effective_right_attenuation)]);
        }
        (left, right)
    }

    fn resampler_at_current_level(&self) -> StereoBlipResampler {
        let (left, right) = self.mix_output();
        StereoBlipResampler::at_level(self.sample_rate, left, right)
    }

    fn advance_lfo_tick(&mut self) {
        if !self.lfo_phase_valid {
            self.channels[1].wave_index = 0;
            self.lfo_counter = lfo_period(self.channels[1].frequency, self.lfo_frequency);
            self.lfo_phase_valid = true;
        }
        let source_sample = self.channels[1].waveform[usize::from(self.channels[1].wave_index)];
        if self.lfo_counter <= 1 {
            self.lfo_counter = lfo_period(self.channels[1].frequency, self.lfo_frequency);
            self.channels[1].wave_index = self.channels[1].wave_index.wrapping_add(1) & 0x1F;
        } else {
            self.lfo_counter -= 1;
        }

        let period = lfo_target_period(
            effective_period(self.channels[0].frequency),
            source_sample,
            self.lfo_depth(),
        );
        let target = &mut self.channels[0];
        if target.wave_counter <= 1 {
            target.wave_counter = period;
            target.wave_index = target.wave_index.wrapping_add(1) & 0x1F;
        } else {
            target.wave_counter -= 1;
        }
    }

    #[inline]
    fn with_selected_channel(&mut self, write: impl FnOnce(&mut PsgChannel)) -> bool {
        if let Some(channel) = self.channels.get_mut(usize::from(self.selected_channel)) {
            write(channel);
            true
        } else {
            false
        }
    }
}

#[inline]
const fn effective_period(frequency: u16) -> i32 {
    if frequency == 0 {
        PSG_ZERO_FREQUENCY_PERIOD
    } else {
        frequency as i32
    }
}

#[inline]
const fn lfo_period(frequency: u16, lfo_frequency: u8) -> i32 {
    if lfo_frequency == 0 {
        PROVISIONAL_LFO_ZERO_PERIOD
    } else {
        effective_period(frequency) * lfo_frequency as i32
    }
}

#[inline]
const fn lfo_target_period(base_period: i32, source_sample: u8, depth: u8) -> i32 {
    let shift = (depth.saturating_sub(1) * 2) as u32;
    let period = base_period + ((source_sample as i32 - 16) << shift);
    if period < LFO_TARGET_PERIOD_MIN {
        LFO_TARGET_PERIOD_MIN
    } else if period > LFO_TARGET_PERIOD_MAX {
        LFO_TARGET_PERIOD_MAX
    } else {
        period
    }
}

const fn attenuation_slot(main: u8, channel: u8, balance: u8) -> u8 {
    let slot = 2 * (15 - (main & 0x0F)) + (31 - (channel & 0x1F)) + 2 * (15 - (balance & 0x0F));
    if slot > 31 { 31 } else { slot }
}

#[inline]
fn blip_level(sample: i64) -> i32 {
    ((sample.clamp(-MIX_SCALE, MIX_SCALE) * BLIP_LEVEL_MAX) / MIX_SCALE) as i32
}

#[inline]
const fn blip_buffer_samples(sample_rate: u32) -> u32 {
    let numerator = BLIP_FRAME_CLOCKS as u64 * sample_rate as u64 * PSG_CLOCK_DENOMINATOR;
    let samples = numerator.div_ceil(PSG_INTERNAL_CLOCK_NUMERATOR) as u32 + BLIP_BUFFER_MARGIN;
    if samples < BLIP_BUFFER_MIN_SAMPLES {
        BLIP_BUFFER_MIN_SAMPLES
    } else {
        samples
    }
}

#[inline]
const fn noise_period(frequency: u8) -> u16 {
    let noise_factor = frequency ^ 0x1F;
    if noise_factor == 0 {
        PROVISIONAL_PSG_NOISE_ZERO_PERIOD
    } else {
        noise_factor as u16 * 64
    }
}

impl BaseBusDevices for HuC6280Psg {
    #[inline]
    fn read_psg(&mut self, port: PsgPort) -> u8 {
        self.read_port(port)
    }

    #[inline]
    fn write_psg(&mut self, port: PsgPort, value: u8) {
        self.write_port(port, value);
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ATTENUATION_GAIN, BLIP_FRAME_CLOCKS, DETERMINISTIC_PSG_RESET_ATTENUATION_SLOT, HuC6280Psg,
        MAX_PSG_STATE_AUDIO_SAMPLES, MAX_PSG_STATE_SECTION_BYTES, MIX_SCALE,
        PROVISIONAL_PSG_GAIN_SCAN_CLOCKS_PER_PASS, PROVISIONAL_PSG_NOISE_ZERO_PERIOD,
        PSG_CHANNEL_COUNT, PSG_CLOCK_DENOMINATOR, PSG_INTERNAL_CLOCK_NUMERATOR,
        PSG_INTERNAL_MASTER_CLOCK_DIVISOR, PsgRevision, StereoBlipResampler, attenuation_slot,
        blip_level, effective_period, lfo_target_period,
    };
    use crate::hardware::PsgPort;
    use zeff_emu_common::save_state::{StateReader, StateWriter};

    fn phase_state(psg: &HuC6280Psg) -> [(u8, i32, u16, u32); 6] {
        std::array::from_fn(|index| {
            let channel = &psg.channels[index];
            (
                channel.wave_index,
                channel.wave_counter,
                channel.noise_counter,
                channel.noise_seed,
            )
        })
    }

    fn seed_effective_gains(psg: &mut HuC6280Psg) {
        for channel in &mut psg.channels {
            channel.effective_left_attenuation = attenuation_slot(
                psg.main_amplitude >> 4,
                channel.amplitude(),
                channel.balance >> 4,
            );
            channel.effective_right_attenuation =
                attenuation_slot(psg.main_amplitude, channel.amplitude(), channel.balance);
        }
        psg.gain_scan_clock = 0;
        psg.gain_scan_active = false;
        psg.gain_scan_queued = false;
        psg.resampler = psg.resampler_at_current_level();
    }

    #[test]
    fn lfo_signed_source_and_target_clamps_are_bounded() {
        assert_eq!(lfo_target_period(100, 0x10, 2), 100);
        assert_eq!(lfo_target_period(100, 0x0F, 2), 96);
        assert_eq!(lfo_target_period(100, 0x00, 2), 36);
        assert_eq!(lfo_target_period(1, 0x00, 3), 1);
        assert_eq!(lfo_target_period(0x1FFF, 0x1F, 3), 0x1FFF);
    }

    #[test]
    fn attenuation_components_follow_exact_slots_and_cutoff() {
        assert_eq!(ATTENUATION_GAIN[30], 0);
        assert_eq!(ATTENUATION_GAIN[31], 0);
        for (slot, gain) in ATTENUATION_GAIN.iter().enumerate().take(30) {
            let expected = (1_000_000.0 * 10_f64.powf(-1.5 * slot as f64 / 20.0)).round();
            assert_eq!(f64::from(*gain), expected);
        }

        for main in 0..16 {
            for channel in 0..32 {
                for balance in 0..16 {
                    let expected = (2 * (15 - main) + (31 - channel) + 2 * (15 - balance)).min(31);
                    let slot = attenuation_slot(main, channel, balance);
                    assert_eq!(slot, expected);
                    assert_eq!(
                        ATTENUATION_GAIN[usize::from(slot)] == 0,
                        main == 0 || channel == 0 || balance == 0 || expected >= 30
                    );
                }
            }
        }

        assert_eq!(attenuation_slot(0xC, 0x1C, 0xF), 9);
        assert_eq!(attenuation_slot(0x8, 0x1C, 0x8), 31);
        assert_eq!(attenuation_slot(0xC, 0x1D, 0xF), 8);
        assert_eq!(attenuation_slot(0x8, 0x1D, 0x8), 30);
    }

    #[test]
    fn provisional_gain_scan_applies_right_then_left_across_six_live_channels() {
        let mut psg = HuC6280Psg::new();
        psg.main_amplitude = 0xFF;
        for channel in &mut psg.channels {
            channel.control = 0x1F;
            channel.balance = 0xFF;
        }
        psg.queue_gain_scan();

        for component in 0..PSG_CHANNEL_COUNT * 2 {
            psg.advance_master_ticks(255 * PSG_INTERNAL_MASTER_CLOCK_DIVISOR);
            let channel = component / 2;
            if component & 1 == 0 {
                assert_eq!(
                    psg.channels[channel].effective_right_attenuation,
                    DETERMINISTIC_PSG_RESET_ATTENUATION_SLOT
                );
            } else {
                assert_eq!(
                    psg.channels[channel].effective_left_attenuation,
                    DETERMINISTIC_PSG_RESET_ATTENUATION_SLOT
                );
            }
            psg.advance_master_ticks(PSG_INTERNAL_MASTER_CLOCK_DIVISOR);
            if component & 1 == 0 {
                assert_eq!(psg.channels[channel].effective_right_attenuation, 0);
            } else {
                assert_eq!(psg.channels[channel].effective_left_attenuation, 0);
            }
        }

        assert_eq!(psg.gain_scan_clock, 3_072);
        assert!(psg.gain_scan_active);
        psg.advance_master_ticks(1_024 * PSG_INTERNAL_MASTER_CLOCK_DIVISOR);
        assert_eq!(psg.gain_scan_clock, 0);
        assert!(!psg.gain_scan_active);
    }

    #[test]
    fn gain_scan_latch_is_stable_and_midpass_writes_coalesce_one_followup_pass() {
        let mut psg = HuC6280Psg::new();
        psg.main_amplitude = 0xFF;
        psg.channels[0].control = 0x1F;
        psg.channels[0].balance = 0xFF;
        psg.queue_gain_scan();
        psg.advance_master_ticks(PSG_INTERNAL_MASTER_CLOCK_DIVISOR);
        assert_eq!(psg.attenuation_latch, 0);

        psg.write_port(PsgPort::from_offset(1), 0xF0);
        psg.write_port(PsgPort::from_offset(1), 0xF0);
        psg.write_port(PsgPort::from_offset(5), 0xFF);
        assert!(psg.gain_scan_queued);
        psg.advance_master_ticks(255 * PSG_INTERNAL_MASTER_CLOCK_DIVISOR);
        assert_eq!(psg.channels[0].effective_right_attenuation, 0);

        psg.advance_master_ticks(3_840 * PSG_INTERNAL_MASTER_CLOCK_DIVISOR);
        assert!(psg.gain_scan_active);
        assert!(!psg.gain_scan_queued);
        assert_eq!(psg.gain_scan_clock, 0);
        psg.advance_master_ticks(PSG_INTERNAL_MASTER_CLOCK_DIVISOR);
        assert_eq!(psg.attenuation_latch, 30);
        psg.advance_master_ticks(255 * PSG_INTERNAL_MASTER_CLOCK_DIVISOR);
        assert_eq!(psg.channels[0].effective_right_attenuation, 30);
        psg.advance_master_ticks(3_840 * PSG_INTERNAL_MASTER_CLOCK_DIVISOR);
        assert!(!psg.gain_scan_active);

        psg.write_port(PsgPort::from_offset(1), 0xF0);
        assert!(psg.gain_scan_active);
        assert_eq!(psg.gain_scan_clock, 0);
    }

    #[test]
    fn gain_apply_after_a_drain_uses_the_end_of_the_next_internal_clock() {
        let mut psg = HuC6280Psg::new();
        psg.set_sample_rate(192_000);
        psg.main_amplitude = 0xFF;
        psg.channels[0].control = 0xDF;
        psg.channels[0].balance = 0xFF;
        psg.channels[0].dda_hold = 31;
        psg.queue_gain_scan();

        psg.advance_master_ticks(255 * PSG_INTERNAL_MASTER_CLOCK_DIVISOR);
        let mut samples = Vec::new();
        psg.drain_audio_samples_into(&mut samples);
        assert!(samples.iter().all(|sample| *sample == 0.0));
        assert_eq!(psg.resampler_clock(), 0);
        assert_eq!(psg.channels[0].effective_right_attenuation, 31);

        psg.advance_master_ticks(PSG_INTERNAL_MASTER_CLOCK_DIVISOR);
        assert_eq!(psg.channels[0].effective_right_attenuation, 0);
        assert_eq!(psg.channels[0].effective_left_attenuation, 31);
        assert_eq!(psg.resampler_clock(), 1);
        assert_eq!(psg.resampler.last_delta_clock, Some(1));
    }

    #[test]
    fn level_edge_at_blip_frame_boundary_starts_the_next_frame() {
        let mut resampler = StereoBlipResampler::new(192_000);
        let mut samples = Vec::new();
        resampler.clocks = BLIP_FRAME_CLOCKS - 1;

        resampler.push_level(MIX_SCALE, -MIX_SCALE, &mut samples);

        assert_eq!(resampler.clocks, 0);
        assert_eq!(resampler.last_delta_clock, Some(0));
        assert_ne!(resampler.left_level, 0);
        assert_ne!(resampler.right_level, 0);
        assert!(!samples.is_empty());

        resampler.push_level(MIX_SCALE, -MIX_SCALE, &mut samples);
        assert_eq!(resampler.clocks, 1);
        assert_eq!(resampler.last_delta_clock, Some(0));
        resampler.flush(&mut samples);
    }

    #[test]
    fn resampler_state_size_stays_fixed_across_multi_hour_clock_history() {
        const SIMULATED_SECONDS: u64 = 3 * 60 * 60;
        let frame_count = (SIMULATED_SECONDS * PSG_INTERNAL_CLOCK_NUMERATOR)
            .div_ceil(u64::from(BLIP_FRAME_CLOCKS) * PSG_CLOCK_DENOMINATOR);
        let mut resampler = StereoBlipResampler::new(1);
        let sample_cells = (
            resampler.left.state_sample_count(),
            resampler.right.state_sample_count(),
        );
        let mut initial_writer = StateWriter::new();
        resampler.write_state(&mut initial_writer);
        let initial_len = initial_writer.position();
        let mut output = Vec::new();

        for frame in 0..frame_count {
            resampler.clocks = BLIP_FRAME_CLOCKS - 1;
            let level = if frame & 1 == 0 {
                MIX_SCALE
            } else {
                -MIX_SCALE
            };
            resampler.push_level(level, -level, &mut output);
            output.clear();
        }

        let mut final_writer = StateWriter::new();
        resampler.write_state(&mut final_writer);
        assert_eq!(final_writer.position(), initial_len);
        assert_eq!(
            (
                resampler.left.state_sample_count(),
                resampler.right.state_sample_count()
            ),
            sample_cells
        );
    }

    #[test]
    fn resampler_state_rejects_malformed_fixed_buffer_metadata() {
        let resampler = StereoBlipResampler::new(44_100);
        let mut writer = StateWriter::new();
        resampler.write_state(&mut writer);
        let bytes = writer.into_bytes();

        let mut bad_clock = bytes.clone();
        bad_clock[..4].copy_from_slice(&BLIP_FRAME_CLOCKS.to_le_bytes());
        assert!(
            StereoBlipResampler::read_state(44_100, &mut StateReader::new(&bad_clock)).is_err()
        );

        let mut bad_factor = bytes.clone();
        bad_factor[12..20].copy_from_slice(&0_u64.to_le_bytes());
        assert!(
            StereoBlipResampler::read_state(44_100, &mut StateReader::new(&bad_factor))
                .unwrap_err()
                .to_string()
                .contains("factor")
        );

        let mut oversized = bytes.clone();
        oversized[36..40].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(
            StereoBlipResampler::read_state(44_100, &mut StateReader::new(&oversized))
                .unwrap_err()
                .to_string()
                .contains("buffer length")
        );

        let mut bad_available = bytes.clone();
        bad_available[32..36].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(
            StereoBlipResampler::read_state(44_100, &mut StateReader::new(&bad_available))
                .unwrap_err()
                .to_string()
                .contains("available-sample")
        );

        let left_cells = u32::from_le_bytes(bytes[36..40].try_into().unwrap()) as usize;
        let right_offset = 12 + 28 + left_cells * 4 + 8;
        let mut mismatched_stereo = bytes.clone();
        mismatched_stereo[right_offset..right_offset + 8].copy_from_slice(&0_u64.to_le_bytes());
        assert!(
            StereoBlipResampler::read_state(44_100, &mut StateReader::new(&mismatched_stereo))
                .unwrap_err()
                .to_string()
                .contains("differs between channels")
        );

        let mut truncated = bytes;
        truncated.pop();
        assert!(
            StereoBlipResampler::read_state(44_100, &mut StateReader::new(&truncated)).is_err()
        );
    }

    #[test]
    fn psg_state_rejects_oversized_queued_pcm() {
        let mut psg = HuC6280Psg::new();
        psg.set_sample_rate(192_000);
        psg.audio_samples.resize(MAX_PSG_STATE_AUDIO_SAMPLES, 0);
        let mut writer = StateWriter::new();
        psg.write_state(&mut writer);
        assert!(writer.position() <= MAX_PSG_STATE_SECTION_BYTES);

        psg.audio_samples.resize(MAX_PSG_STATE_AUDIO_SAMPLES + 2, 0);
        assert!(
            psg.validate_v1_state()
                .unwrap_err()
                .to_string()
                .contains("queued audio")
        );
    }

    #[test]
    fn rate_change_preserves_a_sampled_gain_latch_midpass() {
        for sample_rate in [44_100, 48_000, 96_000, 192_000] {
            let mut psg = HuC6280Psg::new();
            psg.main_amplitude = 0xFF;
            psg.channels[0].control = 0x1F;
            psg.channels[0].balance = 0xFF;
            psg.queue_gain_scan();
            psg.advance_master_ticks(PSG_INTERNAL_MASTER_CLOCK_DIVISOR);
            assert_eq!(psg.attenuation_latch, 0);

            psg.set_sample_rate(sample_rate);
            assert_eq!(psg.gain_scan_clock, 1);
            assert_eq!(psg.attenuation_latch, 0);
            psg.write_port(PsgPort::from_offset(1), 0xF0);
            psg.advance_master_ticks(255 * PSG_INTERNAL_MASTER_CLOCK_DIVISOR);
            assert_eq!(psg.channels[0].effective_right_attenuation, 0);
        }
    }

    #[test]
    fn key_on_with_new_level_uses_old_gain_until_each_scan_apply() {
        for revision in [PsgRevision::HuC6280, PsgRevision::HuC6280A] {
            let mut psg = HuC6280Psg::with_revision(revision);
            psg.main_amplitude = 0xFF;
            psg.channels[0].balance = 0xFF;
            psg.channels[0].dda_hold = 31;
            psg.write_port(PsgPort::from_offset(4), 0xDF);
            assert_eq!(psg.resampler_levels(), (0, 0));

            psg.advance_master_ticks(255 * PSG_INTERNAL_MASTER_CLOCK_DIVISOR);
            assert_eq!(psg.resampler_levels(), (0, 0));
            psg.advance_master_ticks(PSG_INTERNAL_MASTER_CLOCK_DIVISOR);
            assert_eq!(psg.channels[0].effective_right_attenuation, 0);
            assert_eq!(psg.channels[0].effective_left_attenuation, 31);
            assert_eq!(psg.resampler.left_level, 0);
            assert_ne!(psg.resampler.right_level, 0);

            psg.advance_master_ticks(255 * PSG_INTERNAL_MASTER_CLOCK_DIVISOR);
            assert_eq!(psg.resampler.left_level, 0);
            psg.advance_master_ticks(PSG_INTERNAL_MASTER_CLOCK_DIVISOR);
            assert_eq!(psg.channels[0].effective_left_attenuation, 0);
            assert_ne!(psg.resampler.left_level, 0);
        }
    }

    #[test]
    fn master_divide_three_clock_preserves_divide_six_oscillator_pitch() {
        let mut psg = HuC6280Psg::new();
        psg.channels[0].frequency = 1;
        psg.channels[0].control = 0x9F;
        psg.channels[0].wave_counter = 1;

        psg.advance_master_ticks(3);
        assert_eq!(psg.resampler_clock(), 1);
        assert_eq!(psg.channels[0].wave_index, 0);
        assert_eq!(psg.master_tick_remainder(), 3);
        psg.advance_master_ticks(3);
        assert_eq!(psg.resampler_clock(), 2);
        assert_eq!(psg.channels[0].wave_index, 1);
        assert_eq!(psg.master_tick_remainder(), 0);
    }

    #[test]
    fn disabled_generation_advances_gain_scan_and_chunking_is_equivalent() {
        fn configured(sample_rate: u32) -> HuC6280Psg {
            let mut psg = HuC6280Psg::new();
            psg.set_sample_rate(sample_rate);
            psg.channels[0].control = 0xDF;
            psg.channels[0].balance = 0xFF;
            psg.channels[0].dda_hold = 31;
            psg.set_sample_generation_enabled(false);
            psg.write_port(PsgPort::from_offset(1), 0xFF);
            psg
        }

        for sample_rate in [44_100, 48_000, 96_000, 192_000] {
            let mut psg = configured(sample_rate);
            psg.advance_master_ticks(
                u64::from(PROVISIONAL_PSG_GAIN_SCAN_CLOCKS_PER_PASS)
                    * PSG_INTERNAL_MASTER_CLOCK_DIVISOR,
            );
            assert_eq!(psg.channels[0].effective_left_attenuation, 0);
            assert_eq!(psg.channels[0].effective_right_attenuation, 0);
            psg.set_sample_generation_enabled(true);
            assert_ne!(psg.resampler_levels(), (0, 0));
            assert_eq!(psg.resampler_clock(), 0);
        }

        let total = u64::from(PROVISIONAL_PSG_GAIN_SCAN_CLOCKS_PER_PASS)
            * PSG_INTERNAL_MASTER_CLOCK_DIVISOR
            + 2;
        let mut bulk = configured(48_000);
        let mut chunked = configured(48_000);
        bulk.advance_master_ticks(total);
        let mut remaining = total;
        for chunk in [1_u64, 5, 17, 64, 511].into_iter().cycle() {
            if remaining == 0 {
                break;
            }
            let chunk = chunk.min(remaining);
            chunked.advance_master_ticks(chunk);
            remaining -= chunk;
        }
        assert_eq!(bulk.channels, chunked.channels);
        assert_eq!(bulk.gain_scan_clock, chunked.gain_scan_clock);
        assert_eq!(bulk.gain_scan_active, chunked.gain_scan_active);
        assert_eq!(bulk.gain_scan_queued, chunked.gain_scan_queued);
        assert_eq!(bulk.attenuation_latch, chunked.attenuation_latch);
        assert_eq!(bulk.master_tick_remainder, chunked.master_tick_remainder);
    }

    #[test]
    fn defined_noise_frequencies_advance_at_exact_periods() {
        for register in 0..31 {
            let noise_factor = register ^ 0x1F;
            let mut psg = HuC6280Psg::new();
            psg.sample_generation_enabled = false;
            psg.write_port(PsgPort::from_offset(0), 4);
            psg.write_port(PsgPort::from_offset(4), 0x80);
            psg.write_port(PsgPort::from_offset(7), 0x80 | register);

            psg.advance_psg_tick();
            let seed = psg.channels[4].noise_seed;
            for _ in 1..u16::from(noise_factor) * 64 {
                psg.advance_psg_tick();
                assert_eq!(
                    psg.channels[4].noise_seed, seed,
                    "R7={register:02x}, NF={noise_factor}"
                );
            }
            psg.advance_psg_tick();
            assert_ne!(
                psg.channels[4].noise_seed, seed,
                "R7={register:02x}, NF={noise_factor}"
            );
        }
    }

    #[test]
    fn provisional_zero_noise_frequency_advances_every_tick() {
        assert_eq!(PROVISIONAL_PSG_NOISE_ZERO_PERIOD, 1);
        let mut psg = HuC6280Psg::new();
        psg.sample_generation_enabled = false;
        psg.write_port(PsgPort::from_offset(0), 4);
        psg.write_port(PsgPort::from_offset(4), 0x80);
        psg.write_port(PsgPort::from_offset(7), 0x9F);

        let mut seed = psg.channels[4].noise_seed;
        for _ in 0..8 {
            psg.advance_psg_tick();
            assert_ne!(psg.channels[4].noise_seed, seed);
            seed = psg.channels[4].noise_seed;
        }
    }

    #[test]
    fn noise_phase_free_runs_and_register_writes_change_only_the_next_reload() {
        let mut psg = HuC6280Psg::new();
        psg.sample_generation_enabled = false;
        let channel = &psg.channels[4];
        assert_eq!(channel.noise_counter, 0);
        assert_eq!(channel.noise_seed, 1);

        psg.advance_psg_tick();
        let seed = psg.channels[4].noise_seed;
        assert_ne!(seed, 1);
        assert_eq!(psg.channels[4].noise_counter, 31 * 64);

        psg.selected_channel = 4;
        for _ in 0..10 {
            psg.advance_psg_tick();
        }
        let counter = psg.channels[4].noise_counter;
        psg.write_port(PsgPort::from_offset(7), 0x9F);
        assert_eq!(psg.channels[4].noise_counter, counter);
        assert_eq!(psg.channels[4].noise_seed, seed);

        for _ in 1..counter {
            psg.advance_psg_tick();
            assert_eq!(psg.channels[4].noise_seed, seed);
        }
        psg.advance_psg_tick();
        assert_ne!(psg.channels[4].noise_seed, seed);
        assert_eq!(psg.channels[4].noise_counter, 1);
    }

    #[test]
    fn noise_key_and_enable_bits_gate_only_the_mixer() {
        let mut reference = HuC6280Psg::new();
        let mut toggled = HuC6280Psg::new();
        reference.sample_generation_enabled = false;
        toggled.sample_generation_enabled = false;
        toggled.selected_channel = 4;

        for step in 0..20_000 {
            if step % 97 == 0 {
                toggled.write_port(PsgPort::from_offset(4), 0x9F);
                toggled.write_port(PsgPort::from_offset(7), 0x80 | (step as u8 & 0x1F));
            } else if step % 53 == 0 {
                toggled.write_port(PsgPort::from_offset(4), 0);
                toggled.write_port(PsgPort::from_offset(7), step as u8 & 0x1F);
            }
            reference.channels[4].noise_control = toggled.channels[4].noise_control;
            reference.advance_psg_tick();
            toggled.advance_psg_tick();
            assert_eq!(
                toggled.channels[4].noise_counter,
                reference.channels[4].noise_counter
            );
            assert_eq!(
                toggled.channels[4].noise_seed,
                reference.channels[4].noise_seed
            );
        }
    }

    #[test]
    fn noise_lfsr_stays_within_its_nonzero_eighteen_bit_state() {
        let mut psg = HuC6280Psg::new();
        psg.sample_generation_enabled = false;
        psg.selected_channel = 4;
        psg.write_port(PsgPort::from_offset(7), 0x1F);
        for _ in 0..=0x3_FFFF {
            psg.advance_psg_tick();
            assert_ne!(psg.channels[4].noise_seed, 0);
            assert_eq!(psg.channels[4].noise_seed & !0x3_FFFF, 0);
        }
    }

    #[test]
    fn active_dda_rate_change_and_generation_resume_seed_the_live_level() {
        for sample_rate in [44_100, 48_000, 96_000, 192_000] {
            let mut psg = HuC6280Psg::new();
            psg.write_port(PsgPort::from_offset(0), 0);
            psg.write_port(PsgPort::from_offset(1), 0xFF);
            psg.write_port(PsgPort::from_offset(5), 0xFF);
            psg.write_port(PsgPort::from_offset(4), 0xDF);
            psg.write_port(PsgPort::from_offset(6), 31);
            psg.set_sample_generation_enabled(false);
            psg.advance_master_ticks(u64::from(PROVISIONAL_PSG_GAIN_SCAN_CLOCKS_PER_PASS) * 6);
            psg.set_sample_generation_enabled(true);

            psg.set_sample_rate(sample_rate);
            let (left, right) = psg.mix_output();
            assert_eq!(psg.resampler.left_level, blip_level(left));
            assert_eq!(psg.resampler.right_level, blip_level(right));
            psg.advance_master_ticks(100_000);
            let mut samples = Vec::new();
            psg.drain_audio_samples_into(&mut samples);
            assert!(samples.iter().all(|sample| *sample == 0.0));

            psg.set_sample_generation_enabled(false);
            psg.advance_master_ticks(100_000);
            psg.set_sample_generation_enabled(true);
            assert_eq!(psg.resampler.left_level, blip_level(left));
            assert_eq!(psg.resampler.right_level, blip_level(right));
            psg.advance_master_ticks(100_000);
            psg.drain_audio_samples_into(&mut samples);
            assert!(samples.iter().all(|sample| *sample == 0.0));
        }
    }

    #[test]
    fn output_register_writes_refresh_at_the_current_clock_without_advancing_phase() {
        for (revision, sample_rate) in [PsgRevision::HuC6280, PsgRevision::HuC6280A]
            .into_iter()
            .flat_map(|revision| {
                [44_100, 48_000, 96_000, 192_000]
                    .into_iter()
                    .map(move |sample_rate| (revision, sample_rate))
            })
        {
            let mut psg = HuC6280Psg::with_revision(revision);
            psg.set_sample_rate(sample_rate);
            psg.selected_channel = 0;
            psg.main_amplitude = 0xFF;
            psg.channels[0].balance = 0xFF;
            psg.channels[0].control = 0xDF;
            psg.channels[0].dda_hold = 8;
            seed_effective_gains(&mut psg);
            psg.advance_master_ticks(30);

            for (register, value) in [(1, 0xEF), (5, 0xEF), (4, 0xDD)] {
                let phase = phase_state(&psg);
                let clock = psg.resampler.clocks;
                let levels = psg.resampler_levels();
                psg.write_port(PsgPort::from_offset(register), value);
                assert_eq!(psg.resampler_levels(), levels);
                assert_eq!(psg.resampler.clocks, clock);
                assert_eq!(phase_state(&psg), phase);
            }

            let phase = phase_state(&psg);
            let clock = psg.resampler.clocks;
            let levels = psg.resampler_levels();
            psg.write_port(PsgPort::from_offset(6), 24);
            assert_ne!(psg.resampler_levels(), levels);
            assert_eq!(psg.resampler.clocks, clock);
            assert_eq!(phase_state(&psg), phase);

            psg.write_port(PsgPort::from_offset(4), 0x5F);
            let silent_level = (psg.resampler.left_level, psg.resampler.right_level);
            let phase = phase_state(&psg);
            psg.write_port(PsgPort::from_offset(6), 31);
            assert_eq!(psg.channels[0].dda_hold, 31);
            assert_eq!(psg.resampler.left_level, silent_level.0);
            assert_eq!(psg.resampler.right_level, silent_level.1);
            assert_eq!(phase_state(&psg), phase);

            let clock = psg.resampler.clocks;
            psg.write_port(PsgPort::from_offset(4), 0xDF);
            assert_ne!(psg.resampler.left_level, silent_level.0);
            assert_ne!(psg.resampler.right_level, silent_level.1);
            assert_eq!(psg.resampler.clocks, clock);
            psg.advance_master_ticks(6_000);
            let mut samples = Vec::new();
            psg.drain_audio_samples_into(&mut samples);
            assert!(samples.iter().any(|sample| sample.abs() > 0.001));
        }
    }

    #[test]
    fn mute_changes_refresh_immediately_without_advancing_channel_state() {
        for revision in [PsgRevision::HuC6280, PsgRevision::HuC6280A] {
            let mut psg = HuC6280Psg::with_revision(revision);
            psg.main_amplitude = 0xFF;
            psg.channels[0].balance = 0xFF;
            psg.channels[0].control = 0xDF;
            psg.channels[0].dda_hold = 31;
            seed_effective_gains(&mut psg);
            psg.advance_master_ticks(33);
            let audible = psg.resampler_levels();
            let phase = phase_state(&psg);
            let clock = psg.resampler_clock();

            psg.set_channel_mutes(&[true]);
            assert_eq!(psg.resampler_levels(), (0, 0));
            assert_eq!(psg.resampler_clock(), clock);
            assert_eq!(phase_state(&psg), phase);

            psg.set_channel_mutes(&[]);
            assert_eq!(psg.resampler_levels(), audible);
            assert_eq!(psg.resampler_clock(), clock);
            assert_eq!(phase_state(&psg), phase);

            psg.set_sample_generation_enabled(false);
            let phase = phase_state(&psg);
            psg.set_channel_mutes(&[true]);
            assert_eq!(psg.resampler_levels(), (0, 0));
            assert_eq!(psg.resampler_clock(), 0);
            assert_eq!(phase_state(&psg), phase);
            psg.set_sample_generation_enabled(true);
            assert_eq!(psg.resampler_levels(), (0, 0));
            psg.set_channel_mutes(&[]);
            assert_eq!(psg.resampler_levels(), audible);
            assert_eq!(psg.resampler_clock(), 0);
            assert_eq!(phase_state(&psg), phase);
        }
    }

    #[test]
    fn control_writes_switch_dda_wave_and_noise_sources_at_the_write_boundary() {
        for revision in [PsgRevision::HuC6280, PsgRevision::HuC6280A] {
            let mut psg = HuC6280Psg::with_revision(revision);
            psg.main_amplitude = 0xFF;
            psg.selected_channel = 0;
            psg.channels[0].balance = 0xFF;
            psg.channels[0].control = 0xDF;
            psg.channels[0].dda_hold = 31;
            psg.channels[0].waveform[0] = 4;
            seed_effective_gains(&mut psg);
            let dda_levels = psg.resampler_levels();
            let clock = psg.resampler_clock();

            psg.write_port(PsgPort::from_offset(4), 0x9F);
            let wave_levels = psg.resampler_levels();
            assert_ne!(wave_levels, dda_levels);
            assert_eq!(psg.resampler_clock(), clock);
            psg.write_port(PsgPort::from_offset(4), 0xDF);
            assert_eq!(psg.resampler_levels(), dda_levels);
            assert_eq!(psg.resampler_clock(), clock);

            psg.selected_channel = 4;
            psg.channels[4].control = 0xDF;
            psg.channels[4].balance = 0xFF;
            psg.channels[4].dda_hold = 3;
            psg.channels[4].noise_control = 0x80;
            psg.resampler = psg.resampler_at_current_level();
            let noise_levels = psg.resampler_levels();
            let phase = phase_state(&psg);
            psg.write_port(PsgPort::from_offset(6), 29);
            assert_eq!(psg.channels[4].dda_hold, 29);
            assert_eq!(psg.resampler_levels(), noise_levels);
            assert_eq!(phase_state(&psg), phase);
        }
    }

    #[test]
    fn same_value_writes_preserve_mix_but_apply_defined_register_side_effects() {
        for sample_rate in [44_100, 48_000, 96_000, 192_000] {
            let mut psg = HuC6280Psg::new();
            psg.set_sample_rate(sample_rate);
            psg.main_amplitude = 0xFF;
            psg.channels[0].balance = 0xFF;
            psg.channels[0].control = 0xDF;
            psg.channels[0].dda_hold = 31;
            psg.channels[0].wave_index = 7;
            psg.selected_channel = 0;
            psg.resampler = psg.resampler_at_current_level();
            let levels = psg.resampler_levels();
            psg.write_port(PsgPort::from_offset(4), 0xDF);
            assert_eq!(psg.channels[0].wave_index, 0);
            assert_eq!(psg.resampler_levels(), levels);

            psg.selected_channel = 4;
            psg.channels[4].noise_control = 0x80;
            psg.channels[4].noise_counter = 100;
            let noise_state = (psg.channels[4].noise_counter, psg.channels[4].noise_seed);
            psg.write_port(PsgPort::from_offset(7), 0x80);
            assert_eq!(
                (psg.channels[4].noise_counter, psg.channels[4].noise_seed,),
                noise_state
            );
            assert_eq!(psg.resampler_levels(), levels);

            psg.channels[1].control = 0x9F;
            psg.channels[1].balance = 0xFF;
            psg.channels[1].wave_index = 5;
            psg.channels[1].waveform[5] = 31;
            psg.channels[1].waveform[0] = 31;
            psg.lfo_control = 0x80;
            psg.resampler = psg.resampler_at_current_level();
            let levels = psg.resampler_levels();
            psg.write_port(PsgPort::from_offset(9), 0x80);
            assert_eq!(psg.channels[1].wave_index, 0);
            assert_eq!(psg.resampler_levels(), levels);

            psg.advance_master_ticks(60);
            let mut samples = Vec::new();
            psg.drain_audio_samples_into(&mut samples);
            assert!(samples.iter().all(|sample| *sample == 0.0));
        }
    }

    #[test]
    fn low_lfo_trigger_activation_resets_the_audible_source_index() {
        for revision in [PsgRevision::HuC6280, PsgRevision::HuC6280A] {
            let mut psg = HuC6280Psg::with_revision(revision);
            psg.main_amplitude = 0xFF;
            for channel in 0..=1 {
                psg.channels[channel].control = 0x9F;
                psg.channels[channel].balance = 0xFF;
            }
            psg.channels[1].wave_index = 6;
            psg.channels[1].waveform[6] = 31;
            psg.channels[1].waveform[0] = 0;
            seed_effective_gains(&mut psg);
            let before = psg.resampler_levels();
            let clock = psg.resampler_clock();

            psg.write_port(PsgPort::from_offset(9), 1);
            assert!(psg.lfo_active());
            assert_eq!(psg.channels[1].wave_index, 0);
            assert_ne!(psg.resampler_levels(), before);
            assert_eq!(psg.resampler_clock(), clock);
        }
    }

    #[test]
    fn noise_and_lfo_source_writes_refresh_without_advancing_unrelated_state() {
        for sample_rate in [44_100, 48_000, 96_000, 192_000] {
            let mut psg = HuC6280Psg::new();
            psg.set_sample_rate(sample_rate);
            psg.main_amplitude = 0xFF;
            psg.selected_channel = 4;
            psg.channels[4].control = 0x9F;
            psg.channels[4].balance = 0xFF;
            seed_effective_gains(&mut psg);

            let phase = phase_state(&psg);
            let clock = psg.resampler.clocks;
            psg.write_port(PsgPort::from_offset(7), 0x80);
            assert_eq!(phase_state(&psg), phase);
            assert_eq!(psg.resampler.clocks, clock);
            let noise_levels = (psg.resampler.left_level, psg.resampler.right_level);
            assert_ne!(noise_levels, (0, 0));

            psg.write_port(PsgPort::from_offset(7), 0x81);
            assert_eq!(phase_state(&psg), phase);
            assert_eq!(
                (psg.resampler.left_level, psg.resampler.right_level),
                noise_levels
            );
            psg.write_port(PsgPort::from_offset(7), 0x01);
            assert_eq!(phase_state(&psg), phase);
            assert_eq!(
                (psg.resampler.left_level, psg.resampler.right_level),
                (0, 0)
            );

            psg.selected_channel = 1;
            psg.channels[1].control = 0x9F;
            psg.channels[1].balance = 0xFF;
            psg.channels[1].wave_index = 5;
            psg.channels[1].waveform[5] = 31;
            psg.channels[1].waveform[0] = 0;
            seed_effective_gains(&mut psg);
            let noise_phase = [
                (psg.channels[4].noise_counter, psg.channels[4].noise_seed),
                (psg.channels[5].noise_counter, psg.channels[5].noise_seed),
            ];
            let clock = psg.resampler.clocks;
            psg.write_port(PsgPort::from_offset(9), 0x80);
            assert_eq!(psg.channels[1].wave_index, 0);
            assert_eq!(psg.resampler.clocks, clock);
            assert_eq!(
                [
                    (psg.channels[4].noise_counter, psg.channels[4].noise_seed),
                    (psg.channels[5].noise_counter, psg.channels[5].noise_seed),
                ],
                noise_phase
            );
            assert_eq!(
                (psg.resampler.left_level, psg.resampler.right_level),
                (0, 0)
            );
        }
    }

    #[test]
    fn waveform_programming_and_keyed_non_dda_writes_follow_distinct_paths() {
        for (revision, sample_rate) in [PsgRevision::HuC6280, PsgRevision::HuC6280A]
            .into_iter()
            .flat_map(|revision| {
                [44_100, 48_000, 96_000, 192_000]
                    .into_iter()
                    .map(move |sample_rate| (revision, sample_rate))
            })
        {
            let mut psg = HuC6280Psg::with_revision(revision);
            psg.set_sample_rate(sample_rate);
            psg.write_port(PsgPort::from_offset(6), 31);
            assert_eq!(
                (psg.resampler.left_level, psg.resampler.right_level),
                (0, 0)
            );
            assert_eq!(psg.resampler.clocks, 0);

            psg.main_amplitude = 0xFF;
            psg.channels[0].balance = 0xFF;
            psg.channels[0].control = 0x9F;
            psg.channels[0].waveform[psg.channels[0].wave_index as usize] = 31;
            seed_effective_gains(&mut psg);
            let index = psg.channels[0].wave_index;
            let counter = psg.channels[0].wave_counter;
            let levels = (psg.resampler.left_level, psg.resampler.right_level);
            psg.write_port(PsgPort::from_offset(6), 0);
            assert_eq!(psg.channels[0].waveform[usize::from(index)], 0);
            assert_eq!(psg.channels[0].wave_index, index);
            assert_eq!(psg.channels[0].wave_counter, counter);
            assert_ne!(psg.resampler_levels(), levels);
            assert_eq!(psg.resampler.clocks, 0);

            psg.selected_channel = 4;
            psg.channels[4].control = 0x9F;
            psg.channels[4].balance = 0xFF;
            psg.channels[4].noise_control = 0x80;
            psg.channels[4].wave_index = 7;
            psg.channels[4].wave_counter = 123;
            seed_effective_gains(&mut psg);
            let levels = psg.resampler_levels();
            psg.write_port(PsgPort::from_offset(6), 29);
            assert_eq!(psg.channels[4].waveform[7], 29);
            assert_eq!(psg.channels[4].wave_index, 7);
            assert_eq!(psg.channels[4].wave_counter, 123);
            assert_eq!(psg.resampler_levels(), levels);
        }
    }

    #[test]
    fn keyed_wave_write_holds_until_the_exact_next_oscillator_clock() {
        for revision in [PsgRevision::HuC6280, PsgRevision::HuC6280A] {
            let mut psg = HuC6280Psg::with_revision(revision);
            psg.channels[0].control = 0x9F;
            psg.channels[0].balance = 0xFF;
            psg.channels[0].effective_left_attenuation = 0;
            psg.channels[0].effective_right_attenuation = 0;
            psg.channels[0].wave_index = 4;
            psg.channels[0].wave_counter = 1;
            psg.channels[0].waveform[4] = 3;
            psg.channels[0].waveform[5] = 11;
            psg.resampler = psg.resampler_at_current_level();

            psg.write_port(PsgPort::from_offset(6), 29);
            assert_eq!(psg.channels[0].waveform[4], 29);
            assert_eq!(psg.channels[0].wave_index, 4);
            assert_eq!(psg.channels[0].wave_counter, 1);
            assert_eq!(psg.resampler.last_delta_clock, Some(0));

            psg.advance_master_ticks(PSG_INTERNAL_MASTER_CLOCK_DIVISOR);
            assert_eq!(psg.channels[0].wave_index, 4);
            assert_eq!(psg.resampler.last_delta_clock, Some(0));
            psg.advance_master_ticks(PSG_INTERNAL_MASTER_CLOCK_DIVISOR);
            assert_eq!(psg.channels[0].wave_index, 5);
            assert_eq!(psg.channels[0].waveform[5], 11);
            assert_eq!(psg.resampler.last_delta_clock, Some(2));
        }
    }

    #[test]
    fn waveform_rate_change_and_generation_resume_emit_only_real_transitions() {
        for sample_rate in [44_100, 48_000, 96_000, 192_000] {
            let mut psg = HuC6280Psg::with_revision(PsgRevision::HuC6280A);
            psg.write_port(PsgPort::from_offset(0), 0);
            psg.write_port(PsgPort::from_offset(1), 0xFF);
            psg.write_port(PsgPort::from_offset(2), 0);
            psg.write_port(PsgPort::from_offset(3), 8);
            psg.write_port(PsgPort::from_offset(5), 0xFF);
            psg.write_port(PsgPort::from_offset(6), 0);
            psg.write_port(PsgPort::from_offset(6), 31);
            for _ in 2..32 {
                psg.write_port(PsgPort::from_offset(6), 0);
            }
            psg.write_port(PsgPort::from_offset(4), 0x9F);
            psg.set_sample_generation_enabled(false);
            psg.advance_master_ticks(u64::from(PROVISIONAL_PSG_GAIN_SCAN_CLOCKS_PER_PASS) * 6);
            psg.channels[0].wave_index = 0;
            psg.channels[0].wave_counter = effective_period(psg.channels[0].frequency);
            psg.set_sample_generation_enabled(true);

            psg.set_sample_rate(sample_rate);
            let (left, right) = psg.mix_output();
            assert_eq!(psg.resampler.left_level, blip_level(left));
            assert_eq!(psg.resampler.right_level, blip_level(right));
            psg.advance_master_ticks(2_047 * 6);
            let mut samples = Vec::new();
            psg.drain_audio_samples_into(&mut samples);
            assert!(samples.iter().all(|sample| *sample == 0.0));
            psg.advance_master_ticks(1_500 * 6);
            psg.drain_audio_samples_into(&mut samples);
            assert!(samples.iter().any(|sample| sample.abs() > 0.001));

            psg.set_sample_generation_enabled(false);
            psg.advance_master_ticks(500 * 6);
            psg.set_sample_generation_enabled(true);
            let (left, right) = psg.mix_output();
            assert_eq!(psg.resampler.left_level, blip_level(left));
            assert_eq!(psg.resampler.right_level, blip_level(right));
            psg.advance_master_ticks(47 * 6);
            psg.drain_audio_samples_into(&mut samples);
            assert!(samples.iter().all(|sample| *sample == 0.0));
            psg.advance_master_ticks(1_000 * 6);
            psg.drain_audio_samples_into(&mut samples);
            assert!(samples.iter().any(|sample| sample.abs() > 0.001));
        }
    }
}
