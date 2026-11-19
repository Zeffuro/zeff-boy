use super::BusRecord;
use super::{Capture, CaptureError, Entry, HISTORY_CAPACITY, MISSED, Recorder, STATE};
use std::cell::Cell;

pub const PRECEDING_ENTRY_CAPACITY: usize = 16;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InstructionWatch {
    pub pc: u32,
    pub data_register: Option<(u8, u32)>,
}

impl InstructionWatch {
    fn matches(self, entry: Entry) -> bool {
        entry.snapshot.registers.pc == self.pc
            && self.data_register.is_none_or(|(index, value)| {
                entry.snapshot.registers.d[usize::from(index)] == value
            })
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct InstructionMatch {
    pub schema: u8,
    pub watch: InstructionWatch,
    pub complete: bool,
    pub entry: Entry,
    pub preceding_entries: [Option<Entry>; PRECEDING_ENTRY_CAPACITY],
    pub preceding_entries_overwritten: u64,
    pub preceding_callbacks: [Option<BusRecord>; HISTORY_CAPACITY],
    pub callbacks_before_entry: u64,
    pub preceding_callbacks_overwritten: u64,
}

pub(super) struct WatchRecorder {
    watch: InstructionWatch,
    entries: [Option<Entry>; PRECEDING_ENTRY_CAPACITY],
    cursor: usize,
    pub(super) matched: Option<Box<InstructionMatch>>,
}

impl WatchRecorder {
    pub(super) fn new(watch: InstructionWatch) -> Self {
        Self {
            watch,
            entries: [None; PRECEDING_ENTRY_CAPACITY],
            cursor: 0,
            matched: None,
        }
    }
}

impl Capture {
    pub fn begin_watching(watch: InstructionWatch) -> Result<Self, CaptureError> {
        if watch.data_register.is_some_and(|(index, _)| index >= 8) {
            return Err(CaptureError::InvalidDataRegister);
        }
        Self::begin_inner(Some(watch), None)
    }

    pub fn has_instruction_match(&self) -> Result<bool, CaptureError> {
        STATE
            .try_with(|state| {
                let state = state.try_borrow().map_err(|_| CaptureError::Borrowed)?;
                let recorder = state.recorder.as_ref().ok_or(CaptureError::Stale)?;
                if recorder.generation != self.generation {
                    return Err(CaptureError::Stale);
                }
                Ok(recorder
                    .watch
                    .as_ref()
                    .is_some_and(|watch| watch.matched.is_some()))
            })
            .map_err(|_| CaptureError::ThreadUnavailable)?
    }
}

pub(super) fn instruction_entry(recorder: &mut Recorder, entry: Entry) {
    let Some(watch) = recorder.watch.as_mut() else {
        return;
    };
    if watch.matched.is_some() {
        return;
    }
    if watch.watch.matches(entry) {
        let mut entries = watch.entries;
        if entry.boundary > PRECEDING_ENTRY_CAPACITY as u64 {
            entries.rotate_left(watch.cursor);
        }
        let mut callbacks = recorder.history;
        if recorder.next >= HISTORY_CAPACITY as u64 {
            callbacks.rotate_left(recorder.cursor);
        }
        // Freeze before this boundary executes; ordinary field execution continues.
        watch.matched = Some(Box::new(InstructionMatch {
            schema: 1,
            watch: watch.watch,
            complete: recorder.complete
                && recorder.in_flight.is_none()
                && recorder.vdp_scope.is_none()
                && !MISSED.with(Cell::get),
            entry,
            preceding_entries: entries,
            preceding_entries_overwritten: (entry.boundary - 1)
                .saturating_sub(PRECEDING_ENTRY_CAPACITY as u64),
            preceding_callbacks: callbacks,
            callbacks_before_entry: recorder.next,
            preceding_callbacks_overwritten: recorder.overwritten,
        }));
    } else {
        watch.entries[watch.cursor] = Some(entry);
        watch.cursor = (watch.cursor + 1) % PRECEDING_ENTRY_CAPACITY;
    }
}

#[cfg(test)]
pub(crate) fn assert_lifetime_contract(machine: &crate::Emulator) {
    use super::{PhantomData, instruction_entry, observe};

    let watch = InstructionWatch {
        pc: machine.registers().pc,
        data_register: None,
    };
    for index in [8, 255] {
        assert!(matches!(
            Capture::begin_watching(InstructionWatch {
                data_register: Some((index, 0)),
                ..watch
            }),
            Err(CaptureError::InvalidDataRegister)
        ));
    }
    let capture = Capture::begin_watching(watch).unwrap();
    let stale = Capture {
        generation: capture.generation + 1,
        finished: false,
        _owner_thread: PhantomData,
    };
    assert_eq!(stale.has_instruction_match(), Err(CaptureError::Stale));
    drop(stale);
    STATE.with(|state| {
        let _borrow = state.borrow_mut();
        assert_eq!(capture.has_instruction_match(), Err(CaptureError::Borrowed));
    });
    assert_eq!(capture.has_instruction_match(), Ok(false));
    instruction_entry(machine.diagnostic_snapshot());
    assert_eq!(capture.has_instruction_match(), Ok(true));
    let report = capture.finish(machine).unwrap();
    assert!(report.complete && report.instruction_match.unwrap().complete);

    let capture = Capture::begin_watching(InstructionWatch {
        data_register: Some((7, 0)),
        ..watch
    })
    .unwrap();
    let mut snapshot = machine.diagnostic_snapshot();
    snapshot.registers.d[7] = 0x00010000;
    instruction_entry(snapshot);
    assert_eq!(capture.has_instruction_match(), Ok(false));
    snapshot.registers.d[7] = 0;
    instruction_entry(snapshot);
    assert_eq!(capture.has_instruction_match(), Ok(true));
    let report = capture.finish(machine).unwrap();
    assert!(report.complete);
    let found = report.instruction_match.unwrap();
    assert!(found.complete);
    assert_eq!(found.entry.snapshot.registers.d[7], 0);
    assert_eq!(
        found.preceding_entries[0].unwrap().snapshot.registers.d[7],
        0x00010000
    );

    for miss_before_match in [true, false] {
        let capture = Capture::begin_watching(watch).unwrap();
        if !miss_before_match {
            instruction_entry(machine.diagnostic_snapshot());
        }
        STATE.with(|state| {
            let _borrow = state.borrow_mut();
            instruction_entry(machine.diagnostic_snapshot());
        });
        if miss_before_match {
            instruction_entry(machine.diagnostic_snapshot());
        }
        let report = capture.finish(machine).unwrap();
        assert!(!report.complete);
        assert_eq!(
            report.instruction_match.unwrap().complete,
            !miss_before_match
        );
    }
    let capture = Capture::begin_watching(watch).unwrap();
    observe(|recorder| recorder.boundary = u64::MAX);
    instruction_entry(machine.diagnostic_snapshot());
    assert_eq!(capture.has_instruction_match(), Ok(false));
    assert!(!capture.finish(machine).unwrap().complete);
    drop(Capture::begin_watching(watch).unwrap());
    let fresh = Capture::begin().unwrap();
    assert_eq!(fresh.has_instruction_match(), Ok(false));
    let report = fresh.finish(machine).unwrap();
    assert!(report.complete && report.instruction_match.is_none());
}
