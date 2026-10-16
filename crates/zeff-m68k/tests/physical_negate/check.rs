use super::*;
use device::Device;
use model::{Case, Event};

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
        && !cpu.trace_pending()
        && !cpu.stopped()
        && cpu.interrupt_input().level() == 0
        && private.matches("pipeline:").count() == 1
        && private.matches("frozen:").count() == 1
        && private.contains(&pipeline)
        && private.contains(&format!("frozen: {},", c.frozen));
    let before = c
        .before_entry
        .as_ref()
        .map(|(clock, words)| (*clock, words.iter().copied().collect()));
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
            "physical NEG/NEGX {} unsupported={unsupported} actual={actual:?} expected={:?} regs={:?}/{:?} queue={:?}/{:?} clock={}/{} state={:?}/{:?} device={bus:?}",
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
    let _ = (c.label, diagnose);
    (good, unsupported)
}
