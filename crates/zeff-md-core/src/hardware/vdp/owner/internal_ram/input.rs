use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::hardware::vdp) struct Profile {
    pub memory: super::super::ram::Profile,
}

impl Profile {
    pub fn validate(self) -> Result<()> {
        self.memory.validate()?;
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::hardware::vdp) struct PlanesInputs {
    pub profile: Profile,
    pub phases: PhaseLevels,
    pub frontend: MemoryDrives,
    pub horizontal_counter: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::hardware::vdp) struct VideoInputs {
    pub profile: Profile,
    pub phases: PhaseLevels,
    pub frontend: MemoryDrives,
    pub palette: PaletteSignals,
    pub visible_gate: bool,
    pub geometry: super::super::domain::RenderGeometry,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Target {
    Cram,
    Vsram,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Write {
    pub sequence: u64,
    pub target: Target,
    pub index: u8,
    pub mask: u16,
    pub value: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::hardware::vdp) struct BusUpdate {
    pub mask: u16,
    pub value: u16,
}

impl BusUpdate {
    pub fn apply(self, previous: u16) -> u16 {
        (previous & !self.mask) | self.value
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::hardware::vdp) struct VsramView {
    pub out: u16,
    pub even: u16,
    pub odd: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::hardware::vdp) struct PlanesOutput {
    pub before: VsramView,
    pub after: VsramView,
    pub bus: Option<BusUpdate>,
    pub write: Option<Write>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::hardware::vdp) struct VideoOutput {
    pub before_packed: u16,
    pub after_packed: u16,
    pub palette_address: u8,
    pub rgb: DacDrive,
    pub bus: Option<BusUpdate>,
    pub write: Option<Write>,
}

#[derive(Clone, Copy)]
pub(super) struct Store {
    pub target: Target,
    pub index: u8,
    pub mask: u16,
    pub value: u16,
}
