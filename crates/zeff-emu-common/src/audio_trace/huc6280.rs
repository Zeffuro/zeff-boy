use super::{AudioTraceChip, ChipAudioTrace, ChipAudioTraceRecorder};

pub type Huc6280AudioTrace = ChipAudioTrace<Huc6280TraceChip, Huc6280TraceWrite>;
pub type Huc6280AudioTraceRecorder = ChipAudioTraceRecorder<Huc6280TraceChip, Huc6280TraceWrite>;

#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Huc6280TraceWrite {
    pub physical_address: u32,
    pub register: u8,
    pub value: u8,
}

#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
pub enum Huc6280TraceRevision {
    HuC6280,
    HuC6280A,
}

#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Huc6280TraceChip {
    pub clock_hz_numerator: u64,
    pub clock_hz_denominator: u32,
    pub master_clock_divisor: u8,
    pub internal_master_clock_divisor: u8,
    pub revision: Huc6280TraceRevision,
    pub reset: Huc6280ResetState,
}

impl AudioTraceChip for Huc6280TraceChip {
    fn clock_numerator_hz(&self) -> u64 {
        self.clock_hz_numerator
    }

    fn clock_denominator(&self) -> u32 {
        self.clock_hz_denominator
    }
}

#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Huc6280ResetState {
    pub channels: [Huc6280ChannelResetState; 6],
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
}

impl Default for Huc6280ResetState {
    fn default() -> Self {
        Self {
            channels: [Huc6280ChannelResetState::default(); 6],
            selected_channel: 0,
            main_amplitude: 0,
            lfo_frequency: 0,
            lfo_control: 0,
            lfo_counter: 0,
            lfo_phase_valid: false,
            gain_scan_clock: 0,
            gain_scan_active: false,
            gain_scan_queued: false,
            attenuation_latch: 31,
            master_tick_remainder: 0,
        }
    }
}

#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Huc6280ChannelResetState {
    pub frequency: u16,
    pub control: u8,
    pub balance: u8,
    pub waveform: [u8; 32],
    pub wave_index: u8,
    pub dda_hold: u8,
    pub noise_control: u8,
    pub wave_counter: i32,
    pub noise_counter: u16,
    pub noise_seed: u32,
    pub effective_left_attenuation: u8,
    pub effective_right_attenuation: u8,
}

impl Default for Huc6280ChannelResetState {
    fn default() -> Self {
        Self {
            frequency: 0,
            control: 0,
            balance: 0,
            waveform: [0; 32],
            wave_index: 0,
            dda_hold: 0,
            noise_control: 0,
            wave_counter: 4096,
            noise_counter: 0,
            noise_seed: 1,
            effective_left_attenuation: 31,
            effective_right_attenuation: 31,
        }
    }
}
