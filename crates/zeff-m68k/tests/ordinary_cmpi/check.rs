use super::*;
use device::Event;
use fixture::{Case, Outcome};

pub(super) fn run(mut c: Case) -> bool {
    let mut cpu = PrefetchCpu::from_prefetched(c.r, c.queue, CpuClocks(c.bus.clock)).unwrap();
    let memory = c.bus.words.clone();
    let expected = match c.outcome {
        Outcome::Complete => Ok(TimedStep {
            opcode: c.opcode,
            clocks: c.duration,
            exception: None,
        }),
        Outcome::Host(access) => Err(TimingFault::Bus {
            access,
            direction: Direction::Read,
            source: "host",
        }),
        Outcome::Overflow => Err(TimingFault::ClockOverflow),
    };
    let actual = cpu.step_instruction(&mut c.bus);
    let reads = c
        .events
        .iter()
        .filter(|e| matches!(e, Event::Read(..)))
        .count();
    let mut good = actual == expected
        && cpu.registers == c.final_r
        && cpu.queued_words() == Some(c.final_queue)
        && cpu.clock().0 == c.clock
        && c.bus.clock == c.clock
        && c.bus.words == memory
        && c.bus.events == c.events
        && c.bus.count == reads
        && !cpu.trace_pending()
        && !cpu.stopped()
        && cpu.physical_entry_state() == PhysicalEntryState::Idle
        && cpu.physical_inspection() == PhysicalInspection::DiagnosticCompleteValues;
    if !good {
        #[cfg(not(target_arch = "wasm32"))]
        eprintln!(
            "ordinary CMPI {} actual={actual:?} expected={expected:?} regs={:?}/{:?} queue={:?}/{:?} clock={}/{} events={:?}/{:?}",
            c.label,
            cpu.registers,
            c.final_r,
            cpu.queued_words(),
            c.final_queue,
            cpu.clock().0,
            c.clock,
            c.bus.events,
            c.events
        );
    }
    let _ = c.label;
    if c.outcome != Outcome::Complete {
        let frozen = cpu.clone();
        let device = c.bus.clone();
        good &= cpu.step_instruction(&mut c.bus) == Err(TimingFault::Frozen)
            && cpu.step_boundary(&mut c.bus) == Err(TimingFault::Frozen)
            && cpu.prime(&mut c.bus) == Err(TimingFault::Frozen)
            && cpu.service_pending_trace(&mut c.bus) == Err(TimingFault::Frozen)
            && cpu == frozen
            && c.bus == device;
    }
    good
}
