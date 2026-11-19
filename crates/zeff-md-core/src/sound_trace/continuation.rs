use super::attempt::{append_byte, registers};
use super::*;
use zeff_emu_common::debug::{
    DebugEvent, InstructionTraceRecord, RegisterDelta, TraceBoundary, TraceBoundaryKind,
};

#[derive(Clone)]
struct Partial {
    record: InstructionTraceRecord,
    before: [u32; 14],
    after: [u32; 14],
    end: u64,
    interrupt: bool,
    halted: bool,
}

#[derive(Clone)]
struct CapturedAttempt {
    attempt: Attempt,
    trace: bool,
    watch: bool,
}

#[derive(Clone, Default)]
struct State {
    partial: Option<Partial>,
    pending: Option<CapturedAttempt>,
    rmw: Option<CapturedAttempt>,
    epoch: [Option<CapturedAttempt>; 8],
}

impl State {
    fn captures_mut(&mut self) -> impl Iterator<Item = &mut Option<CapturedAttempt>> {
        [&mut self.pending, &mut self.rmw]
            .into_iter()
            .chain(&mut self.epoch)
    }

    fn discard_trace(&mut self) {
        for slot in self.captures_mut() {
            let _ = slot.take_if(|capture| {
                capture.trace = false;
                !capture.watch
            });
        }
    }

    fn discard_watches(&mut self) {
        for slot in self.captures_mut() {
            let _ = slot.take_if(|capture| {
                capture.watch = false;
                !capture.trace
            });
        }
    }
}

#[derive(Default)]
pub struct SoundTraceContinuation {
    state: State,
    watches: super::watch::WatchState,
    physical: super::physical::Journal,
    producer: super::physical::producer::Fold,
    gaps: u64,
}

pub struct SoundTraceAttachment<'a> {
    continuation: &'a mut SoundTraceContinuation,
    output: &'a mut InstructionTraceStore,
}

pub struct SoundTraceFork {
    machine: Box<crate::Emulator>,
    trace: TraceCheckpoint,
}

impl SoundTraceFork {
    pub fn restore(
        self,
        machine: &mut crate::Emulator,
        continuation: &mut SoundTraceContinuation,
        output: &mut InstructionTraceStore,
    ) {
        *machine = *self.machine;
        continuation.state = self.trace.state;
        continuation.watches = self.trace.watches;
        continuation.physical.restore(self.trace.physical);
        continuation.producer = self.trace.producer;
        continuation.gaps = self.trace.gaps;
        *output = self.trace.output;
    }
}

pub(crate) struct TraceCheckpoint {
    state: State,
    watches: super::watch::WatchState,
    physical: super::physical::Checkpoint,
    producer: super::physical::producer::Fold,
    gaps: u64,
    output: InstructionTraceStore,
}

impl SoundTraceContinuation {
    // The caller pairs this owner with one execution branch; machine clones carry no witness.
    pub fn attach<'a>(
        &'a mut self,
        output: &'a mut InstructionTraceStore,
    ) -> SoundTraceAttachment<'a> {
        SoundTraceAttachment {
            continuation: self,
            output,
        }
    }
    // This rollback witness owns the machine and observation captured by one exclusive pair.
    pub fn fork(
        &mut self,
        machine: &mut crate::Emulator,
        output: &mut InstructionTraceStore,
    ) -> SoundTraceFork {
        SoundTraceFork {
            machine: Box::new(machine.clone()),
            trace: self.attach(output).checkpoint(),
        }
    }
    pub fn watchpoints(&self) -> &[super::SoundWatchpoint] {
        &self.watches.rules
    }
    pub fn watch_hit(&self) -> Option<super::SoundWatchHit> {
        self.watches.first
    }
    pub fn watch_gaps(&self) -> u64 {
        self.watches.gaps
    }
    pub fn watch_blocked(&self) -> bool {
        self.watches.blocked
    }
    pub fn watches_armed(&self) -> bool {
        self.watches.armed()
    }
    pub fn clear_watch_hit(&mut self) {
        self.watches.first = None;
    }
    pub fn edit_watchpoint(
        &mut self,
        machine: &crate::Emulator,
        rule: super::SoundWatchRule,
        add: bool,
    ) -> Result<bool, &'static str> {
        if let Some(reason) = machine.passive_sound_trace_rejection() {
            return Err(reason);
        }
        let changed = self.watches.edit(rule, add)?;
        if changed {
            self.state.discard_watches();
            self.watches.gap();
        }
        Ok(changed)
    }
    pub fn gaps(&self) -> u64 {
        self.gaps.saturating_add(self.producer.gaps())
    }
    pub fn disable(&mut self, output: &mut InstructionTraceStore) {
        self.close(output, TraceObservationEnd::Disabled);
        self.state.discard_trace();
        self.producer.disable(output);
        self.physical.set_enabled(false);
        output.set_enabled(false);
    }
    pub fn discontinuity(&mut self, output: &mut InstructionTraceStore) {
        self.trace_discontinuity(output);
        self.state = State::default();
        self.watches.gap();
    }
    pub fn trace_discontinuity(&mut self, output: &mut InstructionTraceStore) {
        self.close(output, TraceObservationEnd::Disabled);
        self.state.discard_trace();
        self.gaps = self.gaps.saturating_add(1);
        self.producer.discontinuity(output);
        self.physical.discontinuity();
    }
    fn close(&mut self, output: &mut InstructionTraceStore, end: TraceObservationEnd) {
        let Some(mut partial) = self.state.partial.take() else {
            return;
        };
        partial.record.observation.end = end;
        let kind = if end == TraceObservationEnd::Fault {
            TraceBoundaryKind::Fault
        } else if partial.interrupt {
            TraceBoundaryKind::Interrupt
        } else if partial.halted {
            TraceBoundaryKind::Stopped
        } else {
            TraceBoundaryKind::Instruction
        };
        partial.record.boundary = Some(TraceBoundary {
            kind,
            cpu_clocks: partial.end.saturating_sub(partial.record.cycle),
            final_exception: None,
            machine_fault: end == TraceObservationEnd::Fault,
        });
        if partial.interrupt {
            partial.record.event = Some(DebugEvent::Interrupt);
        }
        for (register, (before, after)) in partial.before.into_iter().zip(partial.after).enumerate()
        {
            if before != after {
                partial.record.push_register_delta(RegisterDelta {
                    register: register as u8,
                    value: after,
                });
            }
        }
        output.push(partial.record);
    }
    fn accept(&mut self, output: &mut InstructionTraceStore, attempt: Option<Attempt>, cpu: &Cpu) {
        let Some(attempt) = attempt else {
            self.trace_discontinuity(output);
            return;
        };
        let record = attempt.record.into_inner();
        let partial = self.state.partial.get_or_insert(Partial {
            record,
            before: attempt.before,
            after: attempt.before,
            end: record.cycle,
            interrupt: false,
            halted: attempt.halted,
        });
        if partial.end != record.cycle {
            self.trace_discontinuity(output);
            return;
        }
        if partial.record.cycle != record.cycle {
            for &byte in record.instruction_bytes() {
                append_byte(&mut partial.record, byte);
            }
            partial.record.instruction_overflow = partial
                .record
                .instruction_overflow
                .saturating_add(record.instruction_overflow);
            for &write in record.writes() {
                partial.record.push_write(write);
            }
            partial.record.write_overflow = partial
                .record
                .write_overflow
                .saturating_add(record.write_overflow);
        }
        partial.end = cpu.cycles();
        partial.after = registers(cpu);
        partial.interrupt = cpu.last_step_was_interrupt();
        if attempt.complete {
            self.close(output, TraceObservationEnd::Retired);
        }
    }
}

impl SoundCapture for SoundTraceAttachment<'_> {
    type Physical<'a>
        = super::physical::producer::Attachment<'a>
    where
        Self: 'a;
    fn physical(&mut self) -> Self::Physical<'_> {
        super::physical::producer::Attachment::new(
            &mut self.continuation.physical,
            &mut self.continuation.producer,
            self.output,
        )
    }
    type Attempt = Attempt;
    type Checkpoint = TraceCheckpoint;
    fn whole(
        cpu: &Cpu,
        memory: &crate::hardware::sound::SoundMemory,
        cartridge: &crate::cartridge::Cartridge,
        now: u64,
    ) -> Result<(crate::hardware::sound::schedule::PendingSound, Attempt), crate::SoundFault> {
        use crate::hardware::sound::schedule::{PendingSound, Preparation, PreparationPolicy};
        PendingSound::prepare_captured::<Attempt>(
            cpu,
            memory,
            cartridge,
            Preparation {
                now,
                state: BusState::default(),
                bounded: false,
                opn2_status: 0,
            },
            PreparationPolicy {
                irq_asserted: memory.irq_asserted,
                legacy_relative: None,
            },
        )
    }
    fn prepared(&mut self, attempt: Attempt) {
        self.continuation.state.pending = self.capture(attempt);
    }
    fn discard_pending(&mut self) {
        self.continuation.state.pending = None;
    }
    fn publish(&mut self, cpu: &Cpu) {
        let capture = self.continuation.state.pending.take();
        self.accept_capture(capture, cpu);
    }
    fn fault(&mut self) {
        self.continuation
            .close(self.output, TraceObservationEnd::Fault);
        self.continuation.producer.fault(self.output);
    }
    fn reset(&mut self) {
        self.continuation
            .close(self.output, TraceObservationEnd::Reset);
        self.continuation.state = State::default();
        self.continuation.watches.lost();
        self.continuation.physical.discontinuity();
        self.continuation.producer.reset(self.output);
    }
    fn backup_pending(&mut self) {
        self.continuation.state.rmw = self.continuation.state.pending.clone();
    }
    fn finish_rmw(&mut self, restored: bool) {
        let backup = self.continuation.state.rmw.take();
        if restored {
            self.continuation.state.pending = backup;
        }
    }
    fn begin_epoch(&mut self) {
        self.continuation.state.epoch = std::array::from_fn(|_| None);
    }
    fn stage_epoch(&mut self, index: usize, attempt: Attempt) {
        self.continuation.state.epoch[index] = self.capture(attempt);
    }
    fn publish_epoch(&mut self, index: usize, cpu: &Cpu) {
        let capture = self.continuation.state.epoch[index].take();
        self.accept_capture(capture, cpu);
    }
    fn materialize_epoch(&mut self, prepared: Option<usize>) {
        if let Some(index) = prepared {
            self.continuation.state.pending = self.continuation.state.epoch[index].take();
        }
        self.begin_epoch();
    }
    fn checkpoint(&self) -> TraceCheckpoint {
        TraceCheckpoint {
            state: self.continuation.state.clone(),
            watches: self.continuation.watches.clone(),
            physical: self.continuation.physical.checkpoint(),
            producer: self.continuation.producer.clone(),
            gaps: self.continuation.gaps,
            output: self.output.clone(),
        }
    }
    fn rollback(&mut self, checkpoint: TraceCheckpoint) {
        self.continuation.state = checkpoint.state;
        self.continuation.watches = checkpoint.watches;
        self.continuation.physical.restore(checkpoint.physical);
        self.continuation.producer = checkpoint.producer;
        self.continuation.gaps = checkpoint.gaps;
        *self.output = checkpoint.output;
    }
}

impl SoundTraceAttachment<'_> {
    fn capture(&self, attempt: Attempt) -> Option<CapturedAttempt> {
        let trace = self.output.is_enabled();
        let watch = self.continuation.watches.armed();
        (trace || watch).then_some(CapturedAttempt {
            attempt,
            trace,
            watch,
        })
    }

    fn accept_capture(&mut self, capture: Option<CapturedAttempt>, cpu: &Cpu) {
        if let Some(capture) = capture.as_ref().filter(|capture| capture.watch) {
            self.continuation.watches.accept(
                &capture.attempt,
                cpu.cycles(),
                cpu.last_step_was_interrupt(),
            );
        } else {
            self.continuation.watches.lost();
        }
        if self.output.is_enabled() {
            let attempt = capture
                .filter(|capture| capture.trace)
                .map(|capture| capture.attempt);
            self.continuation.accept(self.output, attempt, cpu);
        }
    }

    #[cfg(test)]
    pub(crate) fn physical_journal(&mut self) -> &mut super::physical::Journal {
        self.continuation
            .physical
            .set_enabled(self.output.is_enabled());
        &mut self.continuation.physical
    }
}
