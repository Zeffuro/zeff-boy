use super::{AudioTraceChip, ChipAudioTrace, ChipAudioTraceRecorder};

pub type MdAudioTrace = ChipAudioTrace<MdTraceChip, MdTraceWrite>;
pub type MdAudioTraceRecorder = ChipAudioTraceRecorder<MdTraceChip, MdTraceWrite>;

#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
pub enum MdTraceRegion {
    NtscUs,
    NtscJapan,
    Pal,
}

#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
pub enum MdTraceReset {
    ZeffPowerOnV1,
}

#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MdTraceChip {
    pub master_hz: u64,
    pub region: MdTraceRegion,
    /// Master ticks per internal chip advance clock, not physical input-pin clocks.
    pub psg_divisor: u32,
    /// Master ticks per internal chip advance clock, not physical input-pin clocks.
    pub opn2_divisor: u32,
    pub reset: MdTraceReset,
}

impl AudioTraceChip for MdTraceChip {
    fn clock_numerator_hz(&self) -> u64 {
        self.master_hz
    }

    fn clock_denominator(&self) -> u32 {
        1
    }
}

#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
pub enum MdTraceWriter {
    M68000,
    Z80,
}

#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
pub enum MdTraceSource {
    /// Offset into the effective core ROM bytes identified by the capture.
    EffectiveRom {
        offset: u32,
    },
    MainRam {
        offset: u32,
    },
    SoundRam {
        offset: u16,
    },
    Unknown,
}

#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MdTraceOrigin {
    pub writer: MdTraceWriter,
    /// Architectural CPU/segment entry PC, not guaranteed opcode-fetch provenance.
    pub boundary_pc: u32,
    pub source: MdTraceSource,
}

/// Typed origin is authoritative; generic event PC mirrors its boundary PC and
/// generic instruction_source remains Unknown.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
pub enum MdTraceWrite {
    Psg {
        origin: MdTraceOrigin,
        value: u8,
    },
    Opn2Port {
        origin: MdTraceOrigin,
        port: u8,
        value: u8,
    },
    Opn2Reset {
        origin: MdTraceOrigin,
    },
}

#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MdAudioTraceCapture {
    pub effective_rom_sha256: [u8; 32],
    pub effective_rom_len: u32,
    pub trace: MdAudioTrace,
}

#[cfg(test)]
mod tests;
