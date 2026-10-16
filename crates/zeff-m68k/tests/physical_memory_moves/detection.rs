use super::*;

fn odd(mut case: Case, source: bool) -> Case {
    if !source {
        let old = case
            .r
            .address_register(usize::from(case.destination))
            .unwrap();
        case.r
            .set_address_register(usize::from(case.destination), old | 1);
    } else {
        match case.form.source {
            Source::Indirect | Source::Post | Source::Pre => {
                let old = case.r.address_register(usize::from(case.source)).unwrap();
                case.r
                    .set_address_register(usize::from(case.source), old | 1);
            }
            Source::AbsoluteLong => case.source_low |= 1,
            _ => case.extension |= 1,
        }
    }
    case
}

fn first_odd(plan: &[oracle::Transfer]) -> usize {
    plan.iter()
        .position(|t| t.access.logical_address & 1 != 0 && t.access.lanes == ByteLanes::Both)
        .unwrap()
}

#[test]
fn all_108_source_and_destination_odd_routes_use_default_and_custom_detection() {
    let mut routes = 0;
    let mut checked = 0;
    for form in forms().into_iter().filter(|f| f.size != Size::Byte) {
        for source_odd in [false, true] {
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
                    let (mut cpu, mut bus, case) = odd(case, source_odd).seed();
                    let initial = bus.words.clone();
                    let plan = oracle::plan(case, &initial);
                    let at = first_odd(&plan);
                    let t = plan[at];
                    assert_eq!(
                        oracle::direction(t),
                        if source_odd {
                            Direction::Read
                        } else {
                            Direction::Write
                        }
                    );
                    let c = oracle::capture(t, 3);
                    let prefix = oracle::clocks(case, &bus.elapsed, at, true);
                    let clocks = prefix + u64::from(detection.map_or(0, |(_, n)| n));
                    let mut events = oracle::events(case, &plan[..at], &bus.elapsed, true);
                    let result = if let Some(report) = detection {
                        bus.detection = Some(report);
                        events.push(Event::Detect(
                            t.access,
                            oracle::direction(t),
                            case.start + prefix,
                        ));
                        cpu.step_physical_instruction(&mut bus)
                    } else {
                        cpu.step_physical_instruction(&mut DefaultDetection(&mut bus))
                    };
                    assert_eq!(bus.count, at);
                    assert!(!bus.events.iter().any(|e| matches!(e, Event::Write(..))));
                    assert_eq!(
                        bus.events
                            .iter()
                            .filter(|e| matches!(e, Event::Read(..)))
                            .count(),
                        at
                    );
                    if detection.is_some_and(|(f, _)| f == Failure::Boundary) {
                        assert_eq!(
                            result,
                            Ok(PhysicalInstructionStep::Group0 {
                                opcode: form.opcode(case.source, case.destination),
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
                        assert_eq!(bus.words, oracle::entry_memory(initial, t.registers.ssp, c));
                        oracle::frame(&bus, t.registers.ssp, c, case.start + clocks);
                        assert_eq!(cpu.clock().0, case.start + clocks + 50);
                    } else {
                        let cause = CriticalFault::AddressError {
                            access: t.access,
                            direction: oracle::direction(t),
                        };
                        let host = detection.is_some_and(|(f, _)| f == Failure::Host);
                        assert_eq!(
                            result,
                            Err(if host {
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
                        assert_eq!(bus.words, initial);
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
                    checked += 1;
                }
            }
        }
    }
    assert_eq!(routes, 108);
    assert_eq!(checked, 1512);
}

#[test]
fn source_or_destination_a7_odd_bank_retains_updates_and_halts_at_actual_odd_ssp() {
    for size in [Size::Word, Size::Long] {
        for source in [Source::Indirect, Source::Post, Source::Pre] {
            for destination in DESTINATIONS {
                for source_odd in [false, true] {
                    for supervisor in [false, true] {
                        let mut case = Case::new(Form {
                            size,
                            source,
                            destination,
                        });
                        case.r = registers(supervisor, 31);
                        case.source = if source_odd { 7 } else { 0 };
                        case.destination = if source_odd { 0 } else { 7 };
                        let (mut cpu, mut bus, case) = odd(case, source_odd).seed();
                        bus.detection = Some((Failure::Boundary, 8));
                        let initial = bus.words.clone();
                        let plan = oracle::plan(case, &initial);
                        let at = first_odd(&plan);
                        let t = plan[at];
                        let c = oracle::capture(t, 3);
                        let clocks = oracle::clocks(case, &bus.elapsed, at, true) + 8;
                        let result = cpu.step_physical_instruction(&mut bus);
                        if supervisor {
                            assert!(matches!(
                                result,
                                Err(InstructionFault::Entry(EntryFault::TimingUnavailable(_)))
                            ));
                            assert_eq!(cpu.registers, t.registers);
                            assert_eq!(bus.words, initial);
                            assert_eq!(bus.entry_count, 0);
                            let mut events = oracle::events(case, &plan[..at], &bus.elapsed, true);
                            events.extend([
                                Event::Detect(
                                    t.access,
                                    oracle::direction(t),
                                    case.start + clocks - 8,
                                ),
                                Event::Idle(case.start + clocks, 4),
                                Event::EntryDetect(
                                    oracle::access(
                                        t.registers.ssp.wrapping_sub(2),
                                        true,
                                        false,
                                        AccessKind::Stack,
                                    ),
                                    Direction::Write,
                                    case.start + clocks + 4,
                                ),
                            ]);
                            assert_eq!(bus.events, events);
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
                                    opcode: case.form.opcode(case.source, case.destination),
                                    capture: c,
                                    clocks: clocks + 50
                                })
                            );
                            assert_eq!(cpu.registers, oracle::entered(t.registers));
                            assert_eq!(
                                bus.words,
                                oracle::entry_memory(initial, t.registers.ssp, c)
                            );
                            oracle::frame(&bus, t.registers.ssp, c, case.start + clocks);
                        }
                        assert_eq!(bus.count, at);
                        assert_eq!(cpu.clock().0, bus.clock);
                    }
                }
            }
        }
    }
}

#[test]
fn even_source_a7_with_odd_displacement_index_uses_even_phase_ssp() {
    for size in [Size::Word, Size::Long] {
        for source in [Source::Displacement, Source::Indexed] {
            let mut case = Case::new(Form {
                size,
                source,
                destination: Destination::Pre,
            });
            case.r = registers(true, 31);
            case.source = 7;
            case.destination = 7;
            let (mut cpu, mut bus, case) = odd(case, true).seed();
            bus.detection = Some((Failure::Boundary, 8));
            let initial = bus.words.clone();
            let plan = oracle::plan(case, &initial);
            let at = first_odd(&plan);
            let t = plan[at];
            let c = oracle::capture(t, 3);
            let clocks = oracle::clocks(case, &bus.elapsed, at, true) + 8;
            assert_eq!(
                cpu.step_physical_instruction(&mut bus),
                Ok(PhysicalInstructionStep::Group0 {
                    opcode: case.form.opcode(case.source, case.destination),
                    capture: c,
                    clocks: clocks + 50
                })
            );
            assert_eq!(cpu.registers, oracle::entered(t.registers));
            assert_eq!(bus.words, oracle::entry_memory(initial, t.registers.ssp, c));
            oracle::frame(&bus, t.registers.ssp, c, case.start + clocks);
        }
    }
}

#[test]
fn unknown_next_word_pre_odd_detection_is_consumed_without_entry_or_retry() {
    for source in SOURCES {
        for supervisor in [false, true] {
            let mut case = Case::new(Form {
                size: Size::Word,
                source,
                destination: Destination::Pre,
            });
            case.r = registers(supervisor, 31);
            case.next = 0x4afc;
            let (mut cpu, mut bus, case) = odd(case, false).seed();
            let memory = bus.words.clone();
            let plan = oracle::plan(case, &memory);
            let at = first_odd(&plan);
            let t = plan[at];
            assert_eq!(t.ird, 0x4afc);
            let cause = CriticalFault::AddressError {
                access: t.access,
                direction: Direction::Write,
            };
            bus.detection = Some((Failure::Boundary, 8));
            let prefix = oracle::clocks(case, &bus.elapsed, at, true);
            assert_eq!(
                cpu.step_physical_instruction(&mut bus),
                Err(InstructionFault::CaptureUnavailable {
                    phase: t.phase,
                    cause
                })
            );
            assert_eq!(cpu.registers, t.registers);
            assert_eq!(bus.words, memory);
            assert_eq!(bus.count, at);
            assert_eq!(bus.entry_count, 0);
            let mut events = oracle::events(case, &plan[..at], &bus.elapsed, true);
            events.push(Event::Detect(
                t.access,
                Direction::Write,
                case.start + prefix,
            ));
            assert_eq!(bus.events, events);
            assert_eq!(cpu.clock().0, case.start + prefix + 8);
            assert_eq!(cpu.clock().0, bus.clock);
            assert_eq!(cpu.queued_words(), None);
            assert_eq!(
                cpu.physical_entry_state(),
                PhysicalEntryState::FaultUnqualified { cause }
            );
            assert_eq!(cpu.physical_inspection(), PhysicalInspection::Unavailable);
            guarded(&mut cpu, &mut bus, TimingFault::PhysicalFaultUnqualified);
            let snapshot = cpu.clone();
            let events = bus.events.clone();
            let clock = bus.clock;
            assert_eq!(
                cpu.enter_imported_group0(&mut bus),
                Err(EntryFault::FaultUnqualified(cause))
            );
            assert_eq!(cpu, snapshot);
            assert_eq!(bus.events, events);
            assert_eq!(bus.words, memory);
            assert_eq!(bus.clock, clock);
        }
    }
}
