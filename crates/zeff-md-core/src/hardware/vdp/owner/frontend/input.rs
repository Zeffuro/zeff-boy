use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::hardware::vdp) struct ExternalPins {
    pub cpu_clock1: bool,
    pub address_strobe: bool,
    pub bus_grant: bool,
    pub bus_grant_acknowledge_level: bool,
    pub data_acknowledge_level: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::hardware::vdp) struct MemoryInputs {
    pub opportunity: bool,
    pub data_window: bool,
    pub address: u32,
    pub data: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::hardware::vdp) struct Inputs {
    pub phases: PhaseLevels,
    pub main: MainRequests,
    pub upper_strobe: bool,
    pub lower_strobe: bool,
    pub hv_read: bool,
    pub reset_comb: bool,
    pub external: ExternalPins,
    pub external_address_cycle: bool,
    pub memory: MemoryInputs,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::hardware::vdp) struct DmaView {
    pub active: bool,
    pub external_word: bool,
    pub external_byte: bool,
    pub fill: bool,
    pub copy: bool,
    pub bus_request: bool,
    pub bus_grant_acknowledge: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::hardware::vdp) struct ExternalFeedback {
    pub source_enabled: bool,
    pub bus_grant_acknowledge: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::hardware::vdp) struct MemoryDrives {
    pub address: u32,
    pub data: u16,
    pub upper_byte: bool,
    pub lower_byte: bool,
    pub upper_write: bool,
    pub lower_write: bool,
    pub cram_upper: bool,
    pub cram_lower: bool,
    pub vsram_upper: bool,
    pub vsram_lower: bool,
    pub read_cram: bool,
    pub read_vsram: bool,
    pub read_kind_delay: bool,
    pub fifo_write: bool,
    pub code_bits: u8,
    pub fill: bool,
    pub copy: bool,
    pub data_low_delay: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::hardware::vdp) enum MemoryTarget {
    Vram,
    Cram,
    Vsram,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::hardware::vdp) struct MemoryWrite {
    pub target: MemoryTarget,
    pub address: u32,
    pub data: u16,
    pub upper: bool,
    pub lower: bool,
}

impl MemoryWrite {
    pub(super) fn validate(self) -> Result<()> {
        ensure!(
            self.address <= 0x1ffff && (self.upper || self.lower),
            "invalid VDP launched write"
        );
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::hardware::vdp) struct WriteReceipt {
    pub sequence: u64,
    pub write: MemoryWrite,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::hardware::vdp) struct Output {
    pub control: ControlEvaluation,
    pub dma: DmaView,
    pub memory: MemoryDrives,
    pub acknowledge: bool,
    pub data_ack: bool,
    pub read_ack: bool,
    pub bus_load: Option<(u16, [bool; 2])>,
    pub read_word: u16,
    pub fifo_full: bool,
    pub fifo_empty: bool,
    pub data_capture: bool,
    pub fifo_capture: Option<u8>,
    pub dma_terminal: bool,
    pub dma_activity: bool,
}
