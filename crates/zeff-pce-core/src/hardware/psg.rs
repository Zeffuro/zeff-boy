use super::blip_buf::BlipBuf;
use super::bus::{BaseBusDevices, OPEN_BUS_VALUE, PsgPort};
use super::constants::{
    PCE_DEFAULT_AUDIO_SAMPLE_RATE_HZ, PCE_NTSC_COLORBURST_CLOCK_HZ_DENOMINATOR,
    PCE_NTSC_COLORBURST_CLOCK_HZ_NUMERATOR,
};
use anyhow::bail;
use std::fmt;
use zeff_emu_common::save_state::{StateReader, StateWriter};

mod runtime;
mod state;

#[cfg(test)]
#[path = "psg/tests.rs"]
mod tests;

pub const PSG_CHANNEL_COUNT: usize = 6;
pub const PSG_WAVEFORM_WORDS: usize = 32;
pub const PSG_MASTER_CLOCK_DIVISOR: u64 = 6;
pub const PSG_INTERNAL_MASTER_CLOCK_DIVISOR: u64 = 3;
pub const PSG_CLOCK_NUMERATOR: u64 = PCE_NTSC_COLORBURST_CLOCK_HZ_NUMERATOR;
pub const PSG_CLOCK_DENOMINATOR: u64 = PCE_NTSC_COLORBURST_CLOCK_HZ_DENOMINATOR;
pub const PSG_INTERNAL_CLOCK_NUMERATOR: u64 = PSG_CLOCK_NUMERATOR * 2;
pub const DEFAULT_PSG_SAMPLE_RATE: u32 = PCE_DEFAULT_AUDIO_SAMPLE_RATE_HZ;
pub const MAX_PSG_SAMPLE_RATE: u32 = 192_000;
pub const PSG_DEBUG_WAVEFORM_SAMPLE_COUNT: usize = 512;
pub const PSG_DEBUG_WAVEFORM_RATE_HZ: u32 = PCE_DEFAULT_AUDIO_SAMPLE_RATE_HZ;
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

    #[cfg(test)]
    fn push_level(&mut self, left: i64, right: i64, output: &mut Vec<i16>) {
        self.advance_clocks(1, output);
        self.refresh_level(left, right);
    }

    fn advance_clocks(&mut self, mut clocks: u64, output: &mut Vec<i16>) {
        while clocks != 0 {
            let step = clocks.min(u64::from(BLIP_FRAME_CLOCKS - self.clocks)) as u32;
            self.clocks += step;
            clocks -= u64::from(step);
            if self.clocks == BLIP_FRAME_CLOCKS {
                self.flush(output);
            }
        }
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
