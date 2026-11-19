use super::*;

#[test]
fn source_idle_owns_only_two_clocks_and_does_not_publish_predecrement() {
    for form in forms().into_iter().filter(|f| f.idle()) {
        for remaining in [1, 2] {
            let mut case = Case::new(form);
            case.register = 7;
            case.start = u64::MAX - remaining;
            let (mut cpu, mut bus) = case.seed();
            let memory = bus.words.clone();
            assert_eq!(
                cpu.step_physical_instruction(&mut bus),
                Err(InstructionFault::Entry(EntryFault::ClockOverflow))
            );
            assert_eq!(cpu.registers, case.r);
            assert_eq!(bus.words, memory);
            assert_eq!(
                bus.events,
                if remaining == 2 {
                    vec![Event::Idle(case.start, 2)]
                } else {
                    vec![]
                }
            );
            assert_eq!(bus.count, 0);
            assert_eq!(
                cpu.clock().0,
                case.start + if remaining == 2 { 2 } else { 0 }
            );
            assert_eq!(cpu.clock().0, bus.clock);
            guarded(&mut cpu, &mut bus, TimingFault::Frozen);
        }
    }
}

#[test]
fn all_118_reservations_precede_partial_an_and_an_publication_in_both_banks() {
    let mut positions = 0;
    for form in forms() {
        for blocked in 0..form.count() {
            positions += 1;
            for supervisor in [false, true] {
                let mut case = Case::new(form);
                case.r = registers(supervisor, 31);
                case.register = 7;
                case.start = u64::MAX - u64::from(u32::MAX) + 1
                    - blocked as u64 * 4
                    - u64::from(form.idle()) * 2;
                let (mut cpu, mut bus) = case.seed();
                let memory = bus.words.clone();
                let plan = oracle::plan(case, &memory);
                assert_eq!(
                    cpu.step_physical_instruction(&mut bus),
                    Err(InstructionFault::Entry(EntryFault::ClockOverflow))
                );
                assert_eq!(
                    cpu.registers,
                    if blocked == 0 {
                        case.r
                    } else {
                        plan[blocked - 1].registers
                    }
                );
                assert_eq!(bus.words, memory);
                assert_eq!(
                    bus.events,
                    oracle::events(case, &plan[..blocked], &bus.elapsed, true)
                );
                assert_eq!(bus.count, blocked);
                assert_eq!(bus.entry_count, 0);
                assert_eq!(
                    cpu.clock().0,
                    case.start + oracle::clocks(case, &bus.elapsed, blocked, true)
                );
                assert_eq!(cpu.clock().0, bus.clock);
                guarded(&mut cpu, &mut bus, TimingFault::Frozen);
            }
        }
    }
    assert_eq!(positions, 118);
}

#[test]
fn short_complete_reports_consume_callback_without_discarding_phase_state() {
    for form in forms() {
        for at in 0..form.count() {
            for elapsed in [0, 3] {
                let mut case = Case::new(form);
                case.register = 7;
                let (mut cpu, mut bus) = case.seed();
                let memory = bus.words.clone();
                let plan = oracle::plan(case, &memory);
                bus.elapsed[at] = elapsed;
                assert_eq!(
                    cpu.step_physical_instruction(&mut bus),
                    Err(InstructionFault::Entry(
                        EntryFault::InvalidCompleteDuration { elapsed }
                    ))
                );
                assert_eq!(cpu.registers, plan[at].registers);
                assert_eq!(bus.words, memory);
                assert_eq!(
                    bus.events,
                    oracle::events(case, &plan[..=at], &bus.elapsed, true)
                );
                assert_eq!(bus.count, at + 1);
                assert_eq!(bus.entry_count, 0);
                assert_eq!(
                    cpu.clock().0,
                    case.start + oracle::clocks(case, &bus.elapsed, at + 1, true)
                );
                assert_eq!(cpu.clock().0, bus.clock);
                guarded(&mut cpu, &mut bus, TimingFault::Frozen);
            }
        }
    }
}

#[test]
fn zero_and_short_berr_add_only_the_selected_elapsed_and_fifty_entry_clocks() {
    for form in forms() {
        for at in 0..form.count() {
            for elapsed in [0, 1, 3] {
                let case = Case::new(form);
                let (mut cpu, mut bus) = case.seed();
                let initial = bus.words.clone();
                let plan = oracle::plan(case, &initial);
                let t = plan[at];
                let c = oracle::capture(t, 2);
                bus.elapsed[at] = elapsed;
                bus.failure = Some((at, Failure::Boundary));
                let clocks = oracle::clocks(case, &bus.elapsed, at + 1, true);
                assert_eq!(
                    cpu.step_physical_instruction(&mut bus),
                    Ok(PhysicalInstructionStep::Group0 {
                        opcode: case.opcode(),
                        capture: c,
                        clocks: clocks + 50
                    })
                );
                assert_eq!(cpu.registers, oracle::entered(t.registers));
                assert_eq!(bus.words, oracle::entry_memory(initial, t.registers.ssp, c));
                let mut events = oracle::events(case, &plan[..=at], &bus.elapsed, true);
                events.extend(oracle::entry_events(
                    t.registers.ssp,
                    c,
                    case.start + clocks,
                ));
                assert_eq!(bus.events, events);
                oracle::frame(&bus, t.registers.ssp, c, case.start + clocks);
                assert_eq!(cpu.clock().0, case.start + clocks + 50);
                assert_eq!(cpu.clock().0, bus.clock);
            }
        }
    }
}

#[test]
fn maximum_consumed_prefix_and_each_failure_preserve_phase_before_next_capacity_check() {
    for form in forms() {
        for at in 0..form.count() {
            for failure in [
                Failure::Complete,
                Failure::Host,
                Failure::Boundary,
                Failure::Unavailable,
            ] {
                if failure == Failure::Complete && at + 1 == form.count() {
                    continue;
                }
                let mut case = Case::new(form);
                case.register = 7;
                case.start =
                    u64::MAX - u64::from(u32::MAX) - at as u64 * 4 - u64::from(form.idle()) * 2;
                let (mut cpu, mut bus) = case.seed();
                let memory = bus.words.clone();
                let plan = oracle::plan(case, &memory);
                let t = plan[at];
                bus.elapsed[at] = u32::MAX;
                bus.failure = Some((at, failure));
                assert_eq!(
                    cpu.step_physical_instruction(&mut bus),
                    Err(match failure {
                        Failure::Complete | Failure::Boundary =>
                            InstructionFault::Entry(EntryFault::ClockOverflow),
                        Failure::Host => InstructionFault::Entry(EntryFault::HostFailure {
                            access: t.access,
                            direction: Direction::Read,
                            source: "host"
                        }),
                        Failure::Unavailable => InstructionFault::TransitionTimingUnavailable {
                            phase: t.phase,
                            cause: CriticalFault::BusAbort {
                                access: t.access,
                                direction: Direction::Read
                            }
                        },
                    })
                );
                assert_eq!(cpu.registers, t.registers);
                assert_eq!(bus.words, memory);
                assert_eq!(
                    bus.events,
                    oracle::events(case, &plan[..=at], &bus.elapsed, true)
                );
                assert_eq!(bus.count, at + 1);
                assert_eq!(bus.entry_count, 0);
                assert_eq!(cpu.clock().0, u64::MAX);
                assert_eq!(cpu.clock().0, bus.clock);
                guarded(
                    &mut cpu,
                    &mut bus,
                    if failure == Failure::Unavailable {
                        TimingFault::PhysicalFaultUnqualified
                    } else {
                        TimingFault::Frozen
                    },
                );
            }
        }
    }
}

#[test]
fn trace_irq_and_latched_level7_preflight_preserve_whole_state_before_any_prefix() {
    for form in forms() {
        for trace in [false, true] {
            let case = Case::new(form);
            let (mut cpu, mut bus) = case.seed();
            if trace {
                cpu.registers
                    .set_status_register(cpu.registers.status_register() | TRACE);
            } else {
                cpu.interrupt_input().set_level(6);
            }
            let before = cpu.clone();
            let memory = bus.words.clone();
            assert_eq!(
                cpu.step_physical_instruction(&mut bus),
                Err(if trace {
                    InstructionFault::TraceUnqualified
                } else {
                    InstructionFault::InterruptUnqualified
                })
            );
            assert_eq!(cpu, before);
            assert_eq!(bus.words, memory);
            assert!(bus.events.is_empty());
            assert_eq!(bus.clock, case.start);
            if trace {
                cpu.registers.set_status_register(case.r.status_register());
            } else {
                cpu.interrupt_input().set_level(0);
            }
            let plan = oracle::plan(case, &memory);
            assert!(cpu.step_physical_instruction(&mut bus).is_ok());
            assert_eq!(
                cpu.registers.status_register(),
                plan.last().unwrap().registers.status_register()
            );
            let mut events = oracle::events(case, &plan, &bus.elapsed, true);
            if form.size == Size::Word {
                events.push(Event::Idle(bus.clock - 4, 2));
            }
            events.push(Event::Idle(bus.clock - 2, 2));
            assert_eq!(bus.events, events);
        }
        let case = Case::new(form);
        let (mut cpu, mut bus) = case.seed();
        cpu.interrupt_input().set_level(7);
        cpu.interrupt_input().set_level(0);
        let before = cpu.clone();
        assert_eq!(
            cpu.step_physical_instruction(&mut bus),
            Err(InstructionFault::InterruptUnqualified)
        );
        assert_eq!(cpu, before);
        assert!(bus.events.is_empty());
        assert_eq!(bus.clock, case.start);
    }
}

#[test]
fn dn_an_immediate_reserved_address_arithmetic_and_other_families_remain_unsupported_before_prefix()
{
    let byte_memory = Form {
        operation: Operation::Add,
        size: Size::Byte,
        ea: Ea::Indirect,
    }
    .opcode(0);
    let opcodes = [
        byte_memory,
        0xd0c0,
        0xd1c0,
        0xd0c8,
        0xd1c8,
        0xd0fc,
        0xd1fc,
        0xd0fd,
        0xd1fd,
        0x90c0,
        0x91c0,
        0x90c8,
        0x91c8,
        0x90fc,
        0x91fc,
        0x90fd,
        0x91fd,
        0xd110,
        0x9110,
        0xb108,
        0x0640,
    ];
    for opcode in opcodes {
        let r = registers(false, 31);
        let mut cpu = PrefetchCpu::from_prefetched(r, [opcode, 0x7201], CpuClocks(37)).unwrap();
        let mut bus = Device::new(37);
        let before = cpu.clone();
        let memory = bus.words.clone();
        assert_eq!(
            cpu.step_physical_instruction(&mut bus),
            Err(InstructionFault::Execution(
                TimingFault::UnsupportedInstruction { pc: r.pc, opcode }
            ))
        );
        assert_eq!(cpu, before);
        assert_eq!(bus.words, memory);
        assert!(bus.events.is_empty());
        assert_eq!(bus.clock, 37);
    }
}

#[test]
fn unprimed_queue_and_pc_mismatch_preserve_state_before_any_source_prefix() {
    for form in forms() {
        for supervisor in [false, true] {
            let mut case = Case::new(form);
            case.r = registers(supervisor, 31);
            let (_, mut bus) = case.seed();
            let mut cpu = PrefetchCpu::default();
            cpu.registers = case.r;
            let before = cpu.clone();
            let memory = bus.words.clone();
            assert_eq!(
                cpu.step_physical_instruction(&mut bus),
                Err(InstructionFault::Execution(TimingFault::Unprimed))
            );
            assert_eq!(cpu, before);
            assert_eq!(bus.words, memory);
            assert!(bus.events.is_empty());
            assert_eq!(bus.clock, case.start);
            let (mut cpu, mut bus) = case.seed();
            cpu.registers.pc = cpu.registers.pc.wrapping_add(2);
            let before = cpu.clone();
            let memory = bus.words.clone();
            assert_eq!(
                cpu.step_physical_instruction(&mut bus),
                Err(InstructionFault::Execution(TimingFault::Unprimed))
            );
            assert_eq!(cpu, before);
            assert_eq!(bus.words, memory);
            assert!(bus.events.is_empty());
            assert_eq!(bus.clock, case.start);
        }
    }
}
