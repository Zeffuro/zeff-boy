use super::writes::{Link, ObservedWrite};
use super::*;
use crate::sound_trace::attempt::{append_byte, registers};
use std::collections::VecDeque;
use zeff_emu_common::debug::{
    DebugEvent, InstructionTraceRecord, RegisterDelta, TraceBoundary, TraceBoundaryKind,
    TraceExecMode, TraceFrameKnowledge, TraceObservationEnd, TraceObservationStart,
};
use zeff_z80::{Cpu, RetirementKind};

#[derive(Clone)]
pub(super) struct Row {
    pub id: u64,
    pub record: InstructionTraceRecord,
    pub before: [u32; 14],
    pub after: [u32; 14],
    pub end: u64,
    pub complete: bool,
    pub kind: RetirementKind,
    pub writes: [Option<ObservedWrite>; 8],
}
#[derive(Clone)]
pub(crate) struct Fold {
    pub(super) rows: VecDeque<Row>,
    pub(super) current: Option<u64>,
    pub(super) links: [Option<Link>; 16],
    next: Option<u64>,
    snapshot: Option<CpuSnapshot>,
    enabled: bool,
    initialized: bool,
    gaps: u64,
}
impl Default for Fold {
    fn default() -> Self {
        Self {
            rows: VecDeque::new(),
            current: None,
            links: [None; 16],
            next: Some(1),
            snapshot: None,
            enabled: false,
            initialized: false,
            gaps: 0,
        }
    }
}
struct Retirement {
    complete: bool,
    byte: Option<(u16, u8)>,
    before: u64,
    after: u64,
    kind: RetirementKind,
    lost: bool,
}
impl Fold {
    pub(crate) fn gaps(&self) -> u64 {
        self.gaps
    }
    pub(crate) fn set_enabled(&mut self, enabled: bool, output: &mut InstructionTraceStore) {
        if self.enabled == enabled {
            return;
        }
        self.discontinuity(output);
        self.enabled = enabled;
        if enabled {
            self.initialized = true;
            self.gaps = self.gaps.saturating_add(1);
        }
    }
    pub(crate) fn discontinuity(&mut self, output: &mut InstructionTraceStore) {
        self.close_all(output, TraceObservationEnd::Disabled);
        self.snapshot = None;
        self.links = [None; 16];
        if self.initialized {
            self.gaps = self.gaps.saturating_add(1);
        }
    }
    pub(crate) fn disable(&mut self, output: &mut InstructionTraceStore) {
        self.discontinuity(output);
        self.enabled = false;
    }
    pub(crate) fn reset(&mut self, output: &mut InstructionTraceStore) {
        self.close_all(output, TraceObservationEnd::Reset);
        self.snapshot = None;
        self.links = [None; 16];
    }
    pub(crate) fn fault(&mut self, output: &mut InstructionTraceStore) {
        self.close_all(output, TraceObservationEnd::Fault);
        self.snapshot = None;
        self.links = [None; 16];
    }
    pub(super) fn gap(&mut self, output: &mut InstructionTraceStore) {
        self.gaps = self.gaps.saturating_add(1);
        self.close_all(output, TraceObservationEnd::Disabled);
        self.snapshot = None;
        self.links = [None; 16];
    }
    pub(super) fn row_mut(&mut self, id: u64) -> Option<&mut Row> {
        self.rows.iter_mut().find(|row| row.id == id)
    }
    fn open(
        &mut self,
        context: (CpuSnapshot, u64, RetirementKind),
        output: &mut InstructionTraceStore,
    ) -> Option<u64> {
        if self.rows.len() == 4 {
            self.gap(output);
        }
        let (snapshot, cycle, kind) = context;
        let Some(id) = self.next else {
            self.gap(output);
            return None;
        };
        self.next = id.checked_add(1);
        let mut record = InstructionTraceRecord::new(
            TraceExecMode::Z80,
            snapshot.before[11],
            None,
            0,
            cycle,
            &[],
        );
        record.observation.frame_knowledge = TraceFrameKnowledge::Unknown;
        if !snapshot.starts && kind != RetirementKind::HaltPoll {
            record.observation.start = TraceObservationStart::Suffix;
        }
        self.rows.push_back(Row {
            id,
            record,
            before: snapshot.before,
            after: snapshot.after,
            end: cycle,
            complete: false,
            kind,
            writes: [None; 8],
        });
        self.current = Some(id);
        Some(id)
    }
    pub(super) fn suffix(&mut self, cpu: &Cpu, output: &mut InstructionTraceStore) -> Option<u64> {
        if let Some(id) = self.current {
            return Some(id);
        }
        let snapshot = CpuSnapshot::new(cpu, cpu, false);
        self.open(
            (snapshot, cpu.cycles(), RetirementKind::Instruction),
            output,
        )
    }
    fn retirement(
        &mut self,
        event: &Event,
        receipt: Retirement,
        output: &mut InstructionTraceStore,
    ) {
        let Some(mut snapshot) = self.snapshot.take() else {
            self.gap(output);
            return;
        };
        snapshot.starts &= event
            .occurrence
            .is_some_and(|id| self.links.iter().flatten().any(|link| link.id == id));
        if receipt.lost {
            if let Some(id) = self.current
                && let Some(row) = self.row_mut(id)
            {
                row.end = receipt.after;
                row.after = snapshot.after;
            }
            self.gap(output);
            return;
        }
        if snapshot.starts && self.current.is_some() {
            self.gap(output);
        }
        let id = match self.current {
            Some(id) => id,
            None => match self.open((snapshot, receipt.before, receipt.kind), output) {
                Some(id) => id,
                None => return,
            },
        };
        self.bind_retirement(event.occurrence, id, output);
        let Some(row) = self.row_mut(id) else {
            return;
        };
        if let Some((address, byte)) = receipt.byte {
            if row.record.instruction_len == 0 && snapshot.starts {
                row.record.pc = u32::from(address);
            }
            append_byte(&mut row.record, byte);
        }
        row.after = snapshot.after;
        row.end = receipt.after;
        row.kind = receipt.kind;
        row.complete = receipt.complete;
        if receipt.complete {
            self.current = None;
        }
    }
    pub(crate) fn accept(
        &mut self,
        events: [Option<Event>; HALF_EVENTS],
        overflow: bool,
        cpu: &Cpu,
        output: &mut InstructionTraceStore,
    ) {
        if !self.enabled {
            return;
        }
        for event in events.into_iter().flatten() {
            match event.fact {
                Fact::Activated(binding) => self.activation(&event, binding, output),
                Fact::CpuSnapshot(snapshot) => {
                    if self.snapshot.is_some() {
                        self.gap(output);
                    }
                    self.snapshot = Some(snapshot);
                }
                Fact::Retired(e) => self.retirement(
                    &event,
                    Retirement {
                        complete: e.complete,
                        byte: e.consumed.map(|b| (b.address, b.value)),
                        before: e.cpu_before,
                        after: e.cpu_after,
                        kind: e.kind,
                        lost: e.input_overflowed,
                    },
                    output,
                ),
                Fact::ClassifiedRetired(e) => self.retirement(
                    &event,
                    Retirement {
                        complete: e.complete,
                        byte: e.consumed,
                        before: e.cpu_before,
                        after: e.cpu_after,
                        kind: e.kind,
                        lost: e.input_lost,
                    },
                    output,
                ),
                Fact::LocalWrite { address, value, .. } => {
                    self.local(&event, (address, value), cpu, output)
                }
                Fact::ExternalWriteStrobe { .. } => self.expected(&event, output),
                Fact::WorkCommitted { tag, .. } => self.work(&event, tag, cpu, output),
                Fact::PsgAccepted(receipt) => self.psg(&event, receipt, cpu, output),
                Fact::VdpAccepted(receipt) => self.vdp_write(&event, receipt, cpu, output),
                Fact::Reset => {
                    self.close_all(output, TraceObservationEnd::Reset);
                    self.links = [None; 16];
                    self.snapshot = None;
                }
                Fact::Fault => self.fault(output),
                Fact::Gap => self.gap(output),
                Fact::Sampled(_) | Fact::WorkPulse(_) => {}
            }
        }
        if let Some(id) = self.current
            && let Some(row) = self.row_mut(id)
        {
            row.after = registers(cpu);
            row.end = cpu.cycles();
        }
        if overflow || self.snapshot.is_some() {
            self.gap(output);
        } else {
            self.flush(output);
        }
    }
    fn pending(&self, id: u64) -> bool {
        self.links
            .iter()
            .flatten()
            .any(|link| link.row == Some(id) && link.expected && link.accepted.is_none())
    }
    fn flush(&mut self, output: &mut InstructionTraceStore) {
        while self
            .rows
            .front()
            .is_some_and(|row| row.complete && !self.pending(row.id))
        {
            let row = self.rows.pop_front().unwrap();
            self.publish(row, output, TraceObservationEnd::Retired);
        }
    }
    fn close_all(&mut self, output: &mut InstructionTraceStore, end: TraceObservationEnd) {
        while let Some(row) = self.rows.pop_front() {
            self.publish(row, output, end);
        }
        self.current = None;
    }
    fn publish(
        &mut self,
        mut row: Row,
        output: &mut InstructionTraceStore,
        end: TraceObservationEnd,
    ) {
        row.record.observation.end = end;
        row.record.boundary = Some(TraceBoundary {
            kind: if end == TraceObservationEnd::Fault {
                TraceBoundaryKind::Fault
            } else {
                match row.kind {
                    RetirementKind::Interrupt => TraceBoundaryKind::Interrupt,
                    RetirementKind::HaltPoll => TraceBoundaryKind::Stopped,
                    RetirementKind::Instruction => TraceBoundaryKind::Instruction,
                }
            },
            cpu_clocks: row.end.saturating_sub(row.record.cycle),
            final_exception: None,
            machine_fault: end == TraceObservationEnd::Fault,
        });
        if row.end < row.record.cycle {
            row.record.observation.start = TraceObservationStart::Suffix;
            self.gaps = self.gaps.saturating_add(1);
        }
        if row.kind == RetirementKind::Interrupt {
            row.record.event = Some(DebugEvent::Interrupt);
        }
        for (register, (before, after)) in row.before.into_iter().zip(row.after).enumerate() {
            if before != after {
                row.record.push_register_delta(RegisterDelta {
                    register: register as u8,
                    value: after,
                });
            }
        }
        let mut writes = row.writes;
        writes.sort_by_key(|write| write.map(|w| (w.stamp, w.ordinal)));
        for write in writes.into_iter().flatten() {
            row.record.push_write(write.write);
        }
        output.push(row.record);
    }
}
