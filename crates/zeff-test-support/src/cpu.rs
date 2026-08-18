use std::fmt::Debug;

use zeff_emu_common::debug::BusAccessEvent;
use zeff_emu_common::time::{ClockRate, MasterTicks};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemoryBlock {
    pub start: u32,
    pub bytes: Vec<u8>,
}

impl MemoryBlock {
    pub fn new(start: u32, bytes: impl Into<Vec<u8>>) -> Self {
        Self {
            start,
            bytes: bytes.into(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TraceTiming {
    Exact,
    OrderOnly,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StepKind {
    Instruction,
    Interrupt,
    Idle,
    Trap,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StepObservation {
    pub kind: StepKind,
    pub cpu_cycles: u64,
    pub master_ticks: MasterTicks,
    pub master_rate: ClockRate,
    pub bus_events: Vec<BusAccessEvent>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CpuCase<S> {
    pub initial_state: S,
    pub expected_state: S,
    pub initial_memory: Vec<MemoryBlock>,
    pub expected_memory: Vec<MemoryBlock>,
    pub expected_step: StepObservation,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CaseOutcome<S> {
    pub state: S,
    pub memory: Vec<MemoryBlock>,
    pub step: StepObservation,
}

pub trait CpuConformanceAdapter: Sized {
    type State: Clone + Debug + Eq;

    const TRACE_TIMING: TraceTiming;

    fn from_case(case: &CpuCase<Self::State>) -> Self;
    fn snapshot(&self) -> Self::State;
    fn peek8(&self, address: u32) -> u8;
    fn step(&mut self) -> StepObservation;
}

pub fn assert_case<A>(case: &CpuCase<A::State>)
where
    A: CpuConformanceAdapter,
{
    let first = run_case::<A>(case);
    let second = run_case::<A>(case);
    assert_eq!(first, second, "fresh conformance runs diverged");
}

pub fn run_case<A>(case: &CpuCase<A::State>) -> CaseOutcome<A::State>
where
    A: CpuConformanceAdapter,
{
    let mut adapter = A::from_case(case);
    assert_eq!(adapter.snapshot(), case.initial_state, "initial CPU state");

    let step = adapter.step();
    validate_observation(A::TRACE_TIMING, &step);
    assert_eq!(step, case.expected_step, "CPU step observation");

    let state = adapter.snapshot();
    assert_eq!(state, case.expected_state, "final CPU state");
    let memory = snapshot_memory(&adapter, &case.expected_memory);
    assert_eq!(memory, case.expected_memory, "final memory");

    CaseOutcome {
        state,
        memory,
        step,
    }
}

fn snapshot_memory<A>(adapter: &A, expected: &[MemoryBlock]) -> Vec<MemoryBlock>
where
    A: CpuConformanceAdapter,
{
    expected
        .iter()
        .map(|block| MemoryBlock {
            start: block.start,
            bytes: (0..block.bytes.len())
                .map(|offset| {
                    let offset = u32::try_from(offset).expect("test memory block offset");
                    adapter.peek8(block.start.wrapping_add(offset))
                })
                .collect(),
        })
        .collect()
}

fn validate_observation(trace_timing: TraceTiming, step: &StepObservation) {
    let mut previous_tick = None;
    for event in &step.bus_events {
        match (trace_timing, event.at()) {
            (TraceTiming::Exact, Some(at)) => {
                assert!(
                    at <= step.master_ticks,
                    "bus event at {at} exceeds step duration {}",
                    step.master_ticks
                );
                if let Some(previous) = previous_tick {
                    assert!(at >= previous, "bus event timestamps are not monotonic");
                }
                previous_tick = Some(at);
            }
            (TraceTiming::OrderOnly, None) => {}
            (TraceTiming::Exact, None) => panic!("exact trace omitted a bus timestamp"),
            (TraceTiming::OrderOnly, Some(_)) => {
                panic!("order-only trace claimed an exact bus timestamp")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zeff_emu_common::debug::{TraceWriteKind, TraceWriteWidth};

    fn read(at: Option<MasterTicks>) -> BusAccessEvent {
        BusAccessEvent::Read {
            at,
            space: TraceWriteKind::Memory,
            addr: 0,
            value: 0,
            width: TraceWriteWidth::Byte,
            mapped_addr: None,
        }
    }

    fn observation(events: Vec<BusAccessEvent>) -> StepObservation {
        StepObservation {
            kind: StepKind::Instruction,
            cpu_cycles: 4,
            master_ticks: MasterTicks::new(4),
            master_rate: ClockRate::from_hz(1),
            bus_events: events,
        }
    }

    #[test]
    fn exact_trace_accepts_ordered_timestamps() {
        validate_observation(
            TraceTiming::Exact,
            &observation(vec![
                read(Some(MasterTicks::new(1))),
                read(Some(MasterTicks::new(4))),
            ]),
        );
    }

    #[test]
    fn order_only_trace_accepts_missing_timestamps() {
        validate_observation(TraceTiming::OrderOnly, &observation(vec![read(None)]));
    }

    #[test]
    #[should_panic(expected = "exact trace omitted a bus timestamp")]
    fn exact_trace_rejects_missing_timestamp() {
        validate_observation(TraceTiming::Exact, &observation(vec![read(None)]));
    }

    #[test]
    #[should_panic(expected = "bus event timestamps are not monotonic")]
    fn exact_trace_rejects_reordered_timestamps() {
        validate_observation(
            TraceTiming::Exact,
            &observation(vec![
                read(Some(MasterTicks::new(2))),
                read(Some(MasterTicks::new(1))),
            ]),
        );
    }
}
