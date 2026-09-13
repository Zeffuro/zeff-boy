use super::{AudioTraceChip, ChipAudioTrace, ChipAudioTraceRecorder};

pub type WonderSwanAudioTrace = ChipAudioTrace<WonderSwanTraceChip, WonderSwanTraceWrite>;
pub type WonderSwanAudioTraceRecorder =
    ChipAudioTraceRecorder<WonderSwanTraceChip, WonderSwanTraceWrite>;

#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
pub enum WonderSwanTraceOrigin {
    Cpu,
    CpuInterrupt,
    GeneralDma,
    SoundDma,
}

#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
pub enum WonderSwanTraceWrite {
    Register {
        port: u16,
        value: u8,
        origin: WonderSwanTraceOrigin,
    },
    WaveRam {
        address: u16,
        value: u8,
        origin: WonderSwanTraceOrigin,
    },
}

#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WonderSwanTraceChip {
    pub clock_hz: u32,
    pub color: bool,
    pub reset: WonderSwanResetState,
}

impl AudioTraceChip for WonderSwanTraceChip {
    fn clock_numerator_hz(&self) -> u64 {
        u64::from(self.clock_hz)
    }

    fn clock_denominator(&self) -> u32 {
        1
    }
}

#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WonderSwanResetState {
    pub registers: [u8; 28],
    pub hyper_voice_registers: [u8; 8],
    pub wave_ram: Vec<u8>,
    pub period_counters: [i32; 4],
    pub sample_positions: [u8; 4],
    pub sweep_divider: i32,
    pub sweep_counter: u8,
    pub hyper_voice_next_left: bool,
}

impl Default for WonderSwanResetState {
    fn default() -> Self {
        Self {
            registers: [0; 28],
            hyper_voice_registers: [0; 8],
            wave_ram: vec![0; 0x4000],
            period_counters: [1; 4],
            sample_positions: [0; 4],
            sweep_divider: 8192,
            sweep_counter: 0,
            hyper_voice_next_left: true,
        }
    }
}
