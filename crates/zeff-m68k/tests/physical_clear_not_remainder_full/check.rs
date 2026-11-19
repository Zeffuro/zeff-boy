use super::device::{DefaultSecondary, Device};
use super::model::Case;
use super::oracle::Expected;
use super::{CpuClocks, InstructionFault, PrefetchCpu, TimingFault};

pub(super) fn step(
    cpu: &mut PrefetchCpu,
    bus: &mut Device,
) -> Result<super::PhysicalInstructionStep, InstructionFault<&'static str>> {
    if bus.default_secondary {
        cpu.step_physical_instruction(&mut DefaultSecondary(bus))
    } else {
        cpu.step_physical_instruction(bus)
    }
}
pub(super) fn private(cpu: &PrefetchCpu, e: &Expected) -> bool {
    let pipeline = match (e.pipeline_address, e.queue) {
        (Some(address), Some(words)) => {
            format!("Some(Pipeline {{ address: {address}, words: {words:?} }})")
        }
        (None, None) => "None".to_owned(),
        _ => return false,
    };
    let expected = format!(
        "PrefetchCpu {{ registers: {:?}, clock: CpuClocks({}), pipeline: {}, frozen: {}, stopped: {}, interrupts: InterruptState {{ level: {}, level7_edge: {} }}, pending_trace: {}, physical_state: {:?}, physical_inspection: {:?}, imported_boundary: {:?} }}",
        e.r,
        e.clock,
        pipeline,
        e.frozen,
        e.stopped,
        e.level,
        e.edge,
        e.pending_trace,
        e.state,
        e.inspection,
        e.imported
    );
    format!("{cpu:?}") == expected
}
pub(super) fn verify(cpu: &PrefetchCpu, bus: &Device, e: &Expected) -> bool {
    verify_at(cpu, bus, e, e.clock)
}
pub(super) fn verify_at(cpu: &PrefetchCpu, bus: &Device, e: &Expected, clock: u64) -> bool {
    cpu.registers == e.r
        && cpu.clock().0 == e.clock
        && cpu.queued_words() == e.queue
        && cpu.physical_entry_state() == e.state
        && cpu.physical_inspection() == e.inspection
        && cpu.stopped() == e.stopped
        && cpu.trace_pending() == e.pending_trace
        && bus.clock == clock
        && bus.words == e.words
        && bus.events == e.events
        && bus.ordinary == e.ordinary
        && bus.scripts == e.scripts
        && bus.count == e.count
        && bus.entry_count == e.entries
        && bus.counts == e.counts
        && bus.mutated == e.mutated
        && bus.before_entry == e.before_entry
        && private(cpu, e)
}
pub(super) fn guards(
    cpu: &mut PrefetchCpu,
    bus: &mut Device,
    fault: TimingFault<&'static str>,
    mark: fn(bool),
) -> bool {
    mark(true);
    let saved = cpu.clone();
    let device = bus.clone();
    let mut copy = cpu.clone();
    let mut copied_bus = bus.clone();
    let good = step(&mut copy, &mut copied_bus) == Err(InstructionFault::Execution(fault.clone()))
        && copy == saved
        && copied_bus == device;
    if !good {
        mark(false);
        return false;
    }
    let good = step(cpu, bus) == Err(InstructionFault::Execution(fault.clone()))
        && *cpu == saved
        && *bus == device;
    if !good {
        mark(false);
        return false;
    }
    let good = cpu.step_instruction(bus) == Err(fault.clone()) && *cpu == saved && *bus == device;
    if !good {
        mark(false);
        return false;
    }
    let good = cpu.step_boundary(bus) == Err(fault.clone()) && *cpu == saved && *bus == device;
    if !good {
        mark(false);
        return false;
    }
    let good = cpu.prime(bus) == Err(fault.clone()) && *cpu == saved && *bus == device;
    if !good {
        mark(false);
        return false;
    }
    let good = cpu.service_pending_trace(bus) == Err(fault) && *cpu == saved && *bus == device;
    mark(false);
    good
}
pub(super) fn actual(
    c: Case,
    config: Device,
    e: &Expected,
    mark: fn(bool),
) -> Option<(PrefetchCpu, Device)> {
    mark(true);
    let mut bus = config.clone();
    let mut cpu =
        PrefetchCpu::from_prefetched(c.r, [c.word, c.retained], CpuClocks(c.start)).unwrap();
    let result = step(&mut cpu, &mut bus);
    mark(false);
    if result != e.result || !verify(&cpu, &bus, e) {
        return None;
    }
    if let Some(fault) = e.guard.clone()
        && !guards(&mut cpu, &mut bus, fault, mark)
    {
        return None;
    }
    Some((cpu, bus))
}
pub(super) fn run(c: Case, configure: impl FnOnce(&mut Device), mark: fn(bool)) -> bool {
    let mut config = Device::seed(c);
    configure(&mut config);
    let expected = super::oracle::predict(c, &config);
    actual(c, config, &expected, mark).is_some()
}
