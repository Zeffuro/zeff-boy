use super::{AudioTraceChip, ChipAudioTrace, ChipAudioTraceRecorder};

pub const GBA_AUDIO_TRACE_CLOCK_HZ: u32 = 16_777_216;

pub type GbaAudioTrace = ChipAudioTrace<GbaAudioTraceChip, GbaAudioTraceWrite>;
pub type GbaAudioTraceRecorder = ChipAudioTraceRecorder<GbaAudioTraceChip, GbaAudioTraceWrite>;

#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
pub enum GbaAudioTraceReset {
    PostBiosV1,
}

#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GbaAudioTraceChip {
    pub clock_hz: u32,
    pub reset: GbaAudioTraceReset,
    pub source_sha256: [u8; 32],
}

impl AudioTraceChip for GbaAudioTraceChip {
    fn clock_numerator_hz(&self) -> u64 {
        u64::from(self.clock_hz)
    }

    fn clock_denominator(&self) -> u32 {
        1
    }
}

#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
pub enum GbaDirectSoundFifo {
    A,
    B,
}

#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
pub enum GbaAudioTraceDmaKind {
    Normal,
    Fifo,
}

#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
pub enum GbaAudioTraceSource {
    Rom { offset: u32 },
    Ewram { offset: u32 },
    Iwram { offset: u32 },
    Bios { offset: u32 },
    Latch,
    Unknown { address: u32 },
}

#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GbaAudioTraceDma {
    pub channel: u8,
    pub kind: GbaAudioTraceDmaKind,
    pub requested_source: u32,
    pub aligned_source: u32,
    pub width: u8,
    pub value: u32,
    pub source_latched: bool,
    pub source_lanes: [GbaAudioTraceSource; 4],
}

#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
pub enum GbaAudioTraceOrigin {
    Cpu { active_pc: u32 },
    CpuNonInstruction,
    Dma(GbaAudioTraceDma),
    Timer { timer: u8 },
    Unknown,
}

#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GbaAudioTraceAccess {
    pub address: u32,
    pub width: u8,
    pub halfword_lane: u8,
}

#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GbaAudioTraceFifoState {
    pub queue: [i8; 32],
    pub len: u8,
    pub current: i8,
}

#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
pub enum GbaAudioTraceWrite {
    Control {
        address: u16,
        raw_value: u16,
        io_value: u16,
        access: GbaAudioTraceAccess,
        origin: GbaAudioTraceOrigin,
    },
    FifoHalfword {
        fifo: GbaDirectSoundFifo,
        value: u16,
        access: GbaAudioTraceAccess,
        origin: GbaAudioTraceOrigin,
    },
    FifoReset {
        fifo: GbaDirectSoundFifo,
        access: GbaAudioTraceAccess,
        origin: GbaAudioTraceOrigin,
    },
    FifoPop {
        fifo: GbaDirectSoundFifo,
        timer: u8,
        effective_soundcnt_h: u16,
        before_len: u8,
        after_len: u8,
        value: i8,
        underflow: bool,
        origin: GbaAudioTraceOrigin,
    },
    Terminal {
        fifo_a: GbaAudioTraceFifoState,
        fifo_b: GbaAudioTraceFifoState,
        origin: GbaAudioTraceOrigin,
    },
}

#[cfg(test)]
#[path = "gba/tests.rs"]
mod tests;
