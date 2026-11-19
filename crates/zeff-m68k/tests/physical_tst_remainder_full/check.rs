use super::*;
use device::{DefaultSecondary, Device};
use model::Spec;
use oracle::Expected;
pub(super) fn interrupts(cpu: &mut PrefetchCpu, level: u8, edge: bool) {
    if edge {
        assert!(cpu.interrupt_input().set_level(7));
    }
    assert!(cpu.interrupt_input().set_level(level));
}
pub(super) fn step(
    cpu: &mut PrefetchCpu,
    b: &mut Device,
) -> Result<PhysicalInstructionStep, InstructionFault<&'static str>> {
    if b.default_entry_detector {
        cpu.step_physical_instruction(&mut DefaultSecondary(b))
    } else {
        cpu.step_physical_instruction(b)
    }
}
pub(super) fn freeze(cpu: &mut PrefetchCpu, b: &mut Device, f: TimingFault<&'static str>) -> bool {
    let saved = cpu.clone();
    let device = b.clone();
    let mut copy = saved.clone();
    let mut copied_device = device.clone();
    let mut good = copy.step_physical_instruction(&mut copied_device)
        == Err(InstructionFault::Execution(f.clone()));
    good &= copy == saved && copied_device == device;
    good &= cpu.step_physical_instruction(b) == Err(InstructionFault::Execution(f.clone()));
    good &= *cpu == saved && *b == device;
    good &= cpu.step_instruction(b) == Err(f.clone());
    good &= *cpu == saved && *b == device;
    good &= cpu.step_boundary(b) == Err(f.clone());
    good &= *cpu == saved && *b == device;
    good &= cpu.prime(b) == Err(f.clone());
    good &= *cpu == saved && *b == device;
    good &= cpu.service_pending_trace(b) == Err(f);
    good &= *cpu == saved && *b == device;
    good
}
pub(super) fn rejected(
    cpu: &mut PrefetchCpu,
    b: &mut Device,
    error: InstructionFault<&'static str>,
) -> bool {
    let saved = cpu.clone();
    let device = b.clone();
    let mut copy = saved.clone();
    let mut copied_device = device.clone();
    copy.step_physical_instruction(&mut copied_device) == Err(error.clone())
        && copy == saved
        && copied_device == device
        && cpu.step_physical_instruction(b) == Err(error)
        && *cpu == saved
        && *b == device
}
pub(super) fn private(cpu: &PrefetchCpu, e: &Expected) -> bool {
    let pipeline = if let Some(words) = e.queue {
        format!("Some(Pipeline {{ address: {}, words: {words:?} }})", e.r.pc)
    } else {
        "None".to_owned()
    };
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
    format!("{cpu:?}") == expected
}
pub(super) fn verify(cpu: &PrefetchCpu, b: &Device, e: &Expected, config: &Device) -> bool {
    cpu.registers == e.r
        && cpu.queued_words() == e.queue
        && cpu.clock().0 == e.clock
        && b.clock == e.clock
        && b.words == e.words
        && b.events == e.events
        && b.count == e.count
        && b.entry_count == e.entries
        && b.scripts == e.scripts
        && b.mutation_count == e.mutation_count
        && b.elapsed == config.elapsed
        && b.report == config.report
        && b.mutation == config.mutation
        && b.entry_failure == config.entry_failure
        && b.entry_wait == config.entry_wait
        && b.entry_detector == config.entry_detector
        && b.default_entry_detector == config.default_entry_detector
        && b.default_detector == config.default_detector
        && b.detector == config.detector
        && b.default_word == config.default_word
        && b.write_deposit == config.write_deposit
        && b.entry_write_deposit == config.entry_write_deposit
        && b.omitted_detectors == 0
        && cpu.physical_entry_state() == e.state
        && cpu.physical_inspection() == e.inspection
        && !cpu.trace_pending()
        && !cpu.stopped()
        && private(cpu, e)
}
pub(super) fn run(s: Spec, configure: impl Fn(&mut Device), mark: fn(bool)) -> bool {
    let (_, _, mut setup, _, _) = s.seed();
    configure(&mut setup);
    let e = oracle::predict(s, &setup);
    mark(true);
    let (r, q, mut b, _, _) = s.seed();
    configure(&mut b);
    let mut cpu = PrefetchCpu::from_prefetched(r, q, CpuClocks(s.start)).unwrap();
    let actual = step(&mut cpu, &mut b);
    let mut guarded = true;
    if let Some(f) = e.guard.clone() {
        guarded = freeze(&mut cpu, &mut b, f);
    }
    mark(false);
    let good = guarded && actual == e.result && verify(&cpu, &b, &e, &setup);
    diagnostic(good, s, &cpu, &b, &e, &actual);
    good
}
pub(super) fn diagnostic(
    good: bool,
    s: Spec,
    cpu: &PrefetchCpu,
    b: &Device,
    e: &Expected,
    result: &Result<PhysicalInstructionStep, InstructionFault<&'static str>>,
) {
    #[cfg(not(target_arch = "wasm32"))]
    if !good {
        use std::sync::atomic::{AtomicU32, Ordering};
        static BAD: AtomicU32 = AtomicU32::new(0);
        if BAD.fetch_add(1, Ordering::Relaxed) < 8 {
            eprintln!(
                "TST full spec={s:?} actual={result:?} expected={:?} cpu={cpu:?} expectedCPUregs={:?} queue={:?} state={:?} inspect={:?} guard={:?} b={b:?} expectedEvents={:?} RAM={:?}",
                e.result, e.r, e.queue, e.state, e.inspection, e.guard, e.events, e.words
            );
        }
    }
    #[cfg(target_arch = "wasm32")]
    let _ = (good, s, cpu, b, e, result);
}
