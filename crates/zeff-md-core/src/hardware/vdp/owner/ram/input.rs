use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::hardware::vdp) struct Profile {
    pub registers: [u8; 24],
    pub mode128: bool,
    pub interlace_latch: [bool; 2],
    pub test0: u16,
}

impl Profile {
    pub fn validate(self) -> Result<()> {
        ensure!(
            self.registers[12] & 6 == 0 && self.interlace_latch == [false; 2] && self.test0 == 0,
            "unsupported VDP VRAM controller profile"
        );
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::hardware::vdp) struct BusInputs {
    pub ad: u8,
    pub rd: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::hardware::vdp) struct Inputs {
    pub phases: PhaseLevels,
    pub profile: Profile,
    pub frontend: MemoryDrives,
    pub opportunity: bool,
    pub memory_window: bool,
    pub refresh: bool,
    pub copy_read_address_selected: bool,
    pub bus: BusInputs,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::hardware::vdp) struct Pins {
    pub ras: bool,
    pub cas: bool,
    pub we0: bool,
    pub we1: bool,
    pub oe: bool,
    pub sc: bool,
    pub se0: bool,
    pub se1: bool,
    pub driven: bool,
    pub ad: u8,
    pub rd: u8,
}

impl Pins {
    pub(super) fn software_origin() -> Self {
        Self {
            ras: true,
            cas: true,
            we0: true,
            we1: true,
            oe: true,
            sc: true,
            se0: false,
            se1: true,
            driven: false,
            ad: 0,
            rd: 0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ByteWrite {
    pub sequence: u64,
    pub physical_address: u16,
    pub canonical_address: u16,
    pub value: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::hardware::vdp) struct ByteRead {
    pub physical_address: u16,
    pub canonical_address: u16,
    pub value: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::hardware::vdp) struct RowTransfer {
    pub row: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::hardware::vdp) struct ControllerOutput {
    pub pins: Pins,
    pub returned_data: Option<u16>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::hardware::vdp) struct Response {
    pub bus: BusInputs,
    pub write: Option<ByteWrite>,
    pub read: Option<ByteRead>,
    pub transfer: Option<RowTransfer>,
}
