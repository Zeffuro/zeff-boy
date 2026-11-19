use super::*;

#[test]
fn all_92_callback_reservations_precede_flags_banks_and_operand_writes() {
    let mut positions = 0;
    for form in forms() {
        for blocked in 0..form.count() {
            positions += 1;
            for supervisor in [false, true] {
                let mut case = Case::new(form);
                case.r = registers(supervisor, 31);
                case.destination = 7;
                let idle = form.indexed() && blocked >= form.prefix();
                case.start =
                    u64::MAX - u64::from(u32::MAX) + 1 - blocked as u64 * 4 - u64::from(idle) * 2;
                let (mut cpu, mut bus, case) = case.seed();
                let plan = oracle::plan(case, &bus.words);
                let memory = oracle::memory(bus.words.clone(), &plan[..blocked]);
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
                    oracle::events(case, &plan[..blocked], &bus.elapsed, idle)
                );
                assert_eq!(bus.count, blocked);
                assert_eq!(bus.entry_count, 0);
                assert_eq!(
                    cpu.clock().0,
                    case.start + oracle::clocks(case, &bus.elapsed, blocked, idle)
                );
                assert_eq!(cpu.clock().0, bus.clock);
                guarded(&mut cpu, &mut bus, TimingFault::Frozen);
            }
        }
    }
    assert_eq!(positions, 92);
}

#[test]
fn indexed_idle_reserves_only_two_after_the_source_prefix_and_keeps_old_registers() {
    for size in [Size::Byte, Size::Word, Size::Long] {
        let form = Form {
            size,
            family: Family::Indexed,
        };
        for remaining in [1, 2] {
            let mut case = Case::new(form);
            case.start =
                u64::MAX - u64::from(u32::MAX) - (form.prefix() - 1) as u64 * 4 - remaining;
            let (mut cpu, mut bus, case) = case.seed();
            let memory = bus.words.clone();
            let plan = oracle::plan(case, &memory);
            bus.elapsed[form.prefix() - 1] = u32::MAX;
            assert_eq!(
                cpu.step_physical_instruction(&mut bus),
                Err(InstructionFault::Entry(EntryFault::ClockOverflow))
            );
            assert_eq!(cpu.registers, case.r);
            assert_eq!(bus.words, memory);
            assert_eq!(
                bus.events,
                oracle::events(case, &plan[..form.prefix()], &bus.elapsed, remaining == 2)
            );
            assert_eq!(bus.count, form.prefix());
            assert_eq!(bus.entry_count, 0);
            assert_eq!(cpu.clock().0, u64::MAX - if remaining == 1 { 1 } else { 0 });
            assert_eq!(cpu.clock().0, bus.clock);
            guarded(&mut cpu, &mut bus, TimingFault::Frozen);
        }
    }
}

#[test]
fn malformed_complete_durations_retain_each_consumed_prefix_and_phase_publication() {
    for form in forms() {
        for at in 0..form.count() {
            for elapsed in [0, 3] {
                let (mut cpu, mut bus, case) = Case::new(form).seed();
                let plan = oracle::plan(case, &bus.words);
                let memory = oracle::memory(bus.words.clone(), &plan[..=at]);
                bus.elapsed[at] = elapsed;
                let idle = at >= form.prefix();
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
                    oracle::events(case, &plan[..=at], &bus.elapsed, idle)
                );
                assert_eq!(bus.count, at + 1);
                assert_eq!(bus.entry_count, 0);
                assert_eq!(
                    cpu.clock().0,
                    case.start + oracle::clocks(case, &bus.elapsed, at + 1, idle)
                );
                assert_eq!(cpu.clock().0, bus.clock);
                guarded(&mut cpu, &mut bus, TimingFault::Frozen);
            }
        }
    }
}

#[test]
fn zero_or_short_boundary_durations_add_only_fifty_entry_clocks() {
    for form in forms() {
        for at in 0..form.count() {
            for elapsed in [0, 1, 3] {
                let (mut cpu, mut bus, case) = Case::new(form).seed();
                let plan = oracle::plan(case, &bus.words);
                let t = plan[at];
                let c = oracle::capture(t, 2);
                let memory = oracle::entry_memory(
                    oracle::memory(bus.words.clone(), &plan[..=at]),
                    t.registers.ssp,
                    c,
                );
                bus.elapsed[at] = elapsed;
                bus.failure = Some((at, Failure::Boundary));
                let idle = at >= form.prefix();
                let clocks = oracle::clocks(case, &bus.elapsed, at + 1, idle);
                assert_eq!(
                    cpu.step_physical_instruction(&mut bus),
                    Ok(PhysicalInstructionStep::Group0 {
                        opcode: form.opcode(case.destination),
                        capture: c,
                        clocks: clocks + 50
                    })
                );
                assert_eq!(cpu.registers, oracle::entered(t.registers));
                assert_eq!(bus.words, memory);
                let mut events = oracle::events(case, &plan[..=at], &bus.elapsed, idle);
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
fn maximum_final_complete_commits_pc_and_retained_queue_without_future_reservation() {
    for form in forms() {
        let last = form.count() - 1;
        let mut case = Case::new(form);
        case.start =
            u64::MAX - u64::from(u32::MAX) - last as u64 * 4 - u64::from(form.indexed()) * 2;
        let (mut cpu, mut bus, case) = case.seed();
        let initial = bus.words.clone();
        let plan = oracle::plan(case, &initial);
        let next = oracle::sample(&initial, case.r.pc + form.length());
        let fetched = oracle::fetched(initial.clone(), &plan);
        let memory = oracle::memory(initial, &plan);
        bus.elapsed[last] = u32::MAX;
        let clocks = oracle::clocks(case, &bus.elapsed, plan.len(), true);
        assert_eq!(
            cpu.step_physical_instruction(&mut bus),
            Ok(PhysicalInstructionStep::Instruction(
                zeff_m68k::timing::TimedStep {
                    opcode: form.opcode(case.destination),
                    clocks,
                    exception: None
                }
            ))
        );
        let mut expected = plan[last].registers;
        expected.pc = case.r.pc + form.length();
        assert_eq!(cpu.registers, expected);
        assert_eq!(bus.words, memory);
        assert_eq!(cpu.queued_words(), Some([next, fetched]));
        assert_eq!(bus.events, oracle::events(case, &plan, &bus.elapsed, true));
        assert_eq!(cpu.clock().0, u64::MAX);
        assert_eq!(cpu.clock().0, bus.clock);
    }
}

#[test]
fn maximum_consumed_prefix_or_fault_retains_effects_before_next_capacity_check() {
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
                let idle = form.indexed() && at >= form.prefix();
                let mut case = Case::new(form);
                case.destination = 7;
                case.start = u64::MAX - u64::from(u32::MAX) - at as u64 * 4 - u64::from(idle) * 2;
                let (mut cpu, mut bus, case) = case.seed();
                let plan = oracle::plan(case, &bus.words);
                let t = plan[at];
                let memory = oracle::memory(bus.words.clone(), &plan[..=at]);
                bus.elapsed[at] = u32::MAX;
                bus.failure = Some((at, failure));
                assert_eq!(
                    cpu.step_physical_instruction(&mut bus),
                    Err(match failure {
                        Failure::Complete | Failure::Boundary =>
                            InstructionFault::Entry(EntryFault::ClockOverflow),
                        Failure::Host => InstructionFault::Entry(EntryFault::HostFailure {
                            access: t.access,
                            direction: oracle::direction(t),
                            source: "host"
                        }),
                        Failure::Unavailable => InstructionFault::TransitionTimingUnavailable {
                            phase: t.phase,
                            cause: CriticalFault::BusAbort {
                                access: t.access,
                                direction: oracle::direction(t)
                            }
                        },
                    })
                );
                assert_eq!(cpu.registers, t.registers);
                assert_eq!(bus.words, memory);
                assert_eq!(
                    bus.events,
                    oracle::events(case, &plan[..=at], &bus.elapsed, idle)
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
fn trace_and_accepted_irq_preflight_can_resume_after_input_is_cleared() {
    for form in forms() {
        let (mut edge_cpu, mut edge_bus, _) = Case::new(form).seed();
        edge_cpu.interrupt_input().set_level(7);
        edge_cpu.interrupt_input().set_level(0);
        let pending = edge_cpu.clone();
        let memory = edge_bus.words.clone();
        assert_eq!(
            edge_cpu.step_physical_instruction(&mut edge_bus),
            Err(InstructionFault::InterruptUnqualified)
        );
        assert_eq!(edge_cpu, pending);
        assert_eq!(edge_bus.words, memory);
        assert!(edge_bus.events.is_empty());
        assert_eq!(edge_bus.clock, 37);
        for trace in [false, true] {
            let (mut cpu, mut bus, case) = Case::new(form).seed();
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
            assert_eq!(bus.clock, 37);
            if trace {
                cpu.registers.set_status_register(case.r.status_register());
            } else {
                cpu.interrupt_input().set_level(0);
            }
            let plan = oracle::plan(case, &memory);
            let clocks = oracle::clocks(case, &bus.elapsed, plan.len(), true);
            assert_eq!(
                cpu.step_physical_instruction(&mut bus),
                Ok(PhysicalInstructionStep::Instruction(
                    zeff_m68k::timing::TimedStep {
                        opcode: form.opcode(case.destination),
                        clocks,
                        exception: None
                    }
                ))
            );
            let mut expected = plan.last().unwrap().registers;
            expected.pc = case.r.pc + form.length();
            assert_eq!(cpu.registers, expected);
            assert_eq!(bus.words, oracle::memory(memory.clone(), &plan));
            assert_eq!(
                cpu.queued_words(),
                Some([
                    oracle::sample(&memory, case.r.pc + form.length()),
                    oracle::fetched(memory, &plan)
                ])
            );
            assert_eq!(bus.events, oracle::events(case, &plan, &bus.elapsed, true));
            assert_eq!(cpu.clock().0, bus.clock);
        }
    }
}

#[test]
fn byte_movea_reserved_ea_and_other_families_remain_unsupported_before_prefix() {
    for opcode in [0x107c, 0x35fc, 0x37fc, 0x3ffc, 0x0640, 0x0440, 0x113d] {
        let r = registers(false, 31);
        let mut cpu = PrefetchCpu::from_prefetched(r, [opcode, 0x8aa6], CpuClocks(37)).unwrap();
        let mut bus = BusDevice::with_clock(37);
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
