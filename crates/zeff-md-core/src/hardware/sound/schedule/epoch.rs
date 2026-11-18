use super::*;
use crate::sound_trace::{AttemptCapture, CapturedBus, SoundCapture};

mod bus;
#[cfg(test)]
mod tests;

const MAX_SEGMENTS: usize = 8;
const MAX_TICKS: u64 = 512;

impl std::fmt::Debug for SoundSchedule {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SoundSchedule")
            .field("pending", &self.pending)
            .field("rmw_checkpoint", &self.rmw_checkpoint)
            .field("fault", &self.fault)
            .field("state", &self.state)
            .finish()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Record {
    cpu: Cpu,
    state: BusState,
    instruction: FetchedInstruction,
    at: u64,
    start: u64,
    deadline: u64,
}

#[cfg(all(test, feature = "sound-shadow-observer"))]
const _: fn(Record) -> Cpu = |record| record.cpu;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct LocalEpoch {
    records: [Option<Record>; MAX_SEGMENTS],
    len: usize,
    committed: usize,
    prepared: bool,
    blocked: bool,
}

impl Default for LocalEpoch {
    fn default() -> Self {
        Self {
            records: std::array::from_fn(|_| None),
            len: 0,
            committed: 0,
            prepared: false,
            blocked: false,
        }
    }
}

impl LocalEpoch {
    fn build<C: SoundCapture>(
        &mut self,
        cpu: &Cpu,
        state: BusState,
        memory: &SoundMemory,
        now: u64,
        capture: &mut C,
    ) {
        let mut candidate = cpu.clone();
        let mut continuation = state;
        let mut cursor = now;
        let origin = now / SOUND_CPU_DIVISOR * SOUND_CPU_DIVISOR;
        let mut bus = bus::LocalBus {
            memory,
            rejected: Cell::new(false),
        };
        self.len = 0;
        self.committed = 0;
        self.prepared = false;
        capture.begin_epoch();
        for (index, slot) in self.records.iter_mut().enumerate() {
            let mut attempt = C::Attempt::begin(&candidate, continuation);
            let before = candidate.cycles();
            let Some(step) = candidate.step_bus_segment(
                &mut CapturedBus {
                    inner: &mut bus,
                    attempt: &mut attempt,
                },
                &mut continuation,
            ) else {
                break;
            };
            let instruction = step.instruction;
            if bus.rejected.get()
                || candidate.trap().is_some()
                || instruction.cycles == 0
                || before.checked_add(u64::from(instruction.cycles)) != Some(candidate.cycles())
            {
                break;
            }
            let start = cursor / SOUND_CPU_DIVISOR * SOUND_CPU_DIVISOR;
            let Some(deadline) =
                start.checked_add(u64::from(instruction.cycles) * SOUND_CPU_DIVISOR)
            else {
                break;
            };
            if deadline.saturating_sub(origin) > MAX_TICKS {
                break;
            }
            *slot = Some(Record {
                cpu: candidate.clone(),
                state: continuation,
                instruction,
                at: cursor,
                start,
                deadline,
            });
            attempt.finish(instruction, step.complete);
            capture.stage_epoch(index, attempt);
            self.len += 1;
            cursor = deadline;
        }
    }

    fn pending(&self) -> Option<&Record> {
        self.prepared
            .then(|| self.records[self.committed].as_ref().unwrap())
    }

    fn clear(&mut self) {
        self.len = 0;
        self.committed = 0;
        self.prepared = false;
    }
}

impl Record {
    fn into_pending(self, memory: &SoundMemory) -> PendingSound {
        let mut snapshot = memory.snapshot();
        snapshot.irq_asserted = false;
        #[cfg(feature = "sound-shadow-observer")]
        let observer = observer::image(&snapshot.ram.1, None);
        PendingSound {
            cpu: self.cpu,
            memory: snapshot,
            instruction: self.instruction,
            deadline: self.deadline,
            state: self.state,
            writes: WorkWrites::default(),
            register_writes: RegisterWrites::default(),
            sf001_writes: Sf001Writes::default(),
            sf002_writes: Sf002Writes::default(),
            psg_writes: PsgWrites::default(),
            opn2_writes: Opn2Writes::default(),
            bank_access: None,
            observed_bank_access: None,
            any_bank_access: false,
            start: self.start,
            replay: None,
            trace_origin: None,
            svp_history: None,
            #[cfg(feature = "sound-shadow-observer")]
            observer,
        }
    }
}

impl SoundSchedule {
    pub(crate) fn begin_epoch_field(&mut self) {
        self.epoch = Some(LocalEpoch::default());
    }

    pub(crate) fn finish_epoch_field<C: SoundCapture>(
        &mut self,
        cpu: &mut Cpu,
        memory: &SoundMemory,
        capture: &mut C,
    ) {
        self.materialize_epoch(cpu, memory, capture);
        self.epoch = None;
    }

    pub(crate) fn materialize_epoch<C: SoundCapture>(
        &mut self,
        cpu: &mut Cpu,
        memory: &SoundMemory,
        capture: &mut C,
    ) {
        let Some(epoch) = &mut self.epoch else {
            return;
        };
        if epoch.committed != 0 {
            let record = epoch.records[epoch.committed - 1].take().unwrap();
            *cpu = record.cpu;
            self.state = record.state;
        }
        if epoch.prepared {
            self.pending = Some(
                epoch.records[epoch.committed]
                    .take()
                    .unwrap()
                    .into_pending(memory),
            );
        }
        capture.materialize_epoch(epoch.prepared.then_some(epoch.committed));
        epoch.clear();
    }

    pub(crate) fn block_epoch<C: SoundCapture>(
        &mut self,
        cpu: &mut Cpu,
        memory: &SoundMemory,
        capture: &mut C,
    ) -> bool {
        self.materialize_epoch(cpu, memory, capture);
        let Some(epoch) = &mut self.epoch else {
            return true;
        };
        let previous = epoch.blocked;
        epoch.blocked = true;
        previous
    }

    pub(crate) fn restore_epoch(&mut self, blocked: bool) {
        if let Some(epoch) = &mut self.epoch {
            epoch.blocked = blocked;
        }
    }

    pub(super) fn prepare_epoch<C: SoundCapture>(
        &mut self,
        cpu: &mut Cpu,
        memory: &SoundMemory,
        cursor: u64,
        irq: bool,
        capture: &mut C,
    ) -> bool {
        let Some(epoch) = &self.epoch else {
            return false;
        };
        if epoch.blocked || memory.granted || irq {
            self.materialize_epoch(cpu, memory, capture);
            return false;
        }
        if epoch.len != 0 && epoch.committed == epoch.len {
            self.materialize_epoch(cpu, memory, capture);
        }
        let epoch = self.epoch.as_mut().unwrap();
        if epoch.len == 0 {
            epoch.build(cpu, self.state, memory, cursor, capture);
        }
        if epoch.len == 0 {
            return false;
        }
        if epoch.records[epoch.committed].as_ref().unwrap().at != cursor {
            self.materialize_epoch(cpu, memory, capture);
            return false;
        }
        epoch.prepared = true;
        true
    }

    pub(super) fn epoch_deadline(&self) -> Option<u64> {
        Some(self.epoch.as_ref()?.pending()?.deadline)
    }

    pub(super) fn publish_epoch<C: SoundCapture>(
        &mut self,
        memory: &mut SoundMemory,
        audio: &mut Audio,
        capture: &mut C,
    ) {
        let epoch = self.epoch.as_mut().unwrap();
        audio.request_advance_to(epoch.pending().unwrap().deadline);
        memory.complete_boundary();
        epoch.committed += 1;
        capture.publish_epoch(
            epoch.committed - 1,
            &epoch.records[epoch.committed - 1].as_ref().unwrap().cpu,
        );
        epoch.prepared = false;
    }
}
