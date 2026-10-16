use super::*;
use oracle::Expected;
pub(super) fn step(
    cpu: &mut PrefetchCpu,
    b: &mut Device,
) -> Result<PhysicalInstructionStep, InstructionFault<&'static str>> {
    if b.default_secondary {
        cpu.step_physical_instruction(&mut device::DefaultSecondary(b))
    } else {
        cpu.step_physical_instruction(b)
    }
}
pub(super) fn freeze(cpu: &mut PrefetchCpu, b: &mut Device, f: TimingFault<&'static str>) -> bool {
    let saved = cpu.clone();
    let device = b.clone();
    let mut copy = saved.clone();
    let mut copied = device.clone();
    let mut good =
        copy.step_physical_instruction(&mut copied) == Err(InstructionFault::Execution(f.clone()));
    good &= copy == saved && copied == device;
    if !good {
        return false;
    }
    good &= cpu.step_physical_instruction(b) == Err(InstructionFault::Execution(f.clone()));
    good &= *cpu == saved && *b == device;
    if !good {
        return false;
    }
    good &= cpu.step_instruction(b) == Err(f.clone());
    good &= *cpu == saved && *b == device;
    if !good {
        return false;
    }
    good &= cpu.step_boundary(b) == Err(f.clone());
    good &= *cpu == saved && *b == device;
    if !good {
        return false;
    }
    good &= cpu.prime(b) == Err(f.clone());
    good &= *cpu == saved && *b == device;
    if !good {
        return false;
    }
    good &= cpu.service_pending_trace(b) == Err(f);
    good &= *cpu == saved && *b == device;
    good
}
pub(super) fn verify(cpu: &PrefetchCpu, b: &Device, e: &Expected, config: &Device) -> bool {
    let pipeline = e.queue.map_or_else(
        || "None".to_owned(),
        |q| format!("Some(Pipeline {{ address: {}, words: {q:?} }})", e.r.pc),
    );
    let expected = format!(
        "PrefetchCpu {{ registers: {:?}, clock: CpuClocks({}), pipeline: {}, frozen: {}, stopped: false, interrupts: InterruptState {{ level: {}, level7_edge: {} }}, pending_trace: false, physical_state: {:?}, physical_inspection: {:?}, imported_boundary: None }}",
        e.r,
        e.clock,
        pipeline,
        e.guard == Some(TimingFault::Frozen),
        e.level,
        e.edge,
        e.state,
        e.inspection
    );
    let mut device = config.clone();
    device.words = e.words.clone();
    device.events = e.events.clone();
    device.scripts = e.scripts.clone();
    device.clock = e.clock;
    device.count = e.count;
    device.entry_count = e.entries;
    device.mutations = e.mutations;
    cpu.registers == e.r
        && cpu.queued_words() == e.queue
        && cpu.clock().0 == e.clock
        && cpu.physical_entry_state() == e.state
        && cpu.physical_inspection() == e.inspection
        && !cpu.trace_pending()
        && !cpu.stopped()
        && *b == device
        && format!("{cpu:?}") == expected
}
pub(super) fn run(c: Case, configure: impl Fn(&mut Device), mark: fn(bool)) -> bool {
    let mut config = Device::seed(c);
    configure(&mut config);
    let e = oracle::predict(c, &config);
    mark(true);
    let mut b = config.clone();
    let mut cpu =
        PrefetchCpu::from_prefetched(c.r, [c.opcode(), c.retained()], CpuClocks(c.start)).unwrap();
    let actual = step(&mut cpu, &mut b);
    let guarded = actual == e.result && e.guard.clone().is_none_or(|f| freeze(&mut cpu, &mut b, f));
    mark(false);
    let good = guarded && actual == e.result && verify(&cpu, &b, &e, &config);
    #[cfg(not(target_arch = "wasm32"))]
    if !good {
        use std::sync::atomic::{AtomicU32, Ordering};
        static BAD: AtomicU32 = AtomicU32::new(0);
        if BAD.fetch_add(1, Ordering::Relaxed) < 8 {
            eprintln!(
                "CMPI full word={:04x} actual={actual:?} expected={:?} r={:?} expectedR={:?} b={b:?} expectedEvents={:?} state={:?} queue={:?}",
                c.opcode(),
                e.result,
                cpu.registers,
                e.r,
                e.events,
                e.state,
                e.queue
            );
        }
    }
    good
}
