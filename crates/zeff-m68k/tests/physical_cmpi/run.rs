use super::*;

pub(super) fn frozen(
    cpu: &mut PrefetchCpu,
    bus: &mut BusDevice,
    fault: TimingFault<&'static str>,
) -> bool {
    let before = cpu.clone();
    let device = bus.clone();
    let words = bus.words.clone();
    let events = bus.events.clone();
    let clock = bus.clock;
    let counts = (bus.count, bus.entry_count);
    let mut good =
        cpu.step_physical_instruction(bus) == Err(InstructionFault::Execution(fault.clone()));
    good &= cpu == &before && bus == &device;
    if !good {
        return false;
    }
    good &= cpu.step_instruction(bus) == Err(fault.clone());
    good &= cpu == &before && bus == &device;
    good &= cpu.step_boundary(bus) == Err(fault.clone());
    good &= cpu == &before && bus == &device;
    good &= cpu.prime(bus) == Err(fault.clone());
    good &= cpu == &before && bus == &device;
    good &= cpu.service_pending_trace(bus) == Err(fault.clone());
    good &= cpu == &before && bus == &device;
    good &= bus == &device
        && cpu == &before
        && bus.words == words
        && bus.events == events
        && bus.clock == clock
        && (bus.count, bus.entry_count) == counts;
    let mut copied = before.clone();
    let mut copied_bus = device.clone();
    good &= copied.step_physical_instruction(&mut copied_bus)
        == Err(InstructionFault::Execution(fault))
        && copied == before
        && copied_bus == device;
    good
}

pub(super) fn check(case: Case, failure: Option<(usize, Failure)>, wait: u32) -> bool {
    let (cpu, mut bus) = case.seed();
    bus.elapsed = [4 + wait; 7];
    if let Some((at, Failure::Complete)) = failure {
        bus.elapsed[at] = 3;
    }
    bus.failure = failure;
    seeded(case, cpu, bus)
}

pub(super) fn seeded(case: Case, cpu: PrefetchCpu, bus: BusDevice) -> bool {
    seeded_custom(case, cpu, bus, None)
}
pub(super) fn seeded_custom(
    case: Case,
    mut cpu: PrefetchCpu,
    mut bus: BusDevice,
    secondary: Option<(Failure, u32)>,
) -> bool {
    let mut memory = bus.words.clone();
    let original_memory = memory.clone();
    if let Some((address, value)) = bus.mutate_read {
        memory.insert(address & 0xfffffe, value);
    }
    let plan = oracle::plan(case, &memory);
    let source_address = oracle::address(case, &memory);
    if source_address & 1 != 0 && case.form.size != Size::Byte {
        return false;
    }
    let failure = bus.failure;
    let count = failure.map_or(plan.len(), |(at, _)| at + 1);
    let tail = u64::from(failure.is_none() && case.form.tail()) * 2;
    let elapsed = oracle::clocks(case, &bus.elapsed, count, true) + tail;
    let mut events = oracle::events(case, &plan[..count], &bus.elapsed, true);
    if tail != 0 {
        events.push(Event::Idle(case.start + elapsed - tail, 2));
    }
    let mut expected_clock = case.start + elapsed;
    let mut expected_memory = original_memory.clone();
    if let Some((address, value)) = bus.mutate_read
        && plan[..count]
            .iter()
            .any(|t| t.access.logical_address == address)
    {
        expected_memory.insert(address & 0xfffffe, value);
    }
    let result = cpu.step_physical_instruction(&mut entry::Probe {
        bus: &mut bus,
        primary_default: false,
        secondary,
    });
    let mut good = bus.count == count && cpu.clock().0 == bus.clock;
    match failure {
        None => {
            let r = oracle::final_registers(case, &memory, &plan);
            good &= result
                == Ok(PhysicalInstructionStep::Instruction(
                    zeff_m68k::timing::TimedStep {
                        opcode: case.opcode(),
                        clocks: elapsed,
                        exception: None,
                    },
                ));
            good &= cpu.registers == r
                && cpu.queued_words()
                    == Some([
                        oracle::next(case, &memory),
                        oracle::sample(&memory, r.pc.wrapping_add(2)),
                    ]);
            good &= bus.entry_count == 0 && cpu.physical_entry_state() == PhysicalEntryState::Idle;
        }
        Some((at, kind)) => {
            let t = plan[at];
            let cause = CriticalFault::BusAbort {
                access: t.access,
                direction: Direction::Read,
            };
            if kind == Failure::Boundary && oracle::known(t.ird) {
                let p = entry::predict(
                    case,
                    t,
                    2,
                    expected_clock,
                    expected_memory,
                    (bus.entry_failure, secondary),
                );
                expected_clock = p.end;
                expected_memory = p.words;
                events.extend(p.events);
                good &= result == p.result
                    && cpu.registers == p.registers
                    && cpu.queued_words() == p.queue
                    && bus.entry_count == p.count
                    && cpu.physical_entry_state() == p.state
                    && cpu.physical_inspection() == p.inspection;
                if p.queue.is_none() {
                    good &= frozen(
                        &mut cpu,
                        &mut bus,
                        if matches!(p.state, PhysicalEntryState::DoubleFaultHalted { .. }) {
                            TimingFault::DoubleFaultHalted
                        } else {
                            TimingFault::Frozen
                        },
                    );
                }
            } else {
                let fault = match kind {
                    Failure::Host => InstructionFault::Entry(EntryFault::HostFailure {
                        access: t.access,
                        direction: Direction::Read,
                        source: "host",
                    }),
                    Failure::Complete => {
                        InstructionFault::Entry(EntryFault::InvalidCompleteDuration {
                            elapsed: bus.elapsed[at],
                        })
                    }
                    Failure::Boundary => InstructionFault::CaptureUnavailable {
                        phase: t.phase,
                        cause,
                    },
                    Failure::Unavailable => InstructionFault::TransitionTimingUnavailable {
                        phase: t.phase,
                        cause,
                    },
                };
                good &= result == Err(fault)
                    && cpu.registers == t.registers
                    && cpu.queued_words()
                        == if kind == Failure::Complete {
                            Some([case.opcode(), case.retained()])
                        } else {
                            None
                        }
                    && bus.entry_count == 0;
                if matches!(kind, Failure::Boundary | Failure::Unavailable) {
                    good &= cpu.physical_entry_state()
                        == PhysicalEntryState::FaultUnqualified { cause }
                        && cpu.physical_inspection() == PhysicalInspection::Unavailable;
                }
                good &= frozen(
                    &mut cpu,
                    &mut bus,
                    if matches!(kind, Failure::Boundary | Failure::Unavailable) {
                        TimingFault::PhysicalFaultUnqualified
                    } else {
                        TimingFault::Frozen
                    },
                );
            }
        }
    }
    good && bus.clock == expected_clock
        && cpu.clock().0 == expected_clock
        && bus.events == events
        && bus.words == expected_memory
}
