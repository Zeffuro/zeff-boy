use super::*;
use fixture::{Case, Outcome};

pub(super) fn run(case: Case) -> (bool, bool) {
    let mut cpu =
        PrefetchCpu::from_prefetched(case.initial, case.queue, CpuClocks(case.start)).unwrap();
    let mut device = device::Device::new(&case);
    let expected = match case.outcome {
        Outcome::Complete => Ok(TimedStep {
            opcode: case.opcode,
            clocks: case.duration,
            exception: None,
        }),
        Outcome::Host(access, direction, source) => Err(TimingFault::Bus {
            access,
            direction,
            source,
        }),
        Outcome::Overflow => Err(TimingFault::ClockOverflow),
    };
    let actual = cpu.step_instruction(&mut device);
    let mut without_sr = cpu.registers;
    without_sr.set_status_register(case.final_registers.status_register());
    let expected_memory: BTreeMap<_, _> = case.final_ram.iter().copied().collect();
    let mut other = actual == expected
        && without_sr == case.final_registers
        && cpu.queued_words() == Some(case.final_queue)
        && cpu.clock().0 == case.clock
        && device.clock == case.clock
        && device.words == expected_memory
        && device.events == case.events
        && device.count == case.events.len()
        && device.valid
        && !cpu.trace_pending()
        && !cpu.stopped()
        && cpu.interrupt_input().level() == 0
        && cpu.physical_entry_state() == PhysicalEntryState::Idle
        && cpu.physical_inspection() == PhysicalInspection::DiagnosticCompleteValues;
    if case.outcome != Outcome::Complete {
        let frozen = cpu.clone();
        let saved_device = device.clone();
        other &= cpu.step_instruction(&mut device) == Err(TimingFault::Frozen);
        other &= cpu.step_boundary(&mut device) == Err(TimingFault::Frozen);
        other &= cpu.prime(&mut device) == Err(TimingFault::Frozen);
        other &= cpu.service_pending_trace(&mut device) == Err(TimingFault::Frozen);
        other &= cpu.step(&mut device) == Err(TimingFault::Frozen);
        other &= cpu == frozen && device == saved_device;
    }
    let sr_equal = cpu.registers.status_register() == case.final_registers.status_register();
    let good = other && sr_equal;
    if !good {
        #[cfg(not(target_arch = "wasm32"))]
        eprintln!(
            "ordinary RMW literal {} SR-only={} actual={actual:?} expected={expected:?} registers={:?}/{:?} queue={:?}/{:?} clocks={}/{} callbacks={:?}/{:?} RAM={:?}/{:?}",
            case.label,
            other && !sr_equal,
            cpu.registers,
            case.final_registers,
            cpu.queued_words(),
            case.final_queue,
            cpu.clock().0,
            case.clock,
            device.events,
            case.events,
            device.words,
            expected_memory
        );
    }
    let _ = case.label;
    (good, other && !sr_equal)
}
