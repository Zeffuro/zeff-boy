use super::*;

pub(super) fn odd(mut case: Case) -> Case {
    match case.form.family {
        Family::Indirect | Family::Post | Family::Pre => {
            let value = case.r.address_register(usize::from(case.source)).unwrap() | 1;
            assert!(case.r.set_address_register(usize::from(case.source), value));
        }
        Family::AbsoluteLong => case.e4 |= 1,
        _ => case.e0 |= 1,
    }
    case
}

#[test]
fn all_18_odd_routes_use_true_default_or_explicit_detection_without_aligned_reads() {
    let mut routes = 0;
    for form in forms().into_iter().filter(|f| f.memory()) {
        routes += 1;
        for supervisor in [false, true] {
            for detection in [
                None,
                Some((Failure::Complete, 17)),
                Some((Failure::Host, 17)),
                Some((Failure::Unavailable, 17)),
                Some((Failure::Boundary, 0)),
                Some((Failure::Boundary, 3)),
                Some((Failure::Boundary, 17)),
            ] {
                let mut case = Case::new(form);
                case.r = registers(supervisor, 31);
                case.destination = 7;
                let (mut cpu, mut bus, case) = odd(case).seed();
                let memory = bus.words.clone();
                let plan = oracle::plan(case, &memory);
                let at = plan
                    .iter()
                    .position(|t| t.access.kind == AccessKind::Data)
                    .unwrap();
                let t = plan[at];
                let c = oracle::capture(t, 3);
                let prefix = oracle::clocks(case, &bus.elapsed, at);
                let clocks = prefix + u64::from(detection.map_or(0, |(_, n)| n));
                let mut events = oracle::events(case, &plan[..at], &bus.elapsed);
                let result = if let Some(report) = detection {
                    bus.detection = Some(report);
                    events.push(Event::Detect(
                        t.access,
                        Direction::Read,
                        case.start + prefix,
                    ));
                    cpu.step_physical_instruction(&mut bus)
                } else {
                    cpu.step_physical_instruction(&mut DefaultDetection(&mut bus))
                };
                assert_eq!(bus.count, at);
                assert!(
                    !bus.events
                        .iter()
                        .any(|e| matches!(e, Event::Read(a, _) if a.kind == AccessKind::Data))
                );
                if detection.is_some_and(|(f, _)| f == Failure::Boundary) {
                    assert_eq!(
                        result,
                        Ok(PhysicalInstructionStep::Group0 {
                            opcode: form.opcode(case.destination, case.source),
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
                    assert_eq!(bus.words, oracle::entry_memory(memory, t.registers.ssp, c));
                    oracle::frame(&bus, t.registers.ssp, c, case.start + clocks);
                    assert_eq!(cpu.clock().0, case.start + clocks + 50);
                } else {
                    let cause = CriticalFault::AddressError {
                        access: t.access,
                        direction: Direction::Read,
                    };
                    let host = detection.is_some_and(|(f, _)| f == Failure::Host);
                    assert_eq!(
                        result,
                        Err(if host {
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
                    assert_eq!(bus.words, memory);
                    assert_eq!(cpu.queued_words(), None);
                    assert_eq!(bus.entry_count, 0);
                    assert_eq!(cpu.clock().0, case.start + clocks);
                    if !host {
                        assert_eq!(
                            cpu.physical_entry_state(),
                            PhysicalEntryState::FaultUnqualified { cause }
                        );
                        assert_eq!(cpu.physical_inspection(), PhysicalInspection::Unavailable);
                    }
                    guarded(
                        &mut cpu,
                        &mut bus,
                        if host {
                            TimingFault::Frozen
                        } else {
                            TimingFault::PhysicalFaultUnqualified
                        },
                    );
                }
                assert_eq!(bus.events, events);
                assert_eq!(cpu.clock().0, bus.clock);
            }
        }
    }
    assert_eq!(routes, 18);
}

#[test]
fn source_a7_odd_updates_keep_user_usp_but_supervisor_phase_ssp_causes_secondary_halt() {
    for form in forms()
        .into_iter()
        .filter(|f| matches!(f.family, Family::Indirect | Family::Post | Family::Pre))
    {
        for supervisor in [false, true] {
            let mut case = Case::new(form);
            case.r = registers(supervisor, 31);
            case.source = 7;
            case.destination = 7;
            let (mut cpu, mut bus, case) = odd(case).seed();
            bus.detection = Some((Failure::Boundary, 8));
            let plan = oracle::plan(case, &bus.words);
            let t = plan[0];
            let c = oracle::capture(t, 3);
            let memory = bus.words.clone();
            let clocks = form.idle() + 8;
            let result = cpu.step_physical_instruction(&mut bus);
            if supervisor {
                assert!(matches!(
                    result,
                    Err(InstructionFault::Entry(EntryFault::TimingUnavailable(_)))
                ));
                assert_eq!(cpu.registers, t.registers);
                assert_eq!(bus.words, memory);
                assert_eq!(bus.entry_count, 0);
                assert_eq!(
                    &bus.events[bus.events.len() - 2..],
                    &[
                        Event::Idle(case.start + clocks, 4),
                        Event::EntryDetect(
                            oracle::access(
                                t.registers.ssp.wrapping_sub(2),
                                true,
                                false,
                                AccessKind::Stack
                            ),
                            Direction::Write,
                            case.start + clocks + 4
                        )
                    ]
                );
                assert_eq!(cpu.clock().0, case.start + clocks + 4);
                assert!(matches!(
                    cpu.physical_entry_state(),
                    PhysicalEntryState::DoubleFaultHalted { .. }
                ));
                guarded(&mut cpu, &mut bus, TimingFault::DoubleFaultHalted);
            } else {
                assert_eq!(
                    result,
                    Ok(PhysicalInstructionStep::Group0 {
                        opcode: form.opcode(7, 7),
                        capture: c,
                        clocks: clocks + 50
                    })
                );
                assert_eq!(cpu.registers, oracle::entered(t.registers));
                assert_eq!(cpu.registers.usp, t.registers.usp);
                assert_eq!(bus.words, oracle::entry_memory(memory, t.registers.ssp, c));
                oracle::frame(&bus, t.registers.ssp, c, case.start + clocks);
            }
            assert_eq!(bus.count, 0);
            assert_eq!(cpu.clock().0, bus.clock);
        }
    }
}

#[test]
fn even_source_a7_displacement_or_index_with_odd_ea_uses_the_even_stack() {
    for form in forms()
        .into_iter()
        .filter(|f| matches!(f.family, Family::Displacement | Family::Indexed))
    {
        for supervisor in [false, true] {
            let mut case = Case::new(form);
            case.r = registers(supervisor, 31);
            case.source = 7;
            case.destination = 7;
            let (mut cpu, mut bus, case) = odd(case).seed();
            let plan = oracle::plan(case, &bus.words);
            let t = plan[1];
            let c = oracle::capture(t, 3);
            bus.detection = Some((Failure::Boundary, 8));
            let clocks = form.idle() + 4 + 8;
            assert_eq!(
                cpu.step_physical_instruction(&mut bus),
                Ok(PhysicalInstructionStep::Group0 {
                    opcode: form.opcode(7, 7),
                    capture: c,
                    clocks: clocks + 50
                })
            );
            assert_eq!(cpu.registers, oracle::entered(t.registers));
            oracle::frame(&bus, case.r.ssp, c, case.start + clocks);
            assert_eq!(cpu.clock().0, bus.clock);
        }
    }
}

#[test]
fn three_literal_final_a7_publication_cases_distinguish_odd_values_from_odd_stack_access() {
    for (supervisor, value, words) in [
        (
            true,
            0x1234_8000,
            Some([0x1004, 0x201f, 0, 0x4e71, 0x1004, 0x4e76, 0]),
        ),
        (
            false,
            0x1234_8001,
            Some([0x1004, 0x001f, 0, 0x4e71, 0x1004, 0x4e72, 0]),
        ),
        (true, 0x1234_8001, None),
    ] {
        for failure in [Failure::Complete, Failure::Host, Failure::Boundary] {
            let mut case = Case::new(Form {
                size: Size::Word,
                family: Family::Data,
            });
            case.r = registers(supervisor, 31);
            case.r.pc = 0x1000;
            case.r.ssp = 0x2000;
            case.source = 0;
            case.destination = 7;
            case.value = value;
            case.e0 = 0x4e71;
            let (mut cpu, mut bus, case) = case.seed();
            let plan = oracle::plan(case, &bus.words);
            let t = plan[0];
            bus.failure = Some((0, failure));
            let result = cpu.step_physical_instruction(&mut bus);
            if failure == Failure::Complete {
                assert!(matches!(
                    result,
                    Ok(PhysicalInstructionStep::Instruction(_))
                ));
                let mut expected = t.registers;
                expected.pc = 0x1002;
                assert_eq!(cpu.registers, expected);
                assert_eq!(bus.entry_count, 0);
            } else if failure == Failure::Host {
                assert!(matches!(
                    result,
                    Err(InstructionFault::Entry(EntryFault::HostFailure { .. }))
                ));
                assert_eq!(cpu.registers, t.registers);
                assert_eq!(bus.entry_count, 0);
                guarded(&mut cpu, &mut bus, TimingFault::Frozen);
            } else if let Some(words) = words {
                assert!(matches!(
                    result,
                    Ok(PhysicalInstructionStep::Group0 { clocks: 54, .. })
                ));
                let actual: Vec<_> = bus
                    .events
                    .iter()
                    .filter_map(|e| {
                        if let Event::EntryWrite(_, v, _) = e {
                            Some(*v)
                        } else {
                            None
                        }
                    })
                    .collect();
                assert_eq!(actual, words);
                assert_eq!(cpu.registers, oracle::entered(t.registers));
                assert_eq!(
                    cpu.registers.ssp,
                    if supervisor { 0xffff_7ff2 } else { 0x1ff2 }
                );
                if !supervisor {
                    assert_eq!(cpu.registers.usp, 0xffff_8001);
                }
            } else {
                assert!(matches!(
                    result,
                    Err(InstructionFault::Entry(EntryFault::TimingUnavailable(_)))
                ));
                assert_eq!(cpu.registers.ssp, 0xffff_8001);
                assert_eq!(bus.entry_count, 0);
                assert_eq!(cpu.clock().0, case.start + 8);
                assert!(matches!(
                    cpu.physical_entry_state(),
                    PhysicalEntryState::DoubleFaultHalted { .. }
                ));
                guarded(&mut cpu, &mut bus, TimingFault::DoubleFaultHalted);
            }
            assert_eq!(cpu.clock().0, bus.clock);
        }
    }
}
