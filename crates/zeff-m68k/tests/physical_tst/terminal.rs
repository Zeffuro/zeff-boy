use super::*;

#[path = "terminal_positive.rs"]
mod positive;

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
fn all_45_reservations_precede_flags_and_an_publication_in_both_banks() {
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
    assert_eq!(positions, 45);
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
                        opcode: form.opcode(case.register),
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
fn maximum_final_completion_commits_pc_and_retained_queue_without_future_reserve() {
    for form in forms() {
        let last = form.count() - 1;
        let mut case = Case::new(form);
        case.start = u64::MAX - u64::from(u32::MAX) - last as u64 * 4 - u64::from(form.idle()) * 2;
        let (mut cpu, mut bus) = case.seed();
        let initial = bus.words.clone();
        let plan = oracle::plan(case, &initial);
        bus.elapsed[last] = u32::MAX;
        let clocks = oracle::clocks(case, &bus.elapsed, plan.len(), true);
        assert_eq!(
            cpu.step_physical_instruction(&mut bus),
            Ok(PhysicalInstructionStep::Instruction(
                zeff_m68k::timing::TimedStep {
                    opcode: form.opcode(case.register),
                    clocks,
                    exception: None
                }
            ))
        );
        let mut r = plan[last].registers;
        r.pc = case.r.pc.wrapping_add(form.length());
        assert_eq!(cpu.registers, r);
        assert_eq!(
            cpu.queued_words(),
            Some([
                oracle::next(case, &initial),
                oracle::sample(&initial, r.pc.wrapping_add(2))
            ])
        );
        assert_eq!(bus.words, initial);
        assert_eq!(bus.events, oracle::events(case, &plan, &bus.elapsed, true));
        assert_eq!(cpu.clock().0, u64::MAX);
        assert_eq!(cpu.clock().0, bus.clock);
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
            assert_eq!(bus.events, oracle::events(case, &plan, &bus.elapsed, true));
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
fn original_tst_probe_inputs_preserve_completed_and_unsupported_contracts() {
    let mut opcodes = vec![
        0x4a00, 0x4a40, 0x4a80, 0x4a48, 0x4a7a, 0x4a7b, 0x4a7c, 0x4a7d, 0x4afc, 0x4ad0, 0x0640,
    ];
    opcodes.extend(EAS.into_iter().map(|ea| {
        Form {
            size: Size::Byte,
            ea,
        }
        .opcode(7)
    }));
    for opcode in opcodes {
        let r = registers(false, 31);
        let mut cpu = PrefetchCpu::from_prefetched(r, [opcode, 0x7201], CpuClocks(37)).unwrap();
        let mut bus = Device::new(37);
        if matches!(
            opcode,
            0x4a00 | 0x4a40 | 0x4a80 | 0x4a17 | 0x4a1f | 0x4a27 | 0x4a2f | 0x4a37 | 0x4a38 | 0x4a39
        ) {
            positive::complete(&mut cpu, &mut bus, opcode);
            continue;
        }
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
