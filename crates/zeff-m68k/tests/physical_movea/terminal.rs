use super::*;

#[test]
fn every_68_reservation_precedes_source_update_or_destination_publication() {
    let mut positions = 0;
    for form in forms() {
        for blocked in 0..form.count() {
            positions += 1;
            for supervisor in [false, true] {
                let mut case = Case::new(form);
                case.r = registers(supervisor, 31);
                case.source = 7;
                case.destination = 7;
                case.start = u64::MAX - u64::from(u32::MAX) + 1 - form.idle() - blocked as u64 * 4;
                let (mut cpu, mut bus, case) = case.seed();
                let plan = oracle::plan(case, &bus.words);
                let memory = bus.words.clone();
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
                    oracle::events(case, &plan[..blocked], &bus.elapsed)
                );
                assert_eq!(bus.count, blocked);
                assert_eq!(bus.entry_count, 0);
                assert_eq!(
                    cpu.clock().0,
                    case.start + oracle::clocks(case, &bus.elapsed, blocked)
                );
                assert_eq!(cpu.clock().0, bus.clock);
                guarded(&mut cpu, &mut bus, TimingFault::Frozen);
            }
        }
    }
    assert_eq!(positions, 68);
}

#[test]
fn predecrement_and_index_i2_have_their_own_capacity_before_source_publication() {
    for form in forms().into_iter().filter(|f| f.idle() != 0) {
        for remaining in [1, 2] {
            let mut case = Case::new(form);
            case.start = u64::MAX - remaining;
            case.source = 7;
            let (mut cpu, mut bus, case) = case.seed();
            let memory = bus.words.clone();
            assert_eq!(
                cpu.step_physical_instruction(&mut bus),
                Err(InstructionFault::Entry(EntryFault::ClockOverflow))
            );
            assert_eq!(cpu.registers, case.r);
            assert_eq!(bus.words, memory);
            assert_eq!(
                bus.events,
                if remaining == 1 {
                    vec![]
                } else {
                    vec![Event::Idle(case.start, 2)]
                }
            );
            assert_eq!(
                cpu.clock().0,
                case.start + if remaining == 1 { 0 } else { 2 }
            );
            assert_eq!(cpu.clock().0, bus.clock);
            assert_eq!(bus.count, 0);
            guarded(&mut cpu, &mut bus, TimingFault::Frozen);
        }
    }
}

#[test]
fn all_malformed_zero_or_short_completions_retain_phase_registers_and_freeze() {
    for form in forms() {
        for at in 0..form.count() {
            for elapsed in [0, 3] {
                let (mut cpu, mut bus, case) = Case::new(form).seed();
                let plan = oracle::plan(case, &bus.words);
                let memory = bus.words.clone();
                bus.elapsed[at] = elapsed;
                assert_eq!(
                    cpu.step_physical_instruction(&mut bus),
                    Err(InstructionFault::Entry(
                        EntryFault::InvalidCompleteDuration { elapsed }
                    ))
                );
                assert_eq!(cpu.registers, plan[at].registers);
                assert_eq!(bus.words, memory);
                assert_eq!(bus.events, oracle::events(case, &plan[..=at], &bus.elapsed));
                assert_eq!(bus.count, at + 1);
                assert_eq!(bus.entry_count, 0);
                assert_eq!(
                    cpu.clock().0,
                    case.start + oracle::clocks(case, &bus.elapsed, at + 1)
                );
                assert_eq!(cpu.clock().0, bus.clock);
                guarded(&mut cpu, &mut bus, TimingFault::Frozen);
            }
        }
    }
}

#[test]
fn short_qualified_boundary_elapsed_adds_fifty_only_without_transfer_minimum() {
    for form in forms() {
        for at in 0..form.count() {
            for elapsed in [0, 1, 3] {
                let (mut cpu, mut bus, case) = Case::new(form).seed();
                let plan = oracle::plan(case, &bus.words);
                let t = plan[at];
                let c = oracle::capture(t, 2);
                let memory = oracle::entry_memory(bus.words.clone(), t.registers.ssp, c);
                bus.elapsed[at] = elapsed;
                bus.failure = Some((at, Failure::Boundary));
                let clocks = oracle::clocks(case, &bus.elapsed, at + 1);
                assert_eq!(
                    cpu.step_physical_instruction(&mut bus),
                    Ok(PhysicalInstructionStep::Group0 {
                        opcode: form.opcode(case.destination, case.source),
                        capture: c,
                        clocks: clocks + 50
                    })
                );
                assert_eq!(cpu.registers, oracle::entered(t.registers));
                assert_eq!(bus.words, memory);
                oracle::frame(&bus, t.registers.ssp, c, case.start + clocks);
                assert_eq!(cpu.clock().0, case.start + clocks + 50);
                assert_eq!(cpu.clock().0, bus.clock);
            }
        }
    }
}

#[test]
fn maximum_final_complete_commits_without_an_extra_reservation() {
    for form in forms() {
        let last = form.count() - 1;
        let mut case = Case::new(form);
        case.destination = 7;
        case.start = u64::MAX - u64::from(u32::MAX) - form.idle() - last as u64 * 4;
        let (mut cpu, mut bus, case) = case.seed();
        let plan = oracle::plan(case, &bus.words);
        let t = plan[last];
        let memory = bus.words.clone();
        bus.elapsed[last] = u32::MAX;
        let clocks = oracle::clocks(case, &bus.elapsed, plan.len());
        assert_eq!(
            cpu.step_physical_instruction(&mut bus),
            Ok(PhysicalInstructionStep::Instruction(
                zeff_m68k::timing::TimedStep {
                    opcode: form.opcode(case.destination, case.source),
                    clocks,
                    exception: None
                }
            ))
        );
        let mut expected = t.registers;
        expected.pc += form.length();
        assert_eq!(cpu.registers, expected);
        assert_eq!(bus.words, memory);
        assert_eq!(
            cpu.queued_words(),
            Some([t.ird, bus.sample(t.access.logical_address)])
        );
        assert_eq!(bus.events, oracle::events(case, &plan, &bus.elapsed));
        assert_eq!(cpu.clock().0, u64::MAX);
        assert_eq!(cpu.clock().0, bus.clock);
    }
}

#[test]
fn maximum_completed_prefix_or_final_fault_preserves_consumption_and_published_state() {
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
                case.destination = 7;
                case.source = 7;
                case.start = u64::MAX - u64::from(u32::MAX) - form.idle() - at as u64 * 4;
                let (mut cpu, mut bus, case) = case.seed();
                let plan = oracle::plan(case, &bus.words);
                let t = plan[at];
                let memory = bus.words.clone();
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
                assert_eq!(bus.events, oracle::events(case, &plan[..=at], &bus.elapsed));
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
fn trace_or_accepted_irq_preflight_retains_all_source_banks_and_queued_words() {
    for form in forms() {
        for trace in [false, true] {
            let (mut cpu, mut bus, _) = Case::new(form).seed();
            if trace {
                cpu.registers
                    .set_status_register(cpu.registers.status_register() | TRACE);
            } else {
                cpu.interrupt_input().set_level(7);
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
        }
    }
}

#[test]
fn byte_movea_and_reserved_ea_encodings_are_rejected_before_any_effect() {
    let mut opcodes: Vec<_> = FAMILIES
        .into_iter()
        .map(|family| {
            Form {
                size: Size::Byte,
                family,
            }
            .opcode(7, 7)
        })
        .collect();
    opcodes.extend([0x307d, 0x307e, 0x307f, 0x207d, 0x207e, 0x207f]);
    for opcode in opcodes {
        let r = registers(false, 31);
        let mut cpu = PrefetchCpu::from_prefetched(r, [opcode, 0x8aa6], CpuClocks(37)).unwrap();
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
