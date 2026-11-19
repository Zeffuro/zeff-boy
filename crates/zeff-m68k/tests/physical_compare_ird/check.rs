use super::*;
use device::Device;

fn cause(row: &Row) -> CriticalFault {
    let access = if row.outcome == 2 {
        row.events.last().unwrap().access
    } else {
        row.events[row.count - 1].access
    };
    if row.outcome == 2 {
        CriticalFault::AddressError {
            access,
            direction: Direction::Write,
        }
    } else {
        CriticalFault::BusAbort {
            access,
            direction: Direction::Read,
        }
    }
}

fn state(row: &Row) -> PhysicalEntryState {
    match row.outcome {
        0 => PhysicalEntryState::Critical(EntryKind::ImportedGroup0),
        1 => PhysicalEntryState::FaultUnqualified { cause: cause(row) },
        2 => PhysicalEntryState::DoubleFaultHalted {
            cause: cause(row),
            timing: TimingQualification::Unavailable,
        },
        _ => panic!("literal outcome"),
    }
}

fn result(row: &Row) -> Result<PhysicalInstructionStep, InstructionFault<&'static str>> {
    match row.outcome {
        0 => Ok(PhysicalInstructionStep::Group0 {
            opcode: row.opcode,
            capture: row.capture,
            clocks: row.clock - row.initial_clock,
        }),
        1 => Err(InstructionFault::CaptureUnavailable {
            phase: if row.final_fetch {
                InstructionPhase::OperandRefill
            } else {
                InstructionPhase::OperandReadHigh
            },
            cause: cause(row),
        }),
        2 => Err(InstructionFault::Entry(EntryFault::TimingUnavailable(
            cause(row),
        ))),
        _ => panic!("literal outcome"),
    }
}

fn private_state(cpu: &PrefetchCpu, row: &Row) -> bool {
    let image = PrefetchCpu::from_prefetched(
        row.expected,
        row.queue.unwrap_or([0, 0]),
        CpuClocks(row.clock),
    )
    .unwrap();
    let mut text = format!("{image:?}");
    if row.queue.is_none() {
        let pipeline = format!(
            "pipeline: Some(Pipeline {{ address: {}, words: [0, 0] }}),",
            row.expected.pc
        );
        if text.matches(&pipeline).count() != 1 {
            return false;
        }
        text = text.replacen(&pipeline, "pipeline: None,", 1);
    }
    for (old, new) in [
        (
            "physical_state: Idle,".to_owned(),
            format!("physical_state: {:?},", state(row)),
        ),
        (
            "physical_inspection: DiagnosticCompleteValues,".to_owned(),
            format!(
                "physical_inspection: {:?},",
                if row.outcome == 0 {
                    PhysicalInspection::DiagnosticCompleteValues
                } else {
                    PhysicalInspection::Unavailable
                }
            ),
        ),
    ] {
        if text.matches(&old).count() != 1 {
            return false;
        }
        text = text.replacen(&old, &new, 1);
    }
    text == format!("{cpu:?}")
}

fn guards(cpu: &mut PrefetchCpu, bus: &mut Device, fault: TimingFault<&'static str>) -> bool {
    let saved = cpu.clone();
    let device = bus.clone();
    let mut copy = cpu.clone();
    let mut copy_bus = bus.clone();
    let mut good = copy.step_physical_instruction(&mut copy_bus)
        == Err(InstructionFault::Execution(fault.clone()));
    good &= copy == saved && copy_bus == device;
    good &= cpu.step_physical_instruction(bus) == Err(InstructionFault::Execution(fault.clone()));
    good &= *cpu == saved && *bus == device;
    good &= cpu.step_instruction(bus) == Err(fault.clone());
    good &= *cpu == saved && *bus == device;
    good &= cpu.step_boundary(bus) == Err(fault.clone());
    good &= *cpu == saved && *bus == device;
    good &= cpu.prime(bus) == Err(fault.clone());
    good &= *cpu == saved && *bus == device;
    good &= cpu.service_pending_trace(bus) == Err(fault);
    good &= *cpu == saved && *bus == device;
    good
}

pub(super) fn run(row: &Row) -> bool {
    let mut cpu =
        PrefetchCpu::from_prefetched(row.initial, row.initial_queue, CpuClocks(row.initial_clock))
            .unwrap();
    let mut bus = Device {
        clock: row.initial_clock,
        words: row.initial_memory.clone(),
        events: Vec::new(),
        count: 0,
        entry_count: 0,
        fault_at: row.count - 1,
    };
    let actual = cpu.step_physical_instruction(&mut bus);
    let good = actual == result(row)
        && cpu.registers == row.expected
        && cpu.queued_words() == row.queue
        && cpu.clock().0 == row.clock
        && bus.clock == row.clock
        && bus.words == row.memory
        && bus.events == row.events
        && bus.count == row.count
        && bus.entry_count == row.entry_count
        && cpu.physical_entry_state() == state(row)
        && private_state(&cpu, row)
        && row.capture.phase_sr == row.phase_registers.status_register()
        && row.capture.special_status_word() == row.ssw
        && row.instruction_clock <= row.clock;
    if !good {
        eprintln!("CMP IRD row {}: {actual:?}", row.id);
        return false;
    }
    if row.outcome == 0 {
        return true;
    }
    guards(
        &mut cpu,
        &mut bus,
        if row.outcome == 1 {
            TimingFault::PhysicalFaultUnqualified
        } else {
            TimingFault::DoubleFaultHalted
        },
    )
}
