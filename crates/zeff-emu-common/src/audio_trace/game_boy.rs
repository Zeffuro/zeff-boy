use super::{AudioTraceChip, ChipAudioTrace, ChipAudioTraceRecorder};

pub type GameBoyAudioTrace = ChipAudioTrace<GameBoyTraceChip, GameBoyTraceWrite>;
pub type GameBoyAudioTraceRecorder = ChipAudioTraceRecorder<GameBoyTraceChip, GameBoyTraceWrite>;

#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
pub enum GameBoyTraceModel {
    Dmg,
    Cgb,
}

#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
pub enum GameBoyResetKind {
    PowerOn,
    PostBoot,
}

#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GameBoyResetState {
    pub kind: GameBoyResetKind,
    pub registers: [u8; 0x17],
    pub wave_ram: [u8; 0x10],
    pub nr52: u8,
    pub divider_counter: u16,
    pub double_speed: bool,
}

#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GameBoyTraceChip {
    pub clock_hz: u32,
    pub model: GameBoyTraceModel,
    pub dmg_compatibility: bool,
    pub reset: GameBoyResetState,
}

impl AudioTraceChip for GameBoyTraceChip {
    fn clock_numerator_hz(&self) -> u64 {
        u64::from(self.clock_hz)
    }

    fn clock_denominator(&self) -> u32 {
        1
    }
}

#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
pub enum GameBoyTraceOrigin {
    Cpu,
    CpuInterrupt,
}

#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
pub enum GameBoyDividerResetCause {
    RegisterWrite,
    Stop,
    SpeedSwitch,
}

#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
pub enum GameBoyTraceWrite {
    Register {
        address: u16,
        value: u8,
        origin: GameBoyTraceOrigin,
    },
    WaveRam {
        address: u16,
        value: u8,
        applied_index: Option<u8>,
        origin: GameBoyTraceOrigin,
    },
    DividerReset {
        cause: GameBoyDividerResetCause,
        divider_counter: u16,
        apu_bit: bool,
    },
    SequencerClock {
        primary: u8,
        secondary: u8,
    },
    Stop {
        entered: bool,
    },
    SpeedSwitch {
        double_speed: bool,
    },
    SpeedSwitchDelay {
        cycles: u64,
    },
}
