use super::*;
use default_bus::{DefaultBus, FixtureBus};
use device::Device;
use model::{Case, Event, Image};
use value::Value;

fn image_cpu(image: Image, level: u8, edge: bool) -> PrefetchCpu {
    let mut cpu = PrefetchCpu::from_prefetched(
        image.registers,
        image.queue.unwrap_or([0, 0]),
        CpuClocks(image.clock),
    )
    .unwrap();
    assert!(cpu.interrupt_input().set_level(level));
    assert_eq!(edge, level == 7);
    cpu
}

pub(super) fn private_state(cpu: &PrefetchCpu, c: &Case) -> bool {
    let image = image_cpu(c.expected, c.expected_level, c.expected_edge);
    let words = c.expected.queue.unwrap_or([0, 0]);
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

fn guard_contract(c: &Case) -> bool {
    let expected = c.raw.get("expected");
    let retries = expected.get("retries").list();
    if c.retry.is_none() {
        return retries.is_empty() && expected.get("copied_retry").optional().is_none();
    }
    let name = expected.get("retry_guard").text();
    let apis = [
        "step_physical_instruction",
        "step_instruction",
        "step_boundary",
        "prime",
        "service_pending_trace",
    ];
    if retries.len() != apis.len() {
        return false;
    }
    let mut good = retries.iter().zip(apis).all(|(r, api)| {
        r.get("API").text() == api
            && r.get("expected_error").text() == name
            && r.get("wholeCPU_unchanged").flag()
            && r.get("wholeDevice_unchanged").flag()
    });
    let copy = expected.get("copied_retry");
    good &= copy.get("API").text() == "step_physical_instruction"
        && copy.get("expected_error").text() == name
        && copy.get("wholeCPU_unchanged").flag()
        && copy.get("wholeDevice_unchanged").flag()
        && copy.get("independent_device_copy").flag();
    good
}

fn guards<B: FixtureBus>(
    cpu: &mut PrefetchCpu,
    bus: &mut B,
    fault: TimingFault<&'static str>,
) -> bool {
    let saved = cpu.clone();
    let device = bus.clone();
    let mut copied = cpu.clone();
    let mut copied_bus = bus.clone();
    let mut good = copied.step_physical_instruction(&mut copied_bus)
        == Err(InstructionFault::Execution(fault.clone()));
    good &= copied == saved && copied_bus == device;
    if !good {
        return false;
    }
    good &= cpu.step_physical_instruction(bus) == Err(InstructionFault::Execution(fault.clone()));
    good &= *cpu == saved && *bus == device;
    if !good {
        return false;
    }
    good &= cpu.step_instruction(bus) == Err(fault.clone());
    good &= *cpu == saved && *bus == device;
    if !good {
        return false;
    }
    good &= cpu.step_boundary(bus) == Err(fault.clone());
    good &= *cpu == saved && *bus == device;
    if !good {
        return false;
    }
    good &= cpu.prime(bus) == Err(fault.clone());
    good &= *cpu == saved && *bus == device;
    if !good {
        return false;
    }
    good &= cpu.service_pending_trace(bus) == Err(fault);
    good &= *cpu == saved && *bus == device;
    good
}

fn literal_metadata(c: &Case) -> bool {
    let p = c.raw.get("expected").get("private_cpu");
    let pipeline = p.get("pipeline").optional();
    let mut good = c.raw.get("opcode").u16() == c.initial.queue.unwrap()[0]
        && matches!(c.raw.get("size").number(), 8 | 16 | 32)
        && c.raw.get("source_register").number() < 8
        && !p.get("stopped").flag()
        && !p.get("pending_trace").flag()
        && p.get("imported_boundary").optional().is_none()
        && guard_contract(c);
    good &= match pipeline {
        None => c.expected.queue.is_none() && c.queue_address.is_none(),
        Some(node) => {
            node.get("address").u32() == c.queue_address.unwrap()
                && node
                    .get("words")
                    .list()
                    .iter()
                    .map(Value::u16)
                    .collect::<Vec<_>>()
                    == c.expected.queue.unwrap()
        }
    };
    good
}

fn run_bus<B: FixtureBus>(c: Case, mut bus: B, diagnose: bool) -> (bool, bool) {
    let mut cpu = image_cpu(c.initial, c.initial_level, c.initial_edge);
    if let Some(pc) = c.raw.get("injection").get("pc_after_import").optional() {
        cpu.registers.pc = pc.u32();
    }
    let actual = cpu.step_physical_instruction(&mut bus);
    let unsupported = matches!(
        actual,
        Err(InstructionFault::Execution(
            TimingFault::UnsupportedInstruction { .. }
        ))
    );
    let device = bus.device();
    let mut good = actual == c.result
        && cpu.registers == c.expected.registers
        && cpu.queued_words() == c.expected.queue
        && cpu.clock().0 == c.expected.clock
        && device.clock == c.expected.clock
        && device.words == c.final_memory.iter().copied().collect()
        && device.events == c.events
        && device.counts == c.counts
        && device.before_entry == c.before_entry
        && device.reports_consumed()
        && controls::verify(device, &c)
        && cpu.physical_entry_state() == c.state
        && cpu.physical_inspection() == c.inspection
        && !cpu.trace_pending()
        && !cpu.stopped()
        && cpu.interrupt_input().level() == c.expected_level
        && cpu.clock().0.checked_sub(c.initial.clock) == Some(c.elapsed)
        && private_state(&cpu, &c)
        && literal_metadata(&c);
    let frame: Vec<_> = device
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
    good &= frame == c.frame;
    if c.capture_observable {
        good &= matches!(&actual, Ok(PhysicalInstructionStep::Group0 { capture, .. }) if Some((*capture, capture.special_status_word())) == c.capture);
    }
    if matches!(c.result, Ok(PhysicalInstructionStep::Instruction(_))) {
        good &= cpu == image_cpu(c.expected, c.expected_level, c.expected_edge);
    }
    if !good {
        if diagnose {
            #[cfg(not(target_arch = "wasm32"))]
            eprintln!(
                "physical CMP remainder {} actual={actual:?} expected={:?}",
                c.label, c.result
            );
        }
        return (false, unsupported);
    }
    if actual == c.result {
        if let Some(fault) = c.retry.clone() {
            good &= guards(&mut cpu, &mut bus, fault);
        }
        if let Some(fault) = c.next_rejection.clone() {
            let saved = cpu.clone();
            let device = bus.clone();
            let mut copied = cpu.clone();
            let mut copied_bus = bus.clone();
            good &= copied.step_physical_instruction(&mut copied_bus) == Err(fault.clone());
            good &= copied == saved && copied_bus == device;
            good &= cpu.step_physical_instruction(&mut bus) == Err(fault);
            good &= cpu == saved && bus == device;
        }
    }
    if c.raw.get("category").text() == "terminal_reset" {
        good = good && reset::run(&mut cpu, c.raw.get("reset"));
    }
    if !good && diagnose {
        #[cfg(not(target_arch = "wasm32"))]
        eprintln!(
            "physical CMP remainder {} unsupported={unsupported} actual={actual:?} expected={:?} CPU={cpu:?} device={bus:?}",
            c.label, c.result
        );
    }
    let _ = (c.label, diagnose);
    (good, unsupported)
}

pub(super) fn run(c: Case, diagnose: bool) -> (bool, bool) {
    let bus = Device::new(&c);
    if c.default_detector {
        run_bus(c, DefaultBus(bus), diagnose)
    } else {
        run_bus(c, bus, diagnose)
    }
}
