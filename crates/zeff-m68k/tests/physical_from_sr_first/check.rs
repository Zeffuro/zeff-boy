use super::*;
use device::Device;
use model::{Case, Event};

fn private_state(cpu: &PrefetchCpu, c: &Case) -> bool {
    let words = c.expected.queue.unwrap_or([0, 0]);
    let image =
        PrefetchCpu::from_prefetched(c.expected.registers, words, CpuClocks(c.expected.clock))
            .unwrap();
    let mut text = format!("{image:?}");
    let original = format!(
        "pipeline: Some(Pipeline {{ address: {}, words: {words:?} }}),",
        c.expected.registers.pc
    );
    let pipeline = match c.queue_address {
        Some(address) => format!(
            "pipeline: Some(Pipeline {{ address: {address}, words: {:?} }}),",
            c.expected.queue.unwrap()
        ),
        None => "pipeline: None,".to_owned(),
    };
    for (old, new) in [
        (original, pipeline),
        (
            "frozen: false,".to_owned(),
            format!("frozen: {},", c.frozen),
        ),
        (
            "physical_state: Idle,".to_owned(),
            format!("physical_state: {:?},", c.state),
        ),
        (
            "physical_inspection: DiagnosticCompleteValues,".to_owned(),
            format!("physical_inspection: {:?},", c.inspection),
        ),
    ] {
        if text.matches(&old).count() != 1 {
            return false;
        }
        text = text.replacen(&old, &new, 1);
    }
    format!("{cpu:?}") == text
}

pub(super) fn run(c: Case, diagnose: bool) -> (bool, bool) {
    let mut bus = Device::new(&c);
    let mut cpu = PrefetchCpu::from_prefetched(
        c.initial.registers,
        c.initial.queue.unwrap(),
        CpuClocks(c.initial.clock),
    )
    .unwrap();
    let actual = cpu.step_physical_instruction(&mut bus);
    let unsupported = matches!(
        actual,
        Err(InstructionFault::Execution(
            TimingFault::UnsupportedInstruction { .. }
        ))
    );
    let private = format!("{cpu:?}");
    let pipeline = match c.queue_address {
        Some(address) => format!(
            "pipeline: Some(Pipeline {{ address: {address}, words: {:?} }}),",
            c.expected.queue.unwrap()
        ),
        None => "pipeline: None,".to_owned(),
    };
    let mut good = actual == c.result
        && cpu.registers == c.expected.registers
        && cpu.queued_words() == c.expected.queue
        && cpu.clock().0 == c.expected.clock
        && bus.clock == c.expected.clock
        && bus.words == c.final_memory.iter().copied().collect()
        && bus.events == c.events
        && bus.counts == c.counts
        && cpu.physical_entry_state() == c.state
        && cpu.physical_inspection() == c.inspection
        && cpu.trace_pending() == c.pending_trace
        && cpu.stopped() == c.stopped
        && cpu.interrupt_input().level() == c.interrupt_level
        && cpu.clock().0.checked_sub(c.initial.clock) == Some(c.elapsed)
        && private.matches("imported_boundary:").count() == 1
        && private.contains("imported_boundary: None")
        && private.matches("pipeline:").count() == 1
        && private.matches("frozen:").count() == 1
        && private.contains(&pipeline)
        && private.contains(&format!("frozen: {},", c.frozen))
        && private_state(&cpu, &c);
    let before = c
        .before_entry
        .as_ref()
        .map(|(image, words)| (image.clock, words.iter().copied().collect()));
    good &= bus.before_entry == before;
    let writes: Vec<_> = bus
        .events
        .iter()
        .filter_map(|e| match *e {
            Event::Transfer {
                stage: 0,
                access,
                direction: Direction::Write,
                value: Some(value),
                deposited: true,
                ..
            } => Some((access.logical_address, value)),
            _ => None,
        })
        .collect();
    let frame: Vec<_> = bus
        .events
        .iter()
        .filter_map(|e| match *e {
            Event::Transfer {
                stage: 2,
                access,
                direction: Direction::Write,
                value: Some(value),
                deposited: true,
                ..
            } => Some((access.logical_address, value)),
            _ => None,
        })
        .collect();
    good &= writes == c.writes && frame == c.frame;
    if c.capture_observable {
        good &= matches!(&actual, Ok(PhysicalInstructionStep::Group0 { capture, .. }) if Some((*capture, capture.special_status_word())) == c.capture);
    }
    if matches!(c.result, Ok(PhysicalInstructionStep::Instruction(_))) {
        let expected = PrefetchCpu::from_prefetched(
            c.expected.registers,
            c.expected.queue.unwrap(),
            CpuClocks(c.expected.clock),
        )
        .unwrap();
        good &= cpu == expected;
    }
    if let Some(fault) = c.retry.filter(|_| !unsupported) {
        let saved = cpu.clone();
        let mut copied = cpu.clone();
        let mut copied_bus = bus.clone();
        good &= copied.step_physical_instruction(&mut copied_bus)
            == Err(InstructionFault::Execution(fault.clone()));
        good &= copied == saved && copied_bus == bus;
        let device = bus.clone();
        good &= cpu.step_physical_instruction(&mut bus)
            == Err(InstructionFault::Execution(fault.clone()));
        good &= cpu == saved && bus == device;
        good &= cpu.step_instruction(&mut bus) == Err(fault.clone());
        good &= cpu == saved && bus == device;
        good &= cpu.step_boundary(&mut bus) == Err(fault.clone());
        good &= cpu == saved && bus == device;
        good &= cpu.prime(&mut bus) == Err(fault.clone());
        good &= cpu == saved && bus == device;
        good &= cpu.service_pending_trace(&mut bus) == Err(fault);
        good &= cpu == saved && bus == device;
    }
    if !good && diagnose {
        #[cfg(not(target_arch = "wasm32"))]
        eprintln!(
            "physical MOVE-from-SR {} unsupported={unsupported} actual={actual:?} expected={:?} regs={:?}/{:?} queue={:?}/{:?} clock={}/{} state={:?}/{:?} device={bus:?}",
            c.label,
            c.result,
            cpu.registers,
            c.expected.registers,
            cpu.queued_words(),
            c.expected.queue,
            cpu.clock().0,
            c.expected.clock,
            cpu.physical_entry_state(),
            c.state
        );
    }
    let m = &c.metadata;
    good &= m.opcode == c.initial.queue.unwrap()[0]
        && m.mode <= 7
        && m.register < 8
        && m.supervisor == c.initial.registers.supervisor()
        && m.phase <= 5
        && m.disposition <= 4
        && m.terminal_phase <= 6;
    let _ = (m.failed_write_deposited, m.detector_only, c.label, diagnose);
    (good, unsupported)
}
