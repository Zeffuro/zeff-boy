use super::{AudioTraceChip, AudioTraceSource, ChipAudioTrace, ChipAudioTraceRecorder};

pub type NesAudioTrace = ChipAudioTrace<NesTraceChip, NesTraceWrite>;
pub type NesAudioTraceRecorder = ChipAudioTraceRecorder<NesTraceChip, NesTraceWrite>;

#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
pub enum NesTraceRegion {
    Ntsc,
    Pal,
    Dendy,
}

#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
pub enum NesTraceReset {
    /// Fresh native APU construction, before its first tick; excludes warm reset.
    ZeffPowerOnV1,
}

#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NesTraceChip {
    pub clock_hz_numerator: u64,
    pub clock_hz_denominator: u32,
    pub region: NesTraceRegion,
    pub reset: NesTraceReset,
    /// Events start at APU cycle zero; this CPU origin supplies bus-cycle parity.
    pub initial_cpu_cycle: u64,
    pub initial_cpu_cycle_odd: bool,
    pub initial_apu_frame_cycle: u64,
    pub initial_half_rate_timer_clock: bool,
}

impl AudioTraceChip for NesTraceChip {
    fn clock_numerator_hz(&self) -> u64 {
        self.clock_hz_numerator
    }

    fn clock_denominator(&self) -> u32 {
        self.clock_hz_denominator
    }
}

#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
pub enum NesTraceOrigin {
    Cpu,
    CpuNonInstruction,
    Dma,
}

#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
pub enum NesTraceWrite {
    Register {
        address: u16,
        value: u8,
        odd_cycle: bool,
    },
    StatusRead {
        value: u8,
        origin: NesTraceOrigin,
    },
    DmcFetch {
        address: u16,
        value: u8,
        source: AudioTraceSource,
    },
}
