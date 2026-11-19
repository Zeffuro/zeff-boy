use super::{
    BusRecord, Callback, Capture, CaptureError, Entry, InstructionWatch, Outcome, Recorder,
};
use zeff_m68k::bus::{BusAccess, ByteLanes};

pub const WRITE_HISTORY_CAPACITY: usize = 256;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WriteRange {
    pub first: u32,
    pub last: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WriteWatch {
    pub ranges: [Option<WriteRange>; 4],
}

impl WriteWatch {
    fn valid(self) -> bool {
        self.ranges.iter().any(Option::is_some)
            && self
                .ranges
                .iter()
                .flatten()
                .all(|r| r.first <= r.last && r.last <= 0xff_ffff)
    }

    fn matches(self, access: BusAccess) -> bool {
        let contains = |address| {
            self.ranges
                .iter()
                .flatten()
                .any(|r| (r.first..=r.last).contains(&address))
        };
        let upper = matches!(access.lanes, ByteLanes::Upper | ByteLanes::Both)
            && contains(access.bus_address);
        let lower = matches!(access.lanes, ByteLanes::Lower | ByteLanes::Both)
            && access.bus_address.checked_add(1).is_some_and(contains);
        upper || lower
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WriteObservation {
    pub entry: Entry,
    pub record: BusRecord,
}

// Completed MAIN callbacks can suppress memory writes, and other paths can publish memory.
#[derive(Debug, PartialEq, Eq)]
pub struct WriteHistory {
    pub schema: u8,
    pub complete: bool,
    pub total_matched: u64,
    pub dropped: u64,
    pub overflow: bool,
    pub observations: Vec<WriteObservation>,
}

pub(super) struct WriteRecorder {
    watch: WriteWatch,
    current_entry: Option<Entry>,
    last_sequence: Option<u64>,
    history: Box<WriteHistory>,
}

impl WriteRecorder {
    pub(super) fn new(watch: WriteWatch) -> Self {
        Self {
            watch,
            current_entry: None,
            last_sequence: None,
            history: Box::new(WriteHistory {
                schema: 1,
                complete: true,
                total_matched: 0,
                dropped: 0,
                overflow: false,
                observations: Vec::with_capacity(WRITE_HISTORY_CAPACITY),
            }),
        }
    }

    pub(super) fn finish(mut self, complete: bool) -> Box<WriteHistory> {
        self.history.complete &= complete;
        self.history
    }

    fn drop_match(&mut self) {
        self.history.complete = false;
        if let Some(dropped) = self.history.dropped.checked_add(1) {
            self.history.dropped = dropped;
        }
    }

    fn completed(&mut self, record: BusRecord, matched_call: bool) {
        let ordered = self.last_sequence.map_or(record.sequence == 0, |previous| {
            previous.checked_add(1) == Some(record.sequence)
        });
        self.last_sequence = Some(record.sequence);
        if !matched_call || !ordered {
            self.history.complete = false;
            return;
        }
        if record.callback != Callback::Write || record.outcome != Some(Outcome::Complete) {
            return;
        }
        let Some(access) = record.access else {
            self.history.complete = false;
            return;
        };
        if !self.watch.matches(access) {
            return;
        }
        let Some(total) = self.history.total_matched.checked_add(1) else {
            self.history.complete = false;
            return;
        };
        self.history.total_matched = total;
        if total > WRITE_HISTORY_CAPACITY as u64 {
            self.history.overflow = true;
            self.drop_match();
            return;
        }
        let Some(entry) = self.current_entry.filter(|e| e.boundary == record.boundary) else {
            self.drop_match();
            return;
        };
        if record.service.is_none() || record.returned.is_none() || record.write_value.is_none() {
            self.drop_match();
            return;
        }
        self.history
            .observations
            .push(WriteObservation { entry, record });
    }
}

impl Capture {
    pub fn begin_watching_writes(
        writes: WriteWatch,
        instruction: Option<InstructionWatch>,
    ) -> Result<Self, CaptureError> {
        if !writes.valid() {
            return Err(CaptureError::InvalidWriteRange);
        }
        if instruction.is_some_and(|w| w.data_register.is_some_and(|(i, _)| i >= 8)) {
            return Err(CaptureError::InvalidDataRegister);
        }
        Self::begin_inner(instruction, Some(writes))
    }
}

pub(super) fn instruction_entry(recorder: &mut Recorder, entry: Option<Entry>) {
    if let Some(writes) = recorder.writes.as_mut() {
        writes.current_entry = entry;
    }
}

pub(super) fn completed_call(recorder: &mut Recorder, record: BusRecord, matched_call: bool) {
    let valid = matched_call
        && recorder.complete
        && record.boundary == recorder.boundary
        && !super::MISSED.with(std::cell::Cell::get);
    if let Some(writes) = recorder.writes.as_mut() {
        writes.completed(record, valid);
    }
}

#[cfg(test)]
mod tests;
#[cfg(test)]
pub(crate) use tests::assert_contract;
