use crate::hardware::bus::Bus;
use crate::{BusError, Emulator};
use std::cell::{Cell, RefCell};
use std::marker::PhantomData;
use std::rc::Rc;
use zeff_m68k::bus::{BusAccess, Direction};
use zeff_m68k::timing::InterruptVector;

mod bus;
mod sound;
mod watch;
mod write_watch;
pub(crate) use sound::preparation_failed;
pub use sound::{
    SoundCandidateAccess, SoundPreparationContext, SoundPreparationFault, SoundTerminalObservation,
};
#[cfg(test)]
pub(crate) use watch::assert_lifetime_contract as assert_watch_lifetime_contract;
pub use watch::{InstructionMatch, InstructionWatch, PRECEDING_ENTRY_CAPACITY};
#[cfg(test)]
pub(crate) use write_watch::assert_contract as assert_write_watch_contract;
pub use write_watch::{
    WRITE_HISTORY_CAPACITY, WriteHistory, WriteObservation, WriteRange, WriteWatch,
};
pub mod field;
mod vdp;
pub(crate) use vdp::ControlScope;
pub use vdp::{ControlWriteContext, RegisterPending, RegisterRejection, RejectionSite, VdpContext};
pub(crate) mod context;
pub(crate) use bus::TraceBus;
pub use context::{
    AdapterContext, BoardContext, BoardLocation, BusContext, MachineSnapshot, Observation,
    PendingContext, PendingSoundObservation, ScheduleContext, Slot,
};

pub const HISTORY_CAPACITY: usize = 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Callback {
    Read,
    Write,
    Idle,
    Acknowledge,
    BeginRmw,
    EndRmw,
    BeginReset,
    EndReset,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    PreciseTimingError,
    Complete,
    Read(u16),
    Interrupt(InterruptVector),
    Unsupported,
    UnsupportedAccess { address: u32, direction: Direction },
    UnqualifiedCartridgeReset { address: u32 },
    ClockOverflow,
    ClockMismatch { expected: u64, actual: u64 },
    SvpBusError { address: u32, direction: Direction },
    VdpScheduleError,
    VdpPortError,
}

impl Outcome {
    fn failed(self) -> bool {
        !matches!(
            self,
            Self::Complete | Self::Read(_) | Self::Interrupt(_) | Self::Unsupported
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BusRecord {
    pub sequence: u64,
    pub boundary: u64,
    pub callback: Callback,
    pub access: Option<BusAccess>,
    pub write_value: Option<u16>,
    pub requested_start: u64,
    pub idle_clocks: u32,
    pub wait_clocks: u32,
    pub entry: AdapterContext,
    pub service: Option<BusContext>,
    pub vdp_control: Option<ControlWriteContext>,
    pub returned: Option<AdapterContext>,
    pub outcome: Option<Outcome>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Entry {
    pub boundary: u64,
    pub snapshot: MachineSnapshot,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Report {
    pub schema: u8,
    pub main_clocked_bus_only: bool,
    pub service_scheduler_unavailable: bool,
    pub complete: bool,
    pub entry: Option<Entry>,
    pub terminal: MachineSnapshot,
    pub first_failed_sequence: Option<u64>,
    pub first_sound_failure: Option<SoundPreparationFault>,
    pub total_callbacks: u64,
    pub overwritten: u64,
    pub history: [Option<BusRecord>; HISTORY_CAPACITY],
    pub instruction_match: Option<Box<InstructionMatch>>,
    pub write_history: Option<Box<WriteHistory>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaptureError {
    UnsupportedTiming,
    AlreadyActive,
    Borrowed,
    Stale,
    CounterExhausted,
    ThreadUnavailable,
    InvalidDataRegister,
    InvalidWriteRange,
}

struct Recorder {
    generation: u64,
    boundary: u64,
    entry: Option<Entry>,
    next: u64,
    overwritten: u64,
    cursor: usize,
    history: [Option<BusRecord>; HISTORY_CAPACITY],
    in_flight: Option<BusRecord>,
    first_failed: Option<u64>,
    first_sound_failure: Option<SoundPreparationFault>,
    vdp_scope: Option<u64>,
    complete: bool,
    watch: Option<Box<watch::WatchRecorder>>,
    writes: Option<Box<write_watch::WriteRecorder>>,
}

impl Recorder {
    fn new(generation: u64, watch: Option<InstructionWatch>, writes: Option<WriteWatch>) -> Self {
        Self {
            generation,
            boundary: 0,
            entry: None,
            next: 0,
            overwritten: 0,
            cursor: 0,
            history: [None; HISTORY_CAPACITY],
            in_flight: None,
            first_failed: None,
            first_sound_failure: None,
            vdp_scope: None,
            complete: true,
            watch: watch.map(|watch| Box::new(watch::WatchRecorder::new(watch))),
            writes: writes.map(|watch| Box::new(write_watch::WriteRecorder::new(watch))),
        }
    }
}

#[derive(Default)]
struct State {
    generation: u64,
    recorder: Option<Recorder>,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
    static MISSED: Cell<bool> = const { Cell::new(false) };
}

pub struct Capture {
    generation: u64,
    finished: bool,
    _owner_thread: PhantomData<Rc<()>>,
}

impl Capture {
    pub fn begin() -> Result<Self, CaptureError> {
        Self::begin_inner(None, None)
    }

    fn begin_inner(
        watch: Option<InstructionWatch>,
        writes: Option<WriteWatch>,
    ) -> Result<Self, CaptureError> {
        if field::enabled() {
            return Err(CaptureError::AlreadyActive);
        }
        STATE
            .try_with(|state| {
                let mut state = state.try_borrow_mut().map_err(|_| CaptureError::Borrowed)?;
                if state.recorder.is_some() {
                    return Err(CaptureError::AlreadyActive);
                }
                state.generation = state
                    .generation
                    .checked_add(1)
                    .ok_or(CaptureError::CounterExhausted)?;
                let generation = state.generation;
                state.recorder = Some(Recorder::new(generation, watch, writes));
                MISSED.with(|missed| missed.set(false));
                Ok(Self {
                    generation,
                    finished: false,
                    _owner_thread: PhantomData,
                })
            })
            .map_err(|_| CaptureError::ThreadUnavailable)?
    }

    pub fn finish(mut self, machine: &Emulator) -> Result<Report, CaptureError> {
        if machine.timing_model() == crate::TimingModel::Precise {
            return Err(CaptureError::UnsupportedTiming);
        }
        let terminal = machine.diagnostic_snapshot();
        let result = STATE
            .try_with(|state| {
                let mut state = state.try_borrow_mut().map_err(|_| CaptureError::Borrowed)?;
                if state.recorder.as_ref().map(|value| value.generation) != Some(self.generation) {
                    return Err(CaptureError::Stale);
                }
                let mut recorder = state.recorder.take().unwrap();
                if recorder.next >= HISTORY_CAPACITY as u64 {
                    recorder.history.rotate_left(recorder.cursor);
                }
                let failure_retained = recorder.first_failed.is_none_or(|sequence| {
                    recorder
                        .history
                        .iter()
                        .flatten()
                        .any(|record| record.sequence == sequence)
                });
                let integrity = recorder.complete
                    && recorder.in_flight.is_none()
                    && recorder.vdp_scope.is_none()
                    && !MISSED.with(Cell::get);
                let write_history = recorder.writes.map(|writes| writes.finish(integrity));
                let complete = integrity
                    && failure_retained
                    && write_history
                        .as_ref()
                        .is_none_or(|history| history.complete);
                Ok(Report {
                    schema: 2,
                    main_clocked_bus_only: true,
                    service_scheduler_unavailable: true,
                    complete,
                    entry: recorder.entry,
                    terminal,
                    first_failed_sequence: recorder.first_failed,
                    first_sound_failure: recorder
                        .first_sound_failure
                        .map(|failure| failure.finish(machine)),
                    total_callbacks: recorder.next,
                    overwritten: recorder.overwritten,
                    history: recorder.history,
                    instruction_match: recorder.watch.and_then(|watch| watch.matched),
                    write_history,
                })
            })
            .map_err(|_| CaptureError::ThreadUnavailable)?;
        self.finished = result.is_ok();
        result
    }
}

impl Drop for Capture {
    fn drop(&mut self) {
        if self.finished {
            return;
        }
        let _ = STATE.try_with(|state| {
            if let Ok(mut state) = state.try_borrow_mut() {
                if state.recorder.as_ref().map(|value| value.generation) == Some(self.generation) {
                    state.recorder = None;
                }
            } else {
                let _ = MISSED.try_with(|missed| missed.set(true));
            }
        });
    }
}

pub(crate) fn enabled() -> bool {
    STATE
        .try_with(|state| match state.try_borrow() {
            Ok(state) => state.recorder.is_some(),
            Err(_) => {
                let _ = MISSED.try_with(|missed| missed.set(true));
                false
            }
        })
        .unwrap_or(false)
}

fn observe<T>(callback: impl FnOnce(&mut Recorder) -> T) -> Option<T> {
    STATE
        .try_with(|state| match state.try_borrow_mut() {
            Ok(mut state) => state.recorder.as_mut().map(callback),
            Err(_) => {
                let _ = MISSED.try_with(|missed| missed.set(true));
                None
            }
        })
        .ok()
        .flatten()
}

pub(crate) fn instruction_entry(snapshot: MachineSnapshot) {
    observe(|recorder| {
        if recorder.writes.is_some() && recorder.in_flight.is_some() {
            recorder.complete = false;
        }
        let Some(boundary) = recorder.boundary.checked_add(1) else {
            recorder.complete = false;
            write_watch::instruction_entry(recorder, None);
            return;
        };
        recorder.boundary = boundary;
        write_watch::instruction_entry(recorder, Some(Entry { boundary, snapshot }));
        watch::instruction_entry(recorder, Entry { boundary, snapshot });
        if recorder.first_failed.is_none() {
            recorder.entry = Some(Entry { boundary, snapshot });
        }
    });
}

pub(crate) fn service_entry(bus: &Bus, access: BusAccess, callback: Callback) {
    if !enabled() {
        return;
    }
    let service = context::bus_context(bus, Some(access.bus_address));
    observe(|recorder| match recorder.in_flight.as_mut() {
        Some(record) if record.callback == callback && record.access == Some(access) => {
            if record.service.replace(service).is_some() {
                recorder.complete = false;
            }
        }
        _ => recorder.complete = false,
    });
}

fn begin_call(mut record: BusRecord) -> Option<u64> {
    observe(|recorder| {
        if recorder.in_flight.is_some() {
            recorder.complete = false;
            return None;
        }
        let Some(next) = recorder.next.checked_add(1) else {
            recorder.complete = false;
            return None;
        };
        record.sequence = recorder.next;
        record.boundary = recorder.boundary;
        recorder.next = next;
        recorder.in_flight = Some(record);
        Some(record.sequence)
    })
    .flatten()
}

fn end_call(sequence: u64, returned: AdapterContext, waits: u32, outcome: Outcome) {
    observe(|recorder| {
        let Some(mut record) = recorder.in_flight.take() else {
            recorder.complete = false;
            return;
        };
        let matched_call = record.sequence == sequence && recorder.vdp_scope.is_none();
        if !matched_call {
            recorder.complete = false;
        }
        record.returned = Some(returned);
        record.wait_clocks = waits;
        record.outcome = Some(outcome);
        write_watch::completed_call(recorder, record, matched_call);
        if outcome.failed() && recorder.first_failed.is_none() {
            recorder.first_failed = Some(sequence);
        }
        if recorder.history[recorder.cursor].is_some() {
            if let Some(overwritten) = recorder.overwritten.checked_add(1) {
                recorder.overwritten = overwritten;
            } else {
                recorder.complete = false;
            }
        }
        recorder.history[recorder.cursor] = Some(record);
        recorder.cursor = (recorder.cursor + 1) % HISTORY_CAPACITY;
    });
}

pub(crate) trait TraceError {
    fn diagnostic_outcome(&self) -> Outcome;
}

impl TraceError for BusError {
    fn diagnostic_outcome(&self) -> Outcome {
        match self {
            Self::UnsupportedAccess { address, direction } => Outcome::UnsupportedAccess {
                address: *address,
                direction: *direction,
            },
            Self::UnqualifiedCartridgeReset { address } => {
                Outcome::UnqualifiedCartridgeReset { address: *address }
            }
            Self::Svp {
                address, direction, ..
            } => Outcome::SvpBusError {
                address: *address,
                direction: *direction,
            },
            Self::ClockOverflow => Outcome::ClockOverflow,
            Self::ClockMismatch { expected, actual } => Outcome::ClockMismatch {
                expected: expected.0,
                actual: actual.0,
            },
            Self::Vdp(_) => Outcome::VdpScheduleError,
            Self::VdpPort(_) => Outcome::VdpPortError,
            Self::Owned(_) => Outcome::PreciseTimingError,
        }
    }
}

impl TraceError for Box<BusError> {
    fn diagnostic_outcome(&self) -> Outcome {
        self.as_ref().diagnostic_outcome()
    }
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod vdp_tests;
