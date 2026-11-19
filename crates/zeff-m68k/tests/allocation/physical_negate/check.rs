use super::*;

pub(super) fn run(
    memory: &mut MemoryBus,
    form: (u16, bool, usize, usize),
    sample: (bool, u16, u32),
    path: Path,
) {
    let (family, long, source, register) = form;
    let (supervisor, ccr, value) = sample;
    let odd = matches!(path, Path::Odd);
    let ea = 0x3000 + u32::from(odd);
    let stride = if long { 4 } else { 2 };
    let ext = extensions(source);
    let anchor = 0x2002 + 2 * ext as u32;
    let opcode = family
        | if long { 0x80 } else { 0x40 }
        | FIELDS[source]
        | if source < 5 { register as u16 } else { 0 };
    let mut r = Registers::new();
    r.pc = 0x2000;
    r.a.fill(0x4000);
    r.d.fill(0xaabb_ccdd);
    r.d[7] = 0x1000;
    r.usp = 0x6000;
    r.ssp = 0x5000;
    r.set_status_register(0x0500 | ccr | if supervisor { 0x2000 } else { 0 });
    if source < 5 {
        r.set_address_register(
            register,
            match source {
                2 => ea + stride,
                4 => 0x2000,
                _ => ea,
            },
        );
    }
    let extension = match source {
        0..=2 => 0x4e71,
        3 | 6 => 0,
        4 => 0x7800 | u16::from(odd),
        5 => ea as u16,
        _ => unreachable!(),
    };
    memory.word(0x2000, opcode);
    memory.word(0x2002, extension);
    memory.word(0x2004, if source == 6 { ea as u16 } else { 0x4e71 });
    memory.word(anchor as usize, 0x4e71);
    memory.word((anchor + 2) as usize, 0x7201);
    memory.word(
        (ea & !1) as usize,
        if long {
            (value >> 16) as u16
        } else {
            value as u16
        },
    );
    memory.word(
        ((ea + 2) & !1) as usize,
        if long { value as u16 } else { 0x5a5a },
    );
    let expected_operand = if long { value } else { value & 0xffff };
    let high = memory.read(access(ea & !1, supervisor, true)).unwrap();
    let low = memory
        .read(access((ea + 2) & !1, supervisor, true))
        .unwrap();
    assert_eq!(
        if long {
            u32::from(high) << 16 | u32::from(low)
        } else {
            u32::from(high)
        },
        expected_operand
    );
    let (result, partial_sr, full_sr) = stages(r.status_register(), family, value, long);
    let mut actions = [Action::Reserve; 20];
    let mut length = 0;
    let mut push = |a| {
        actions[length] = a;
        length += 1;
    };
    if matches!(source, 2 | 4) {
        push(Action::Idle);
    }
    for at in 0..ext {
        push(Action::Read(access(
            0x2004 + 2 * at as u32,
            supervisor,
            false,
        )));
    }
    if source == 2 || source == 1 && !long {
        push(Action::Reserve);
        push(Action::Address(if source == 2 { ea } else { ea + stride }));
    }
    push(Action::Read(access(ea, supervisor, true)));
    if long {
        if source == 1 {
            push(Action::Reserve);
            push(Action::Address(ea + stride));
        }
        push(Action::Read(access(ea + 2, supervisor, true)));
    }
    push(Action::Reserve);
    push(Action::Flags(partial_sr));
    push(Action::Read(access(anchor + 2, supervisor, false)));
    if long {
        push(Action::Reserve);
        push(Action::Flags(full_sr));
        push(Action::Write(
            access(ea + 2, supervisor, true),
            result as u16,
        ));
    }
    push(Action::Write(
        access(ea, supervisor, true),
        if long {
            (result >> 16) as u16
        } else {
            result as u16
        },
    ));
    push(Action::Commit);
    let mut start = 17;
    let mut maximum = None;
    if let Path::Blocked(target) = path {
        if target == 0 {
            start = u64::MAX - BUDGET + 1;
        } else {
            maximum = Some(target - 1);
            let mut prefix = 0;
            let mut index = 0;
            for a in &actions[..length] {
                match a {
                    Action::Idle => prefix += 2,
                    Action::Read(_) | Action::Write(_, _) => {
                        if index == target {
                            break;
                        }
                        prefix += if index == target - 1 { BUDGET } else { 7 };
                        index += 1;
                    }
                    _ => {}
                }
            }
            start = u64::MAX - prefix;
        }
    }
    let host = if let Path::Host(at) = path {
        Some(at)
    } else {
        None
    };
    let queue = [opcode, extension];
    let mut expected_memory = memory.0;
    let mut expected_events = [None; 8];
    let mut event_count = 0;
    let mut transfers = 0;
    let mut clock = start;
    let mut expected = r;
    let mut expected_queue = Some(queue);
    let mut fault = None;
    for action in &actions[..length] {
        match *action {
            Action::Reserve if clock.checked_add(BUDGET).is_none() => {
                fault = Some(InstructionFault::Entry(EntryFault::ClockOverflow));
                break;
            }
            Action::Reserve => {}
            Action::Idle => {
                expected_events[event_count] = Some(Event::Idle(clock, 2));
                event_count += 1;
                clock += 2;
            }
            Action::Address(address) => {
                expected.set_address_register(register, address);
            }
            Action::Flags(sr) => {
                expected.set_status_register(sr);
            }
            Action::Commit => {
                expected.pc = anchor;
                expected_queue = Some([0x4e71, 0x7201]);
            }
            Action::Read(a) | Action::Write(a, _) => {
                if clock.checked_add(BUDGET).is_none() {
                    fault = Some(InstructionFault::Entry(EntryFault::ClockOverflow));
                    break;
                }
                if a.logical_address & 1 != 0 {
                    expected_queue = None;
                    fault = Some(InstructionFault::TransitionTimingUnavailable {
                        phase: InstructionPhase::OperandReadHigh,
                        cause: CriticalFault::AddressError {
                            access: a,
                            direction: Direction::Read,
                        },
                    });
                    break;
                }
                let offset = a.bus_address as usize;
                let elapsed = if maximum == Some(transfers) {
                    u32::MAX
                } else {
                    7
                };
                let direction = if let Action::Write(_, value) = *action {
                    expected_memory[offset..offset + 2].copy_from_slice(&value.to_be_bytes());
                    expected_events[event_count] = Some(Event::Write(a, value, clock, elapsed));
                    Direction::Write
                } else {
                    let value =
                        u16::from_be_bytes([expected_memory[offset], expected_memory[offset + 1]]);
                    expected_events[event_count] = Some(Event::Read(a, value, clock, elapsed));
                    Direction::Read
                };
                event_count += 1;
                clock += u64::from(elapsed);
                let failed = host == Some(transfers);
                transfers += 1;
                if failed {
                    expected_queue = None;
                    fault = Some(InstructionFault::Entry(EntryFault::HostFailure {
                        access: a,
                        direction,
                        source: (),
                    }));
                    break;
                }
            }
        }
    }
    let mut cpu = PrefetchCpu::from_prefetched(r, queue, CpuClocks(start)).unwrap();
    let mut bus = Device {
        memory,
        events: [None; 8],
        event_count: 0,
        transfers: 0,
        clock: start,
        host,
        maximum,
    };
    let actual = black_box(cpu.step_physical_instruction(&mut bus));
    if let Some(fault) = &fault {
        assert_eq!(actual, Err(fault.clone()));
    } else {
        assert!(
            matches!(actual, Ok(PhysicalInstructionStep::Instruction(step))
            if step.opcode == opcode && step.clocks == clock - start && step.exception.is_none())
        );
    }
    assert_eq!(cpu.registers, expected);
    assert_eq!(cpu.queued_words(), expected_queue);
    assert_eq!(cpu.clock().0, clock);
    assert_eq!(bus.clock, clock);
    assert_eq!(bus.transfers, transfers);
    assert_eq!(bus.event_count, event_count);
    assert_eq!(bus.events, expected_events);
    assert_eq!(bus.memory.0, expected_memory);
    assert!(!cpu.trace_pending());
    assert!(!cpu.stopped());
    if let Some(InstructionFault::TransitionTimingUnavailable { cause, .. }) = &fault {
        assert_eq!(
            cpu.physical_entry_state(),
            PhysicalEntryState::FaultUnqualified { cause: *cause }
        );
        assert_eq!(cpu.physical_inspection(), PhysicalInspection::Unavailable);
    } else {
        assert_eq!(cpu.physical_entry_state(), PhysicalEntryState::Idle);
        assert_eq!(
            cpu.physical_inspection(),
            PhysicalInspection::DiagnosticCompleteValues
        );
    }
    assert_eq!(*cpu.interrupt_input(), Default::default());
    if fault.is_none() {
        let expected_cpu =
            PrefetchCpu::from_prefetched(expected, expected_queue.unwrap(), CpuClocks(clock))
                .unwrap();
        assert_eq!(cpu, expected_cpu);
    } else {
        let saved = cpu.clone();
        let device = bus.snapshot();
        let guard = if matches!(path, Path::Odd) {
            TimingFault::PhysicalFaultUnqualified
        } else {
            TimingFault::Frozen
        };
        let mut copied = saved.clone();
        assert_eq!(
            copied.step_physical_instruction(&mut bus),
            Err(InstructionFault::Execution(guard.clone()))
        );
        assert_eq!(copied, saved);
        assert!(device.matches(&bus));
        assert_eq!(
            cpu.step_physical_instruction(&mut bus),
            Err(InstructionFault::Execution(guard.clone()))
        );
        assert_eq!(cpu, saved);
        assert!(device.matches(&bus));
        assert_eq!(cpu.step_instruction(&mut bus), Err(guard.clone()));
        assert_eq!(cpu, saved);
        assert!(device.matches(&bus));
        assert_eq!(cpu.step_boundary(&mut bus), Err(guard.clone()));
        assert_eq!(cpu, saved);
        assert!(device.matches(&bus));
        assert_eq!(cpu.prime(&mut bus), Err(guard.clone()));
        assert_eq!(cpu, saved);
        assert!(device.matches(&bus));
        assert_eq!(cpu.service_pending_trace(&mut bus), Err(guard));
        assert_eq!(cpu, saved);
        assert!(device.matches(&bus));
    }
}
