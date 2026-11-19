//! Unstable, thread-local observation of two ordinary field calls.

use super::MachineSnapshot;
use crate::{Emulator, MachineFault, MachineStep};
use std::cell::{Cell, RefCell};
use std::marker::PhantomData;
use std::rc::Rc;

mod types;
pub use types::*;

pub const CAPACITY: usize = 128;
pub mod issue {
    pub const BORROW: u32 = 1;
    pub const THREAD: u32 = 2;
    pub const CONTEXT: u32 = 4;
    pub const OWNER: u32 = 8;
    pub const REENTRANT: u32 = 16;
    pub const COUNTER: u32 = 32;
    pub const CALLS: u32 = 64;
    pub const GAP: u32 = 128;
    pub const SKIP: u32 = 256;
}
pub mod cause {
    pub const RENDER_ERROR: u16 = 1;
    pub const DIMENSIONS: u16 = 2;
    pub const ROW_ZERO: u16 = 4;
    pub const PARTNER_INVALID: u16 = 8;
    pub const ADOPT_FIELD: u16 = 16;
    pub const ROWS_MISMATCH: u16 = 32;
    pub const FIELD_MISMATCH: u16 = 64;
    pub const ACCEPTED: u16 = 128;
    pub const PUBLISHED: u16 = 256;
}

struct Recorder {
    generation: u64,
    owner: Option<(usize, usize)>,
    load_owner: Option<usize>,
    active: bool,
    calls: u8,
    successes: u8,
    boundaries: u8,
    call_crossings: u8,
    last_crossing: Option<(u64, u64)>,
    labels: u8,
    issues: u32,
    dropped: u64,
    skips: u64,
    next: u64,
    used: usize,
    events: Box<[Option<Event>]>,
    pending: Option<Event>,
    boundary: Option<(MachineSnapshot, u64)>,
    raster: Option<RowContext>,
    fields: [Option<FieldCounts>; 4],
}
impl Recorder {
    fn new(generation: u64) -> Self {
        Self {
            generation,
            owner: None,
            load_owner: None,
            active: false,
            calls: 0,
            successes: 0,
            boundaries: 0,
            call_crossings: 0,
            last_crossing: None,
            labels: 0,
            issues: 0,
            dropped: 0,
            skips: 0,
            next: 0,
            used: 0,
            events: vec![None; CAPACITY].into_boxed_slice(),
            pending: None,
            boundary: None,
            raster: None,
            fields: [None; 4],
        }
    }
    fn sequence(&mut self) -> u64 {
        let sequence = self.next;
        if let Some(next) = self.next.checked_add(1) {
            self.next = next;
        } else {
            self.issues |= issue::COUNTER;
        }
        sequence
    }
    fn store(&mut self, event: Event) {
        if self.used < self.events.len() {
            self.events[self.used] = Some(event);
            self.used += 1;
        } else {
            self.dropped = self.dropped.checked_add(1).unwrap_or_else(|| {
                self.issues |= issue::COUNTER;
                u64::MAX
            });
        }
    }
    fn flush(&mut self) {
        if let Some(event) = self.pending.take() {
            self.store(event);
        }
    }
    fn emit(&mut self, at: u64, kind: Kind) {
        self.flush();
        let sequence = self.sequence();
        self.store(Event {
            first_sequence: sequence,
            last_sequence: sequence,
            call: self.calls,
            first_at: at,
            last_at: at,
            kind,
        });
    }
    fn endpoint(
        &mut self,
        label: Label,
        endpoint: Endpoint,
        result: Option<Result<(), MachineFault>>,
    ) {
        if endpoint
            .raster
            .field
            .is_none_or(|field| field.id != endpoint.pacing)
            || endpoint.raster.vdp.ticks != endpoint.machine.context.bus.master_ticks
        {
            self.issues |= issue::CONTEXT;
        }
        self.labels = self.labels.checked_add(1).unwrap_or_else(|| {
            self.issues |= issue::COUNTER;
            u8::MAX
        });
        self.emit(
            endpoint.machine.context.bus.master_ticks,
            Kind::Endpoint {
                label,
                endpoint,
                result,
            },
        );
    }
    fn row(
        &mut self,
        context: RowContext,
        before: Option<RowState>,
        after: Option<RowState>,
        flags: u16,
    ) {
        let slot = self
            .fields
            .iter()
            .position(|value| value.is_some_and(|v| v.id == context.completed_field.id))
            .or_else(|| self.fields.iter().position(Option::is_none));
        if let Some(slot) = slot {
            let counts = self.fields[slot].get_or_insert(FieldCounts {
                id: context.completed_field.id,
                ..FieldCounts::default()
            });
            let reason = if before.is_none() {
                &mut counts.outside_height
            } else if flags & cause::ACCEPTED != 0 {
                &mut counts.accepted
            } else if flags & cause::RENDER_ERROR != 0 {
                &mut counts.render_error
            } else {
                &mut counts.mismatch
            };
            if let (Some(offered), Some(total)) =
                (counts.offered.checked_add(1), reason.checked_add(1))
            {
                counts.offered = offered;
                *reason = total;
            } else {
                self.issues |= issue::COUNTER;
            }
        } else {
            self.issues |= issue::COUNTER;
        }
        let ordinary = cause::ACCEPTED | cause::ROWS_MISMATCH | cause::FIELD_MISMATCH;
        if flags & !ordinary != 0 {
            self.emit(
                context.at,
                Kind::RowTransition {
                    context,
                    before: before.unwrap(),
                    after: after.unwrap(),
                    causes: flags,
                },
            );
            return;
        }
        let reason = match flags {
            cause::ACCEPTED => RowReason::Accepted,
            cause::ROWS_MISMATCH => RowReason::RowsMismatch,
            cause::FIELD_MISMATCH => RowReason::FieldMismatch,
            0 if before.is_none() => RowReason::OutsideHeight,
            _ => RowReason::BothMismatch,
        };
        let sequence = self.sequence();
        if let Some(event) = self.pending.as_mut()
            && let Kind::RowRun(run) = &mut event.kind
        {
            let first = run.first;
            if event.call == self.calls
                && run.reason == reason
                && first.raw == context.raw
                && first.height == context.height
                && first.completed_field.id == context.completed_field.id
                && first.completed_field.active_mode == context.completed_field.active_mode
                && first.completed_field.active_odd == context.completed_field.active_odd
                && run.last_row.checked_add(1) == Some(context.row)
                && run.after == before
            {
                if let Some(count) = run.count.checked_add(1) {
                    run.count = count;
                    run.last_row = context.row;
                    run.after = after;
                    event.last_sequence = sequence;
                    event.last_at = context.at;
                    return;
                }
                self.issues |= issue::COUNTER;
            }
        }
        self.flush();
        self.pending = Some(Event {
            first_sequence: sequence,
            last_sequence: sequence,
            call: self.calls,
            first_at: context.at,
            last_at: context.at,
            kind: Kind::RowRun(RowRun {
                first: context,
                last_row: context.row,
                count: 1,
                before,
                after,
                reason,
            }),
        });
    }
}
#[derive(Default)]
struct State {
    generation: u64,
    recorder: Option<Recorder>,
}
thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
    static MISSED: Cell<u32> = const { Cell::new(0) };
}
fn missed(flag: u32) {
    let _ = MISSED.try_with(|value| value.set(value.get() | flag));
}
fn observe<T>(callback: impl FnOnce(&mut Recorder) -> T) -> Option<T> {
    STATE
        .try_with(|state| match state.try_borrow_mut() {
            Ok(mut state) => state.recorder.as_mut().map(callback),
            Err(_) => {
                missed(issue::BORROW);
                None
            }
        })
        .unwrap_or_else(|_| {
            missed(issue::THREAD);
            None
        })
}
pub(crate) fn enabled() -> bool {
    observe(|_| true).unwrap_or(false)
}
pub struct Capture {
    generation: u64,
    finished: bool,
    _thread: PhantomData<Rc<()>>,
}
impl Capture {
    pub fn begin() -> Result<Self, CaptureError> {
        if super::enabled() {
            return Err(CaptureError::OtherCaptureActive);
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
                state.recorder = Some(Recorder::new(generation));
                MISSED.with(|value| value.set(0));
                Ok(Self {
                    generation,
                    finished: false,
                    _thread: PhantomData,
                })
            })
            .map_err(|_| CaptureError::ThreadUnavailable)?
    }
    pub fn after_load(&self, machine: &Emulator) {
        if machine.timing_model() == crate::TimingModel::Precise {
            return;
        }
        let endpoint = Endpoint::read(machine);
        observe(|recorder| {
            if recorder.generation != self.generation || recorder.labels != 0 || recorder.calls != 0
            {
                recorder.issues |= issue::CONTEXT;
            }
            recorder.load_owner = Some(machine as *const Emulator as usize);
            recorder.endpoint(Label::AfterLoad, endpoint, None);
        });
    }
    pub fn finish(mut self, machine: &Emulator) -> Result<Report, CaptureError> {
        if machine.timing_model() == crate::TimingModel::Precise {
            return Err(CaptureError::UnsupportedTiming);
        }
        let terminal = Endpoint::read(machine);
        let result = STATE
            .try_with(|state| {
                let mut state = state.try_borrow_mut().map_err(|_| CaptureError::Borrowed)?;
                if state.recorder.as_ref().map(|value| value.generation) != Some(self.generation) {
                    return Err(CaptureError::Stale);
                }
                let mut recorder = state.recorder.take().unwrap();
                recorder.flush();
                recorder.issues |= MISSED.with(Cell::get);
                if recorder
                    .owner
                    .is_none_or(|owner| owner.0 != machine as *const Emulator as usize)
                {
                    recorder.issues |= issue::OWNER;
                }
                if recorder.active
                    || recorder.boundary.is_some()
                    || recorder.raster.is_some()
                    || Some(recorder.labels)
                        != recorder
                            .calls
                            .checked_mul(2)
                            .and_then(|count| count.checked_add(1))
                {
                    recorder.issues |= issue::CONTEXT;
                }
                if recorder.calls != 2 || recorder.successes != 2 || recorder.boundaries != 2 {
                    recorder.issues |= issue::CALLS;
                }
                Ok(Report {
                    schema: 1,
                    complete: recorder.issues == 0 && recorder.dropped == 0,
                    issues: recorder.issues,
                    observer_dropped: recorder.dropped,
                    calls: recorder.calls,
                    successful_calls: recorder.successes,
                    final_boundaries: recorder.boundaries,
                    skip_count: recorder.skips,
                    total_observations: recorder.next,
                    fields: recorder.fields,
                    terminal,
                    events: recorder.events,
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
                missed(issue::BORROW);
            }
        });
    }
}
pub(crate) fn call_begin(owner: usize, bus: usize, endpoint: Endpoint) {
    observe(|recorder| {
        if recorder.active {
            recorder.issues |= issue::REENTRANT;
        }
        if recorder.owner.is_some_and(|value| value != (owner, bus))
            || recorder.load_owner != Some(owner)
        {
            recorder.issues |= issue::OWNER;
        }
        recorder.owner.get_or_insert((owner, bus));
        recorder.calls = recorder.calls.checked_add(1).unwrap_or_else(|| {
            recorder.issues |= issue::COUNTER;
            u8::MAX
        });
        recorder.active = true;
        recorder.call_crossings = 0;
        recorder.last_crossing = None;
        let label = match recorder.calls {
            1 => Label::BeforeCall1,
            2 => Label::BeforeCall2,
            _ => {
                recorder.issues |= issue::CALLS;
                Label::BeforeCall2
            }
        };
        recorder.endpoint(label, endpoint, None);
    });
}
pub(crate) fn call_end(endpoint: Endpoint, result: &Result<(), MachineFault>) {
    observe(|recorder| {
        if !recorder.active || recorder.boundary.is_some() || recorder.raster.is_some() {
            recorder.issues |= issue::CONTEXT;
        }
        if result.is_ok() {
            if recorder.call_crossings != 1
                || recorder.last_crossing.map(|crossing| crossing.0) != Some(endpoint.pacing)
            {
                recorder.issues |= issue::GAP;
            }
            recorder.successes = recorder.successes.checked_add(1).unwrap_or_else(|| {
                recorder.issues |= issue::COUNTER;
                u8::MAX
            });
        }
        recorder.endpoint(
            if recorder.calls == 1 {
                Label::AfterCall1
            } else {
                Label::AfterCall2
            },
            endpoint,
            Some(result.clone()),
        );
        recorder.active = false;
    });
}
pub(crate) fn boundary_entry(snapshot: MachineSnapshot, pacing: u64) {
    observe(|recorder| {
        if !recorder.active || recorder.boundary.replace((snapshot, pacing)).is_some() {
            recorder.issues |= issue::CONTEXT;
        }
    });
}
pub(crate) fn boundary_exit(
    after: MachineSnapshot,
    pacing_after: u64,
    result: &Result<MachineStep, MachineFault>,
) {
    observe(|recorder| {
        let Some((before, pacing_before)) = recorder.boundary.take() else {
            recorder.issues |= issue::CONTEXT;
            return;
        };
        if pacing_before != pacing_after || result.is_err() {
            if result.is_ok()
                && recorder.last_crossing.is_none_or(|(id, at)| {
                    id != pacing_after
                        || before.context.bus.master_ticks > at
                        || at > after.context.bus.master_ticks
                })
            {
                recorder.issues |= issue::GAP;
            }
            recorder.boundaries = recorder.boundaries.checked_add(1).unwrap_or_else(|| {
                recorder.issues |= issue::COUNTER;
                u8::MAX
            });
            recorder.emit(
                after.context.bus.master_ticks,
                Kind::Boundary {
                    before,
                    after,
                    pacing_before,
                    pacing_after,
                    result: result.clone(),
                },
            );
        }
    });
}
pub(crate) fn raster_advance(before: Raster, after: Raster, at: u64) {
    observe(|recorder| {
        if !recorder.active {
            recorder.issues |= issue::CONTEXT;
        }
        match (before.field, after.field) {
            (Some(old), Some(new)) if old.id != new.id => {
                if old.id.checked_add(1) != Some(new.id) {
                    recorder.issues |= issue::GAP;
                }
                recorder.call_crossings =
                    recorder.call_crossings.checked_add(1).unwrap_or_else(|| {
                        recorder.issues |= issue::COUNTER;
                        u8::MAX
                    });
                recorder.last_crossing = Some((new.id, new.start));
                recorder.emit(
                    at,
                    Kind::Calendar {
                        before,
                        after,
                        crossed_at: new.start,
                    },
                );
            }
            (None, _) | (_, None) => recorder.issues |= issue::CONTEXT,
            _ => {}
        }
        if before.vdp.pending.is_some_and(|write| {
            matches!(write.index, 0 | 1 | 12)
                && write.publish_at <= after.vdp.register_ticks
                && after.vdp.pending != Some(write)
        }) {
            recorder.emit(at, Kind::Publication { before, after });
        }
    });
}
pub(crate) fn skip(before: Raster, after: Raster, target: u64, horizon: u64) {
    observe(|recorder| {
        recorder.issues |= issue::SKIP;
        recorder.skips = recorder.skips.checked_add(1).unwrap_or_else(|| {
            recorder.issues |= issue::COUNTER;
            u64::MAX
        });
        recorder.emit(
            after.vdp.ticks,
            Kind::Skip {
                before,
                after,
                target,
                horizon,
            },
        );
    });
}
pub(crate) struct RasterScope {
    generation: u64,
}
impl RasterScope {
    pub(crate) fn begin(context: RowContext) -> Option<Self> {
        observe(|recorder| {
            if !recorder.active || recorder.raster.is_some() {
                recorder.issues |= issue::CONTEXT;
            }
            recorder.raster = Some(context);
            Self {
                generation: recorder.generation,
            }
        })
    }
}
impl Drop for RasterScope {
    fn drop(&mut self) {
        observe(|recorder| {
            if recorder.generation == self.generation && recorder.raster.take().is_some() {
                recorder.issues |= issue::CONTEXT;
            }
        });
    }
}
pub(crate) fn row_result(before: RowState, after: RowState, causes: u16) {
    observe(|recorder| {
        if let Some(context) = recorder.raster.take() {
            recorder.row(context, Some(before), Some(after), causes);
        } else {
            recorder.issues |= issue::CONTEXT;
        }
    });
}
pub(crate) fn outside_height() {
    observe(|recorder| {
        if let Some(context) = recorder.raster.take() {
            recorder.row(context, None, None, 0);
        } else {
            recorder.issues |= issue::CONTEXT;
        }
    });
}

#[cfg(test)]
pub(crate) fn assert_loss_invalidates(machine: &Emulator) {
    for borrowed in [false, true] {
        let capture = Capture::begin().unwrap();
        capture.after_load(machine);
        observe(|recorder| {
            recorder.owner = Some((machine as *const Emulator as usize, 0));
            recorder.calls = 2;
            recorder.successes = 2;
            recorder.boundaries = 2;
            recorder.labels = 5;
            if !borrowed {
                for _ in 0..CAPACITY {
                    recorder.emit(
                        0,
                        Kind::Endpoint {
                            label: Label::AfterLoad,
                            endpoint: Endpoint::read(machine),
                            result: None,
                        },
                    );
                }
            }
        });
        if borrowed {
            STATE.with(|state| {
                let _borrow = state.borrow_mut();
                assert!(!enabled());
            });
        }
        let report = capture.finish(machine).unwrap();
        assert!(!report.complete);
        assert_eq!(report.issues, if borrowed { issue::BORROW } else { 0 });
        assert_eq!(report.observer_dropped, if borrowed { 0 } else { 1 });
    }
}
