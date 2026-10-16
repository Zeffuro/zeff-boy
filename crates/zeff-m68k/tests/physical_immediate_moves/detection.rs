use super::*;

fn odd(mut case: Case) -> Case {
    match case.form.family {
        Family::Indirect | Family::Post | Family::Pre => {
            let old = case
                .r
                .address_register(usize::from(case.destination))
                .unwrap();
            case.r
                .set_address_register(usize::from(case.destination), old | 1);
        }
        Family::AbsoluteLong => case.destination_low |= 1,
        _ => case.extension |= 1,
    }
    case
}

#[test]
fn all_14_odd_routes_use_default_or_custom_detection_with_no_operand_write() {
    let mut routes = 0;
    for form in forms().into_iter().filter(|f| f.size != Size::Byte) {
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
                let (mut cpu, mut bus, case) = odd(case).seed();
                let memory = bus.words.clone();
                let plan = oracle::plan(case, &memory);
                let at = plan.iter().position(|t| t.value.is_some()).unwrap();
                let t = plan[at];
                let c = oracle::capture(t, 3);
                let prefix = oracle::clocks(case, &bus.elapsed, at, true);
                let clocks = prefix + u64::from(detection.map_or(0, |(_, n)| n));
                let mut events = oracle::events(case, &plan[..at], &bus.elapsed, true);
                let result = if let Some(report) = detection {
                    bus.detection = Some(report);
                    events.push(Event::Detect(
                        t.access,
                        Direction::Write,
                        case.start + prefix,
                    ));
                    cpu.step_physical_instruction(&mut bus)
                } else {
                    cpu.step_physical_instruction(&mut DefaultDetection(&mut bus))
                };
                assert_eq!(bus.count, at);
                assert!(!bus.events.iter().any(|e| matches!(e, Event::Write(..))));
                if detection.is_some_and(|(f, _)| f == Failure::Boundary) {
                    assert_eq!(
                        result,
                        Ok(PhysicalInstructionStep::Group0 {
                            opcode: form.opcode(case.destination),
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
                        direction: Direction::Write,
                    };
                    let host = detection.is_some_and(|(f, _)| f == Failure::Host);
                    assert_eq!(
                        result,
                        Err(if host {
                            InstructionFault::Entry(EntryFault::HostFailure {
                                access: t.access,
                                direction: Direction::Write,
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
    assert_eq!(routes, 14);
}

#[test]
fn odd_destination_a7_updates_user_usp_but_supervisor_stack_access_halts() {
    for form in forms().into_iter().filter(|f| {
        f.size != Size::Byte && matches!(f.family, Family::Indirect | Family::Post | Family::Pre)
    }) {
        for supervisor in [false, true] {
            let mut case = Case::new(form);
            case.r = registers(supervisor, 31);
            case.destination = 7;
            let (mut cpu, mut bus, case) = odd(case).seed();
            bus.detection = Some((Failure::Boundary, 8));
            let memory = bus.words.clone();
            let plan = oracle::plan(case, &memory);
            let at = plan.iter().position(|t| t.value.is_some()).unwrap();
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
                        opcode: form.opcode(7),
                        capture: c,
                        clocks: clocks + 50
                    })
                );
                assert_eq!(cpu.registers, oracle::entered(t.registers));
                assert_eq!(cpu.registers.usp, t.registers.usp);
                assert_eq!(bus.words, oracle::entry_memory(memory, t.registers.ssp, c));
                oracle::frame(&bus, t.registers.ssp, c, case.start + clocks);
            }
            assert_eq!(bus.count, at);
            assert_eq!(cpu.clock().0, bus.clock);
        }
    }
}

#[test]
fn even_a7_base_with_odd_displacement_or_index_ea_still_uses_even_ssp() {
    for form in forms().into_iter().filter(|f| {
        f.size != Size::Byte && matches!(f.family, Family::Displacement | Family::Indexed)
    }) {
        for supervisor in [false, true] {
            let mut case = Case::new(form);
            case.r = registers(supervisor, 31);
            case.destination = 7;
            let (mut cpu, mut bus, case) = odd(case).seed();
            let memory = bus.words.clone();
            let plan = oracle::plan(case, &memory);
            let at = plan.iter().position(|t| t.value.is_some()).unwrap();
            let t = plan[at];
            let c = oracle::capture(t, 3);
            bus.detection = Some((Failure::Boundary, 8));
            let clocks = oracle::clocks(case, &bus.elapsed, at, true) + 8;
            assert_eq!(
                cpu.step_physical_instruction(&mut bus),
                Ok(PhysicalInstructionStep::Group0 {
                    opcode: form.opcode(7),
                    capture: c,
                    clocks: clocks + 50
                })
            );
            assert_eq!(cpu.registers, oracle::entered(t.registers));
            assert_eq!(bus.words, oracle::entry_memory(memory, t.registers.ssp, c));
            oracle::frame(&bus, t.registers.ssp, c, case.start + clocks);
            assert_eq!(cpu.clock().0, bus.clock);
        }
    }
}
