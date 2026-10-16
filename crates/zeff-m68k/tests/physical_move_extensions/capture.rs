use super::*;

fn failed(case: Case, at: usize, failure: Failure) {
    let (mut cpu, mut bus, case) = case.seed();
    bus.elapsed = [7, 11, 5, 13, 9];
    bus.failure = Some((at, failure));
    let plan = oracle::plan(case, &bus.words);
    let t = plan[at];
    let capture = oracle::capture(t, 2);
    let clocks = oracle::clocks(case, &bus.elapsed, at + 1);
    let memory = oracle::memory(bus.words.clone(), &plan[..=at]);
    let mut events = oracle::events(case, &plan[..=at], &bus.elapsed);
    let result = cpu.step_physical_instruction(&mut bus);
    assert_eq!(bus.count, at + 1);
    if failure == Failure::Boundary {
        assert_eq!(
            result,
            Ok(PhysicalInstructionStep::Group0 {
                opcode: case.form.opcode(case.dn, case.an),
                capture,
                clocks: clocks + 50
            })
        );
        events.extend(oracle::entry_events(
            case.r.ssp,
            capture,
            case.start + clocks,
        ));
        assert_eq!(cpu.registers, oracle::entered(t.registers));
        assert_eq!(cpu.queued_words(), Some([0x4e71, 0x7201]));
        assert_eq!(bus.words, oracle::entry_memory(memory, case.r.ssp, capture));
        oracle::frame(&bus, case.r.ssp, capture, case.start + clocks);
        assert_eq!(cpu.clock().0, case.start + clocks + 50);
        assert_eq!(bus.entry_count, 11);
    } else {
        let cause = CriticalFault::BusAbort {
            access: t.access,
            direction: capture.direction,
        };
        assert_eq!(
            result,
            Err(if failure == Failure::Host {
                InstructionFault::Entry(EntryFault::HostFailure {
                    access: t.access,
                    direction: capture.direction,
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
    assert_eq!(bus.events, events);
    assert_eq!(cpu.clock().0, bus.clock);
}

#[test]
fn all_113_transfer_positions_separate_host_boundary_and_unavailable_in_both_banks() {
    let mut positions = 0;
    for form in forms() {
        for at in 0..form.count() {
            positions += 1;
            for supervisor in [false, true] {
                for failure in [Failure::Host, Failure::Boundary, Failure::Unavailable] {
                    let mut case = Case::new(form);
                    case.r = registers(supervisor, 31);
                    case.an = 7;
                    failed(case, at, failure);
                }
            }
        }
    }
    assert_eq!(positions, 113);
}

#[test]
fn all_ccr_patterns_and_long_values_distinguish_high_old_low_full_and_early_absolute_flags() {
    for form in forms().into_iter().filter(|f| f.write) {
        for ccr in 0..32 {
            for value in [0, 0x0001_0000, 0x0000_8000, 0x8000_0001] {
                for at in 0..form.count() {
                    let mut case = Case::new(form);
                    case.r = registers(true, ccr);
                    case.value = value;
                    failed(case, at, Failure::Boundary);
                }
            }
        }
    }
}

#[test]
fn extension_words_do_not_classify_early_faults_or_success_but_actual_next_classifies_final_fault()
{
    for form in forms() {
        for next in [0x4afc, 0x2970, 0x6b5f] {
            let mut case = Case::new(form);
            case.next = next;
            case.e0 = 0x4afc;
            for at in 0..form.count() {
                if at + 1 != form.count() || next != 0x4afc {
                    failed(case, at, Failure::Boundary);
                } else {
                    let (mut cpu, mut bus, case) = case.seed();
                    let plan = oracle::plan(case, &bus.words);
                    bus.failure = Some((at, Failure::Boundary));
                    let t = plan[at];
                    let cause = CriticalFault::BusAbort {
                        access: t.access,
                        direction: Direction::Read,
                    };
                    let memory = oracle::memory(bus.words.clone(), &plan);
                    assert_eq!(
                        cpu.step_physical_instruction(&mut bus),
                        Err(InstructionFault::CaptureUnavailable {
                            phase: InstructionPhase::OperandRefill,
                            cause
                        })
                    );
                    assert_eq!(cpu.registers, t.registers);
                    assert_eq!(bus.words, memory);
                    assert_eq!(bus.events, oracle::events(case, &plan, &bus.elapsed));
                    assert_eq!(
                        cpu.clock().0,
                        case.start + oracle::clocks(case, &bus.elapsed, plan.len())
                    );
                    assert_eq!(cpu.clock().0, bus.clock);
                    assert_eq!(bus.entry_count, 0);
                    assert_eq!(cpu.physical_inspection(), PhysicalInspection::Unavailable);
                    assert_eq!(
                        cpu.physical_entry_state(),
                        PhysicalEntryState::FaultUnqualified { cause }
                    );
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
            }
            let (mut cpu, mut bus, _) = case.seed();
            assert!(matches!(
                cpu.step_physical_instruction(&mut bus),
                Ok(PhysicalInstructionStep::Instruction(_))
            ));
            assert_eq!(cpu.queued_words(), Some([next, case.last]));
            assert_eq!(cpu.clock().0, bus.clock);
        }
    }
}

#[test]
fn every_secondary_entry_transfer_keeps_completed_store_ram_without_retry_or_partial_commit() {
    for family in [Family::Displacement, Family::AbsoluteLong] {
        let form = Form {
            size: Size::Long,
            family,
            write: true,
        };
        for at in 0..11 {
            for failure in [Failure::Host, Failure::Boundary, Failure::Unavailable] {
                let (mut cpu, mut bus, case) = Case::new(form).seed();
                let plan = oracle::plan(case, &bus.words);
                let last = plan.len() - 1;
                bus.failure = Some((last, Failure::Boundary));
                bus.entry_failure = Some((at, failure));
                let c = oracle::capture(plan[last], 2);
                let start = case.start + oracle::clocks(case, &bus.elapsed, plan.len());
                let schedule = oracle::entry_events(case.r.ssp, c, start);
                let end = schedule
                    .iter()
                    .enumerate()
                    .filter(|(_, e)| matches!(e, Event::EntryRead(..) | Event::EntryWrite(..)))
                    .nth(at)
                    .unwrap()
                    .0;
                let mut memory = oracle::memory(bus.words.clone(), &plan);
                for event in &schedule[..=end] {
                    if let Event::EntryWrite(access, value, _) = event {
                        memory.insert(access.bus_address, *value);
                    }
                }
                let result = cpu.step_physical_instruction(&mut bus);
                assert!(matches!(result, Err(InstructionFault::Entry(_))));
                assert_eq!(cpu.registers, plan[last].registers);
                assert_eq!(cpu.queued_words(), None);
                assert_eq!(bus.words, memory);
                let mut events = oracle::events(case, &plan, &bus.elapsed);
                events.extend_from_slice(&schedule[..=end]);
                assert_eq!(bus.events, events);
                assert_eq!(bus.entry_count, at + 1);
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
fn callback_mutated_actual_next_controls_queue_and_final_capture_class() {
    for form in forms() {
        for next in [0x2970, 0x4afc] {
            for boundary in [false, true] {
                let (mut cpu, mut bus, case) = Case::new(form).seed();
                let address = case.r.pc + form.length();
                bus.mutate_read = Some((address, next));
                let mut image = bus.words.clone();
                image.insert(address & BUS_ADDRESS_MASK, next);
                let plan = oracle::plan(case, &image);
                let last = plan.len() - 1;
                let t = plan[last];
                let memory = oracle::memory(image, &plan);
                if boundary {
                    bus.failure = Some((last, Failure::Boundary));
                }
                let clocks = oracle::clocks(case, &bus.elapsed, plan.len());
                let result = cpu.step_physical_instruction(&mut bus);
                if !boundary {
                    assert!(matches!(
                        result,
                        Ok(PhysicalInstructionStep::Instruction(_))
                    ));
                    let mut expected = t.registers;
                    expected.pc += form.length();
                    assert_eq!(cpu.registers, expected);
                    assert_eq!(cpu.queued_words(), Some([next, case.last]));
                    assert_eq!(bus.words, memory);
                    assert_eq!(cpu.clock().0, case.start + clocks);
                } else if next == 0x2970 {
                    let c = oracle::capture(t, 2);
                    assert_eq!(
                        result,
                        Ok(PhysicalInstructionStep::Group0 {
                            opcode: form.opcode(case.dn, case.an),
                            capture: c,
                            clocks: clocks + 50
                        })
                    );
                    assert_eq!(c.ird, next);
                    assert_eq!(cpu.registers, oracle::entered(t.registers));
                    assert_eq!(bus.words, oracle::entry_memory(memory, case.r.ssp, c));
                    oracle::frame(&bus, case.r.ssp, c, case.start + clocks);
                } else {
                    let cause = CriticalFault::BusAbort {
                        access: t.access,
                        direction: Direction::Read,
                    };
                    assert_eq!(
                        result,
                        Err(InstructionFault::CaptureUnavailable {
                            phase: t.phase,
                            cause
                        })
                    );
                    assert_eq!(cpu.registers, t.registers);
                    assert_eq!(bus.words, memory);
                    assert_eq!(bus.entry_count, 0);
                    assert_eq!(cpu.clock().0, case.start + clocks);
                    guarded(&mut cpu, &mut bus, TimingFault::PhysicalFaultUnqualified);
                }
                let events = oracle::events(case, &plan, &bus.elapsed);
                assert_eq!(bus.events[..events.len()], events);
                assert!(bus.mutate_read.is_none());
                assert_eq!(bus.count, form.count());
                assert_eq!(cpu.clock().0, bus.clock);
            }
        }
    }
}
