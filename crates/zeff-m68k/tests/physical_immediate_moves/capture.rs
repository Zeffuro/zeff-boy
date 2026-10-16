use super::*;

pub(super) fn failed(case: Case, at: usize, failure: Failure) {
    let (mut cpu, mut bus, case) = case.seed();
    bus.elapsed = [7, 11, 5, 13, 9, 6, 8];
    bus.failure = Some((at, failure));
    let plan = oracle::plan(case, &bus.words);
    let t = plan[at];
    let c = oracle::capture(t, 2);
    let memory = oracle::memory(bus.words.clone(), &plan[..=at]);
    let idle = at >= case.form.prefix();
    let clocks = oracle::clocks(case, &bus.elapsed, at + 1, idle);
    let mut events = oracle::events(case, &plan[..=at], &bus.elapsed, idle);
    let result = cpu.step_physical_instruction(&mut bus);
    if failure == Failure::Boundary {
        assert_eq!(
            result,
            Ok(PhysicalInstructionStep::Group0 {
                opcode: case.form.opcode(case.destination),
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
            direction: oracle::direction(t),
        };
        assert_eq!(
            result,
            Err(if failure == Failure::Host {
                InstructionFault::Entry(EntryFault::HostFailure {
                    access: t.access,
                    direction: oracle::direction(t),
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
fn all_92_positions_cover_three_outcomes_banks_every_ccr_and_distinct_source_values() {
    let mut positions = 0;
    for form in forms() {
        for at in 0..form.count() {
            positions += 1;
            for supervisor in [false, true] {
                for ccr in 0..32 {
                    for value in [0, 0x0001_0000, 0x0000_8000, 0x8000_0001] {
                        for failure in [Failure::Host, Failure::Boundary, Failure::Unavailable] {
                            let mut case = Case::new(form);
                            case.r = registers(supervisor, ccr);
                            case.destination = 7;
                            case.value = value;
                            case.low = value as u16;
                            failed(case, at, failure);
                        }
                    }
                }
            }
        }
    }
    assert_eq!(positions, 92);
}

#[test]
fn base_index_a7_alias_faults_observe_old_index_bank_and_phase_ssp() {
    for size in [Size::Byte, Size::Word, Size::Long] {
        let form = Form {
            size,
            family: Family::Indexed,
        };
        for supervisor in [false, true] {
            for long in [false, true] {
                for at in 0..form.count() {
                    let mut case = Case::new(form);
                    case.r = registers(supervisor, 31);
                    case.destination = 7;
                    case.extension = 0xf6f0 | if long { 0x800 } else { 0 };
                    failed(case, at, Failure::Boundary);
                }
            }
        }
    }
}

#[test]
fn secondary_entry_failures_retain_prefix_stores_current_phase_ssp_and_all_state() {
    for supervisor in [false, true] {
        for at in 0..11 {
            for failure in [Failure::Host, Failure::Boundary, Failure::Unavailable] {
                let mut case = Case::new(Form {
                    size: Size::Long,
                    family: Family::Post,
                });
                case.r = registers(supervisor, 31);
                case.destination = 7;
                let (mut cpu, mut bus, case) = case.seed();
                let plan = oracle::plan(case, &bus.words);
                let last = plan.len() - 1;
                let t = plan[last];
                let c = oracle::capture(t, 2);
                bus.failure = Some((last, Failure::Boundary));
                bus.entry_failure = Some((at, failure));
                let start = case.start + oracle::clocks(case, &bus.elapsed, plan.len(), true);
                let schedule = oracle::entry_events(t.registers.ssp, c, start);
                let end = schedule
                    .iter()
                    .enumerate()
                    .filter(|(_, e)| matches!(e, Event::EntryRead(..) | Event::EntryWrite(..)))
                    .nth(at)
                    .unwrap()
                    .0;
                let mut memory = oracle::memory(bus.words.clone(), &plan);
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
                let mut events = oracle::events(case, &plan, &bus.elapsed, true);
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
fn unknown_actual_next_allows_early_o_but_consumed_n_blocks_both_apis_without_retry() {
    for form in forms() {
        let mut case = Case::new(form);
        case.next = 0x4afc;
        for at in 0..form.count() {
            let (mut cpu, mut bus, case) = case.seed();
            let plan = oracle::plan(case, &bus.words);
            let t = plan[at];
            if t.ird != 0x4afc {
                failed(case, at, Failure::Boundary);
                continue;
            }
            let address = case.r.pc + form.length();
            bus.word(address, 0x7201);
            bus.mutate_read = Some((address, 0x4afc));
            let mut memory = bus.words.clone();
            memory.insert(address & BUS_ADDRESS_MASK, 0x4afc);
            let memory = oracle::memory(memory, &plan[..=at]);
            bus.failure = Some((at, Failure::Boundary));
            let cause = CriticalFault::BusAbort {
                access: t.access,
                direction: oracle::direction(t),
            };
            let idle = at >= form.prefix();
            assert_eq!(
                cpu.step_physical_instruction(&mut bus),
                Err(InstructionFault::CaptureUnavailable {
                    phase: t.phase,
                    cause
                })
            );
            assert_eq!(cpu.registers, t.registers);
            assert_eq!(bus.words, memory);
            assert_eq!(cpu.queued_words(), None);
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
            assert_eq!(
                cpu.physical_entry_state(),
                PhysicalEntryState::FaultUnqualified { cause }
            );
            assert_eq!(cpu.physical_inspection(), PhysicalInspection::Unavailable);
            guarded(&mut cpu, &mut bus, TimingFault::PhysicalFaultUnqualified);
            let before = cpu.clone();
            let events = bus.events.clone();
            let memory = bus.words.clone();
            let clock = bus.clock;
            assert_eq!(
                cpu.enter_imported_group0(&mut bus),
                Err(EntryFault::FaultUnqualified(cause))
            );
            assert_eq!(cpu, before);
            assert_eq!(bus.events, events);
            assert_eq!(bus.words, memory);
            assert_eq!(bus.clock, clock);
        }
        completion::completed(case, false);
    }
}

#[test]
fn retained_actual_next_survives_exception_stack_overlapping_the_source_stream() {
    for form in forms() {
        let mut case = Case::new(form);
        case.r = registers(true, 31);
        case.r.pc = 0x4000;
        case.r.ssp = 0x4000 + form.length() + 2;
        let (mut cpu, mut bus, case) = case.seed();
        let next_address = case.r.pc + form.length();
        bus.mutate_read = Some((next_address, 0x7602));
        let mut memory = bus.words.clone();
        memory.insert(next_address & BUS_ADDRESS_MASK, 0x7602);
        let plan = oracle::plan(case, &memory);
        let last = plan.len() - 1;
        let t = plan[last];
        let c = oracle::capture(t, 2);
        bus.failure = Some((last, Failure::Boundary));
        let clocks = oracle::clocks(case, &bus.elapsed, plan.len(), true);
        assert_eq!(
            cpu.step_physical_instruction(&mut bus),
            Ok(PhysicalInstructionStep::Group0 {
                opcode: form.opcode(case.destination),
                capture: c,
                clocks: clocks + 50
            })
        );
        assert_eq!(c.ird, 0x7602);
        assert_eq!(cpu.registers, oracle::entered(t.registers));
        assert_eq!(
            bus.words,
            oracle::entry_memory(oracle::memory(memory, &plan), t.registers.ssp, c)
        );
        oracle::frame(&bus, t.registers.ssp, c, case.start + clocks);
        assert_eq!(cpu.clock().0, bus.clock);
    }
}
