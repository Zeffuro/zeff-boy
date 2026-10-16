use super::*;

pub(super) fn odd(mut case: Case) -> Case {
    if case.form.family == Family::AbsoluteLong {
        case.e4 |= 1;
    } else {
        case.e0 |= 1;
    }
    case
}

#[test]
fn all_20_odd_routes_preserve_extensions_without_aligned_operand_callbacks() {
    let mut routes = 0;
    for form in forms()
        .into_iter()
        .filter(|f| f.memory() && f.size != Size::Byte)
    {
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
                case.an = 7;
                let (mut cpu, mut bus, case) = odd(case).seed();
                let memory = bus.words.clone();
                let plan = oracle::plan(case, &memory);
                let at = plan
                    .iter()
                    .position(|t| t.access.kind == AccessKind::Data)
                    .unwrap();
                let t = plan[at];
                let capture = oracle::capture(t, 3);
                let prefix = oracle::clocks(case, &bus.elapsed, at);
                let clocks = prefix + u64::from(detection.map_or(0, |(_, elapsed)| elapsed));
                let mut events = oracle::events(case, &plan[..at], &bus.elapsed);
                let result = if let Some(report) = detection {
                    bus.detection = Some(report);
                    events.push(Event::Detect(
                        t.access,
                        capture.direction,
                        case.start + prefix,
                    ));
                    cpu.step_physical_instruction(&mut bus)
                } else {
                    cpu.step_physical_instruction(&mut DefaultDetection(&mut bus))
                };
                assert_eq!(bus.count, at);
                assert!(!bus.events.iter().any(|e| matches!(e, Event::Read(a, _) | Event::Write(a, _, _) if a.kind == AccessKind::Data)));
                if detection.is_some_and(|(f, _)| f == Failure::Boundary) {
                    assert_eq!(
                        result,
                        Ok(PhysicalInstructionStep::Group0 {
                            opcode: form.opcode(case.dn, case.an),
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
                    assert_eq!(bus.words, oracle::entry_memory(memory, case.r.ssp, capture));
                    oracle::frame(&bus, case.r.ssp, capture, case.start + clocks);
                    assert_eq!(cpu.clock().0, case.start + clocks + 50);
                } else {
                    let cause = CriticalFault::AddressError {
                        access: t.access,
                        direction: capture.direction,
                    };
                    let host = detection.is_some_and(|(f, _)| f == Failure::Host);
                    assert_eq!(
                        result,
                        Err(if host {
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
    assert_eq!(routes, 20);
}

#[test]
fn even_banked_stack_with_odd_a7_displacement_or_index_enters_an_ordinary_frame() {
    for form in forms()
        .into_iter()
        .filter(|f| f.uses_an() && f.size != Size::Byte)
    {
        for supervisor in [false, true] {
            let mut case = Case::new(form);
            case.r = registers(supervisor, 31);
            case.an = 7;
            let (mut cpu, mut bus, case) = odd(case).seed();
            bus.detection = Some((Failure::Boundary, 8));
            let plan = oracle::plan(case, &bus.words);
            let at = plan
                .iter()
                .position(|t| t.access.kind == AccessKind::Data)
                .unwrap();
            let capture = oracle::capture(plan[at], 3);
            let clocks = oracle::clocks(case, &bus.elapsed, at) + 8;
            assert_eq!(
                cpu.step_physical_instruction(&mut bus),
                Ok(PhysicalInstructionStep::Group0 {
                    opcode: form.opcode(case.dn, 7),
                    capture,
                    clocks: clocks + 50
                })
            );
            assert_eq!(cpu.registers, oracle::entered(plan[at].registers));
            assert_eq!(cpu.registers.usp, case.r.usp);
            oracle::frame(&bus, case.r.ssp, capture, case.start + clocks);
            assert_eq!(cpu.clock().0, bus.clock);
        }
    }
}

#[test]
fn odd_supervisor_stack_is_a_secondary_halt_after_the_operand_detection_prefix() {
    for form in forms()
        .into_iter()
        .filter(|f| f.memory() && f.size != Size::Byte)
    {
        let mut case = odd(Case::new(form));
        case.r.ssp |= 1;
        let (mut cpu, mut bus, case) = case.seed();
        let plan = oracle::plan(case, &bus.words);
        let at = plan
            .iter()
            .position(|t| t.access.kind == AccessKind::Data)
            .unwrap();
        bus.detection = Some((Failure::Boundary, 8));
        let memory = bus.words.clone();
        assert!(matches!(
            cpu.step_physical_instruction(&mut bus),
            Err(InstructionFault::Entry(EntryFault::TimingUnavailable(_)))
        ));
        let clocks = oracle::clocks(case, &bus.elapsed, at) + 8;
        let mut events = oracle::events(case, &plan[..at], &bus.elapsed);
        events.push(Event::Detect(
            plan[at].access,
            if form.write {
                Direction::Write
            } else {
                Direction::Read
            },
            case.start + clocks - 8,
        ));
        events.push(Event::Idle(case.start + clocks, 4));
        events.push(Event::EntryDetect(
            oracle::access(case.r.ssp - 2, Size::Word, true, false, AccessKind::Stack),
            Direction::Write,
            case.start + clocks + 4,
        ));
        assert_eq!(bus.events, events);
        assert_eq!(bus.words, memory);
        assert_eq!(cpu.registers, plan[at].registers);
        assert_eq!(bus.entry_count, 0);
        assert_eq!(cpu.queued_words(), None);
        assert!(matches!(
            cpu.physical_entry_state(),
            PhysicalEntryState::DoubleFaultHalted { .. }
        ));
        assert_eq!(cpu.physical_inspection(), PhysicalInspection::Unavailable);
        assert_eq!(cpu.clock().0, case.start + clocks + 4);
        assert_eq!(cpu.clock().0, bus.clock);
        guarded(&mut cpu, &mut bus, TimingFault::DoubleFaultHalted);
    }
}
