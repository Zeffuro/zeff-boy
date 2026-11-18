use crate::diagnostic::{MachineSnapshot, VdpContext};
use crate::hardware::vdp::{FieldPosition, InterlaceMode};
use crate::{Emulator, MachineFault, MachineStep, Vdp, VdpLineError};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Progressive,
    Normal,
    Double,
}
impl From<InterlaceMode> for Mode {
    fn from(value: InterlaceMode) -> Self {
        match value {
            InterlaceMode::Progressive => Self::Progressive,
            InterlaceMode::Normal => Self::Normal,
            InterlaceMode::Double => Self::Double,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Field {
    pub id: u64,
    pub start: u64,
    pub projected_end: Option<u64>,
    pub line: u16,
    pub line_start: u64,
    pub line_ticks: u64,
    pub lines: u64,
    pub mode: Mode,
    pub odd: bool,
    pub active_mode: Mode,
    pub active_odd: bool,
}
impl From<FieldPosition> for Field {
    fn from(value: FieldPosition) -> Self {
        Self {
            id: value.id,
            start: value.start.get(),
            projected_end: value.projected_end.map(|tick| tick.get()),
            line: value.line,
            line_start: value.line_start.get(),
            line_ticks: value.line_ticks,
            lines: value.lines,
            mode: value.mode.into(),
            odd: value.odd,
            active_mode: value.active_mode.into(),
            active_odd: value.active_odd,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Raster {
    pub field: Option<Field>,
    pub vdp: VdpContext,
    pub effective_line_ticks: u64,
}
impl Raster {
    pub(crate) fn read(vdp: &Vdp, region: crate::Region) -> Self {
        Self {
            field: vdp.field_position(region).ok().map(Into::into),
            vdp: vdp.diagnostic_context(),
            effective_line_ticks: vdp.effective_line_ticks(),
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Endpoint {
    pub machine: MachineSnapshot,
    pub pacing: u64,
    pub raster: Raster,
    pub presented: u64,
    pub ready: bool,
    pub dimensions: (usize, usize),
}
impl Endpoint {
    pub(crate) fn read(machine: &Emulator) -> Self {
        Self {
            machine: machine.diagnostic_snapshot(),
            pacing: machine.pacing_field_count(),
            raster: Raster::read(machine.vdp(), machine.region()),
            presented: machine.frame_count(),
            ready: machine.frame_ready(),
            dimensions: machine.framebuffer_dimensions(),
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RowField {
    pub id: u64,
    pub height: usize,
    pub mode: Mode,
    pub odd: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RowState {
    pub width: usize,
    pub height: usize,
    pub rows: usize,
    pub field: Option<RowField>,
    pub valid_fields: u8,
    pub presented: u64,
    pub ready: bool,
    pub error: Option<VdpLineError>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RowContext {
    pub at: u64,
    pub completed_field: Field,
    pub row: u16,
    pub raw: [Option<u8>; 3],
    pub height: usize,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RowReason {
    Accepted,
    RowsMismatch,
    FieldMismatch,
    BothMismatch,
    OutsideHeight,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RowRun {
    pub first: RowContext,
    pub last_row: u16,
    pub count: u64,
    pub before: Option<RowState>,
    pub after: Option<RowState>,
    pub reason: RowReason,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FieldCounts {
    pub id: u64,
    pub offered: u64,
    pub accepted: u64,
    pub render_error: u64,
    pub mismatch: u64,
    pub outside_height: u64,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Label {
    AfterLoad,
    BeforeCall1,
    AfterCall1,
    BeforeCall2,
    AfterCall2,
}
// Keep event payloads in the preallocated slots.
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Kind {
    Endpoint {
        label: Label,
        endpoint: Endpoint,
        result: Option<Result<(), MachineFault>>,
    },
    Calendar {
        before: Raster,
        after: Raster,
        crossed_at: u64,
    },
    Publication {
        before: Raster,
        after: Raster,
    },
    RowTransition {
        context: RowContext,
        before: RowState,
        after: RowState,
        causes: u16,
    },
    RowRun(RowRun),
    Boundary {
        before: MachineSnapshot,
        after: MachineSnapshot,
        pacing_before: u64,
        pacing_after: u64,
        result: Result<MachineStep, MachineFault>,
    },
    Skip {
        before: Raster,
        after: Raster,
        target: u64,
        horizon: u64,
    },
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Event {
    pub first_sequence: u64,
    pub last_sequence: u64,
    pub call: u8,
    pub first_at: u64,
    pub last_at: u64,
    pub kind: Kind,
}
#[derive(Debug, PartialEq, Eq)]
pub struct Report {
    pub schema: u8,
    pub complete: bool,
    pub issues: u32,
    pub observer_dropped: u64,
    pub calls: u8,
    pub successful_calls: u8,
    pub final_boundaries: u8,
    pub skip_count: u64,
    pub total_observations: u64,
    pub fields: [Option<FieldCounts>; 4],
    pub terminal: Endpoint,
    pub events: Box<[Option<Event>]>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaptureError {
    UnsupportedTiming,
    AlreadyActive,
    OtherCaptureActive,
    Borrowed,
    Stale,
    CounterExhausted,
    ThreadUnavailable,
}
