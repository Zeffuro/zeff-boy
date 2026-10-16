use super::*;

fn failed(case: Case, at: usize, failure: Failure) {
    let (mut cpu, mut bus, case) = case.seed();
    bus.elapsed = [7, 11, 5, 13, 9];
    bus.failure = Some((at, failure));
    let plan = oracle::plan(case, &bus.words);
    let t = plan[at];
    let c = oracle::capture(t, 2);
    let memory = bus.words.clone();
    let clocks = oracle::clocks(case, &bus.elapsed, at + 1);
    let mut events = oracle::events(case, &plan[..=at], &bus.elapsed);
    let result = cpu.step_physical_instruction(&mut bus);
    if failure == Failure::Boundary {
        assert_eq!(
            result,
            Ok(PhysicalInstructionStep::Group0 {
                opcode: case.form.opcode(case.destination, case.source),
                capture: c,
                clocks: clocks + 50
            })
        );
        events.extend(oracle::entry_events(
            t.registers.ssp,
            c,
            case.start + clocks,
        ));
        assert_eq!(cpu.registers, oracle::entered(t.registers));
        assert_eq!(cpu.queued_words(), Some([0x4e71, 0x7201]));
        assert_eq!(bus.words, oracle::entry_memory(memory, t.registers.ssp, c));
        oracle::frame(&bus, t.registers.ssp, c, case.start + clocks);
        assert_eq!(cpu.clock().0, case.start + clocks + 50);
    } else {
        let cause = CriticalFault::BusAbort {
            access: t.access,
            direction: Direction::Read,
        };
        assert_eq!(
            result,
            Err(if failure == Failure::Host {
                InstructionFault::Entry(EntryFault::HostFailure {
                    access: t.access,
                    direction: Direction::Read,
                    source: "host",
                })
            } else {
                InstructionFault::TransitionTimingUnavailable {
                    phase: t.phase,
                    cause,
                }
            })
        );
        assert_eq!(cpu.registers, t.registers);
        assert_eq!(cpu.registers.d, case.r.d);
        assert_eq!(cpu.registers.status_register(), case.r.status_register());
        assert_eq!(cpu.queued_words(), None);
        assert_eq!(bus.words, memory);
        assert_eq!(bus.entry_count, 0);
        assert_eq!(cpu.clock().0, case.start + clocks);
        if failure == Failure::Unavailable {
            assert_eq!(
                cpu.physical_entry_state(),
                PhysicalEntryState::FaultUnqualified { cause }
            );
            assert_eq!(cpu.physical_inspection(), PhysicalInspection::Unavailable);
        }
        guarded(
            &mut cpu,
            &mut bus,
            if failure == Failure::Host {
                TimingFault::Frozen
            } else {
                TimingFault::PhysicalFaultUnqualified
            },
        );
    }
    assert_eq!(bus.count, at + 1);
    assert_eq!(bus.events, events);
    assert_eq!(cpu.clock().0, bus.clock);
}

#[test]
fn all_68_positions_separate_host_boundary_and_unavailable_with_phase_stack_banks() {
    let mut positions = 0;
    for form in forms() {
        for at in 0..form.count() {
            positions += 1;
            for supervisor in [false, true] {
                for failure in [Failure::Host, Failure::Boundary, Failure::Unavailable] {
                    let mut case = Case::new(form);
                    case.r = registers(supervisor, 31);
                    case.destination = 7;
                    case.source = 7;
                    failed(case, at, failure);
                }
            }
        }
    }
    assert_eq!(positions, 68);
}

#[test]
fn all_32_ccrs_remain_unchanged_at_every_phase_with_destination_aliases() {
    for form in forms() {
        for ccr in 0..32 {
            for at in 0..form.count() {
                let mut case = Case::new(form);
                case.r = registers(true, ccr);
                case.destination = 2;
                failed(case, at, Failure::Boundary);
            }
        }
    }
}

#[test]
fn all_secondary_entry_failures_retain_the_published_destination_and_phase_ssp() {
    for supervisor in [false, true] {
        for at in 0..11 {
            for failure in [Failure::Host, Failure::Boundary, Failure::Unavailable] {
                let mut case = Case::new(Form {
                    size: Size::Long,
                    family: Family::Post,
                });
                case.r = registers(supervisor, 31);
                case.source = 7;
                case.destination = 7;
                case.value = 0x3456_9000;
                let (mut cpu, mut bus, case) = case.seed();
                let plan = oracle::plan(case, &bus.words);
                let last = plan.len() - 1;
                let t = plan[last];
                bus.failure = Some((last, Failure::Boundary));
                bus.entry_failure = Some((at, failure));
                let c = oracle::capture(t, 2);
                let start = case.start + oracle::clocks(case, &bus.elapsed, plan.len());
                let schedule = oracle::entry_events(t.registers.ssp, c, start);
                let end = schedule
                    .iter()
                    .enumerate()
                    .filter(|(_, e)| matches!(e, Event::EntryRead(..) | Event::EntryWrite(..)))
                    .nth(at)
                    .unwrap()
                    .0;
                let mut memory = bus.words.clone();
                for e in &schedule[..=end] {
                    if let Event::EntryWrite(a, v, _) = e {
                        memory.insert(a.bus_address, *v);
                    }
                }
                assert!(matches!(
                    cpu.step_physical_instruction(&mut bus),
                    Err(InstructionFault::Entry(_))
                ));
                assert_eq!(cpu.registers, t.registers);
                assert_eq!(cpu.queued_words(), None);
                assert_eq!(bus.words, memory);
                let mut events = oracle::events(case, &plan, &bus.elapsed);
                events.extend_from_slice(&schedule[..=end]);
                assert_eq!(bus.events, events);
                assert_eq!(bus.entry_count, at + 1);
                assert_eq!(bus.count, plan.len());
                assert_eq!(
                    cpu.clock().0,
                    start + 4 + (at as u64 + 1) * 4 + if at == 10 { 2 } else { 0 }
                );
                assert_eq!(cpu.clock().0, bus.clock);
                if failure != Failure::Host {
                    assert!(matches!(
                        cpu.physical_entry_state(),
                        PhysicalEntryState::DoubleFaultHalted { .. }
                    ));
                    assert_eq!(cpu.physical_inspection(), PhysicalInspection::Unavailable);
                }
                guarded(
                    &mut cpu,
                    &mut bus,
                    if failure == Failure::Host {
                        TimingFault::Frozen
                    } else {
                        TimingFault::DoubleFaultHalted
                    },
                );
            }
        }
    }
}

#[test]
fn unknown_actual_next_is_consumed_only_at_final_f_and_blocks_both_entry_apis() {
    for form in forms() {
        let mut case = Case::new(form);
        if form.length() == 2 {
            case.e0 = 0x4afc;
        } else {
            case.next = 0x4afc;
        }
        for at in 0..form.count() {
            if at + 1 != form.count() {
                failed(case, at, Failure::Boundary);
                continue;
            }
            let mut actual = case;
            if form.length() != 2 {
                actual.next = 0x7201;
            }
            let (mut cpu, mut bus, case) = actual.seed();
            let mut memory = bus.words.clone();
            if form.length() != 2 {
                bus.mutate_read = Some((case.r.pc + form.length(), 0x4afc));
                memory.insert((case.r.pc + form.length()) & BUS_ADDRESS_MASK, 0x4afc);
            }
            let plan = oracle::plan(case, &memory);
            let t = plan[at];
            bus.failure = Some((at, Failure::Boundary));
            let cause = CriticalFault::BusAbort {
                access: t.access,
                direction: Direction::Read,
            };
            assert_eq!(
                cpu.step_physical_instruction(&mut bus),
                Err(InstructionFault::CaptureUnavailable {
                    phase: t.phase,
                    cause
                })
            );
            assert_eq!(cpu.registers, t.registers);
            assert_eq!(bus.words, memory);
            assert_eq!(bus.events, oracle::events(case, &plan, &bus.elapsed));
            assert_eq!(bus.count, plan.len());
            assert_eq!(bus.entry_count, 0);
            assert_eq!(cpu.queued_words(), None);
            assert_eq!(
                cpu.clock().0,
                case.start + oracle::clocks(case, &bus.elapsed, plan.len())
            );
            assert_eq!(cpu.clock().0, bus.clock);
            assert_eq!(
                cpu.physical_entry_state(),
                PhysicalEntryState::FaultUnqualified { cause }
            );
            assert_eq!(cpu.physical_inspection(), PhysicalInspection::Unavailable);
            guarded(&mut cpu, &mut bus, TimingFault::PhysicalFaultUnqualified);
            let before = cpu.clone();
            let events = bus.events.clone();
            assert_eq!(
                cpu.enter_imported_group0(&mut bus),
                Err(EntryFault::FaultUnqualified(cause))
            );
            assert_eq!(cpu, before);
            assert_eq!(bus.events, events);
        }
        let (mut cpu, mut bus, case) = case.seed();
        assert!(matches!(
            cpu.step_physical_instruction(&mut bus),
            Ok(PhysicalInstructionStep::Instruction(_))
        ));
        assert_eq!(cpu.queued_words(), Some([0x4afc, case.last]));
        assert_eq!(cpu.clock().0, bus.clock);
    }
}

#[test]
fn retained_actual_next_survives_destination_stack_overlapping_the_instruction() {
    for form in forms()
        .into_iter()
        .filter(|f| matches!(f.family, Family::Data | Family::Immediate))
    {
        let mut case = Case::new(form);
        case.r = registers(true, 31);
        case.r.pc = 0x4000;
        case.destination = 7;
        case.value = case.r.pc + form.length() + 2;
        let (mut cpu, mut bus, case) = case.seed();
        let plan = oracle::plan(case, &bus.words);
        let last = plan.len() - 1;
        let t = plan[last];
        let c = oracle::capture(t, 2);
        let memory = oracle::entry_memory(bus.words.clone(), t.registers.ssp, c);
        bus.failure = Some((last, Failure::Boundary));
        let clocks = oracle::clocks(case, &bus.elapsed, plan.len());
        assert_eq!(
            cpu.step_physical_instruction(&mut bus),
            Ok(PhysicalInstructionStep::Group0 {
                opcode: form.opcode(case.destination, case.source),
                capture: c,
                clocks: clocks + 50
            })
        );
        assert_eq!(c.ird, 0x7201);
        assert_eq!(cpu.registers, oracle::entered(t.registers));
        assert_eq!(bus.words, memory);
        oracle::frame(&bus, t.registers.ssp, c, case.start + clocks);
        assert_eq!(cpu.clock().0, bus.clock);
    }
}
