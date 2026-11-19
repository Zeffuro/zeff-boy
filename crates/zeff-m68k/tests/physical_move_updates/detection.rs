use super::*;

#[test]
fn all_odd_operand_routes_detect_without_aligned_data_and_keep_real_prefixes() {
    let mut routes = 0;
    for form in FORMS.into_iter().filter(|f| f.size != Size::Byte) {
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
                let (mut cpu, mut bus, initial) = seed(
                    form,
                    registers(supervisor, 31),
                    3,
                    2,
                    0x0000_8000,
                    0x1234_6001,
                );
                let memory = bus.words.clone();
                let plan = oracle::plan(form, initial, 3, 2, 0x0000_8000, 0x7201);
                let at = plan
                    .iter()
                    .position(|t| t.access.kind == AccessKind::Data)
                    .unwrap();
                let transfer = plan[at];
                let capture = oracle::capture(transfer, 3);
                let elapsed = detection.map_or(0, |(_, elapsed)| elapsed);
                let clocks = u64::from(form.idle()) + at as u64 * 4 + u64::from(elapsed);
                let mut expected_events = oracle::events(form, &plan[..at], &bus.elapsed, 37);
                let detection_start = 37 + u64::from(form.idle()) + at as u64 * 4;
                let result = if let Some(report) = detection {
                    bus.detection = Some(report);
                    expected_events.push(Event::Detect(
                        transfer.access,
                        capture.direction,
                        detection_start,
                    ));
                    cpu.step_physical_instruction(&mut bus)
                } else {
                    cpu.step_physical_instruction(&mut DefaultDetection(&mut bus))
                };
                assert_eq!(bus.count, at);
                assert!(!bus.events.iter().any(|event| matches!(event,
                    Event::Read(access, _) | Event::Write(access, _, _) if access.kind == AccessKind::Data)));
                let cause = CriticalFault::AddressError {
                    access: transfer.access,
                    direction: capture.direction,
                };
                if detection.is_some_and(|(failure, _)| failure == Failure::Boundary) {
                    assert_eq!(
                        result,
                        Ok(PhysicalInstructionStep::Group0 {
                            opcode: form.opcode(3, 2),
                            capture,
                            clocks: clocks + 50
                        })
                    );
                    expected_events.extend(oracle::entry_events(
                        transfer.registers.ssp,
                        capture,
                        37 + clocks,
                    ));
                    oracle::frame(&bus, transfer.registers.ssp, capture, 37 + clocks);
                    assert_eq!(cpu.registers, oracle::entered(transfer.registers));
                    assert_eq!(
                        bus.words,
                        oracle::entry_memory(memory, transfer.registers.ssp, capture)
                    );
                    assert_eq!(cpu.clock().0, 37 + clocks + 50);
                } else {
                    assert_eq!(
                        result,
                        Err(
                            if detection.is_some_and(|(failure, _)| failure == Failure::Host) {
                                InstructionFault::Entry(EntryFault::HostFailure {
                                    access: transfer.access,
                                    direction: capture.direction,
                                    source: "host",
                                })
                            } else {
                                InstructionFault::TransitionTimingUnavailable {
                                    phase: transfer.phase,
                                    cause,
                                }
                            }
                        )
                    );
                    assert_eq!(cpu.registers, transfer.registers);
                    assert_eq!(bus.words, memory);
                    assert_eq!(bus.entry_count, 0);
                    assert_eq!(cpu.clock().0, 37 + clocks);
                    assert_eq!(cpu.queued_words(), None);
                    if !detection.is_some_and(|(failure, _)| failure == Failure::Host) {
                        assert_eq!(
                            cpu.physical_entry_state(),
                            PhysicalEntryState::FaultUnqualified { cause }
                        );
                        assert_eq!(cpu.physical_inspection(), PhysicalInspection::Unavailable);
                    }
                    guarded(
                        &mut cpu,
                        &mut bus,
                        if detection.is_some_and(|(failure, _)| failure == Failure::Host) {
                            TimingFault::Frozen
                        } else {
                            TimingFault::PhysicalFaultUnqualified
                        },
                    );
                }
                assert_eq!(bus.events, expected_events);
                assert_eq!(cpu.clock().0, bus.clock);
            }
        }
    }
    assert_eq!(routes, 8);
}

#[test]
fn user_stack_odd_operands_keep_updated_usp_and_use_the_even_supervisor_stack() {
    for form in FORMS.into_iter().filter(|f| f.size != Size::Byte) {
        let (mut cpu, mut bus, initial) =
            seed(form, registers(false, 31), 6, 7, 0x0001_0000, 0x1234_6001);
        bus.detection = Some((Failure::Boundary, 8));
        let plan = oracle::plan(form, initial, 6, 7, 0x0001_0000, 0x7201);
        let at = plan
            .iter()
            .position(|t| t.access.kind == AccessKind::Data)
            .unwrap();
        let clocks = u64::from(form.idle()) + at as u64 * 4 + 8;
        let capture = oracle::capture(plan[at], 3);
        assert_eq!(
            cpu.step_physical_instruction(&mut bus),
            Ok(PhysicalInstructionStep::Group0 {
                opcode: form.opcode(6, 7),
                capture,
                clocks: clocks + 50
            })
        );
        assert_eq!(cpu.registers, oracle::entered(plan[at].registers));
        assert_eq!(cpu.registers.usp, plan[at].registers.usp);
        oracle::frame(&bus, initial.ssp, capture, 37 + clocks);
        assert_eq!(cpu.clock().0, bus.clock);
    }
}

#[test]
fn odd_supervisor_stack_operand_faults_halt_before_any_aligned_stack_write() {
    for form in FORMS.into_iter().filter(|f| f.size != Size::Byte) {
        let (mut cpu, mut bus, initial) =
            seed(form, registers(true, 31), 6, 7, 0x0001_0000, 0x1234_6001);
        bus.detection = Some((Failure::Boundary, 8));
        let plan = oracle::plan(form, initial, 6, 7, 0x0001_0000, 0x7201);
        let at = plan
            .iter()
            .position(|t| t.access.kind == AccessKind::Data)
            .unwrap();
        let result = cpu.step_physical_instruction(&mut bus);
        assert!(matches!(
            result,
            Err(InstructionFault::Entry(EntryFault::TimingUnavailable(_)))
        ));
        assert!(matches!(
            cpu.physical_entry_state(),
            PhysicalEntryState::DoubleFaultHalted { .. }
        ));
        assert_eq!(cpu.physical_inspection(), PhysicalInspection::Unavailable);
        assert_eq!(cpu.registers, plan[at].registers);
        assert_eq!(cpu.queued_words(), None);
        assert_eq!(bus.count, at);
        assert_eq!(bus.entry_count, 0);
        let mut access = oracle::access(
            plan[at].registers.ssp.wrapping_sub(2),
            Size::Word,
            true,
            true,
        );
        access.kind = AccessKind::Stack;
        let clock = 37 + u64::from(form.idle()) + at as u64 * 4 + 8;
        assert_eq!(
            &bus.events[bus.events.len() - 2..],
            &[
                Event::Idle(clock, 4),
                Event::EntryDetect(access, Direction::Write, clock + 4)
            ]
        );
        assert_eq!(cpu.clock().0, clock + 4);
        assert_eq!(cpu.clock().0, bus.clock);
        guarded(&mut cpu, &mut bus, TimingFault::DoubleFaultHalted);
    }
}

#[test]
fn seven_known_odd_frames_match_independent_literal_write_sequences() {
    let cases = [
        (
            Form {
                size: Size::Word,
                route: Route::PostLoad,
            },
            7,
            4,
            0x0090_0cc4,
            0x62e2_b461,
            0x271b,
            1,
            [0x0cc6, 0x271b, 0x0090, 0x3e1c, 0xb461, 0x3e15, 0x62e2],
            58,
        ),
        (
            Form {
                size: Size::Word,
                route: Route::PreLoad,
            },
            1,
            2,
            0x006b_526c,
            0x2859_1ac1,
            0x0018,
            1,
            [0x5270, 0x0018, 0x006b, 0x3222, 0x1abf, 0x3231, 0x2859],
            60,
        ),
        (
            Form {
                size: Size::Word,
                route: Route::PostStore,
            },
            6,
            0,
            0x0040_61c4,
            0x947b_6ce5,
            0x2518,
            0x8000,
            [0x61c8, 0x2518, 0x0040, 0x30c6, 0x6ce5, 0x30c5, 0x947b],
            58,
        ),
        (
            Form {
                size: Size::Long,
                route: Route::PostLoad,
            },
            7,
            1,
            0x00f9_ca5a,
            0xbf76_6783,
            0x2618,
            1,
            [0xca5c, 0x2618, 0x00f9, 0x2e19, 0x6783, 0x2e15, 0xbf76],
            58,
        ),
        (
            Form {
                size: Size::Long,
                route: Route::PreLoad,
            },
            2,
            6,
            0x0094_6e24,
            0xbc99_e7b1,
            0x061f,
            1,
            [0x6e26, 0x061f, 0x0094, 0x2426, 0xe7ad, 0x2431, 0xbc99],
            60,
        ),
        (
            Form {
                size: Size::Long,
                route: Route::PostStore,
            },
            5,
            2,
            0x0014_85ee,
            0x8757_766d,
            0x2515,
            1,
            [0x85f2, 0x2515, 0x0014, 0x24c5, 0x766d, 0x24c5, 0x8757],
            58,
        ),
        (
            Form {
                size: Size::Long,
                route: Route::PreStore,
            },
            0,
            6,
            0x00ab_1604,
            0x6174_6429,
            0x0718,
            0x8000_0001,
            [0x1608, 0x0718, 0x00ab, 0x2d00, 0x6427, 0x2d01, 0x6174],
            62,
        ),
    ];
    for (form, dn, an, pc, address, sr, value, words, clocks) in cases {
        let mut registers = registers(sr & SUPERVISOR != 0, sr & 31);
        registers.pc = pc;
        registers.set_status_register(sr);
        let (mut cpu, mut bus, _) = seed(form, registers, dn, an, value, address);
        bus.detection = Some((Failure::Boundary, 8));
        assert!(
            matches!(cpu.step_physical_instruction(&mut bus), Ok(PhysicalInstructionStep::Group0 { clocks: elapsed, .. }) if elapsed == clocks)
        );
        let actual: Vec<_> = bus
            .events
            .iter()
            .filter_map(|event| match event {
                Event::EntryWrite(_, value, _) => Some(*value),
                _ => None,
            })
            .collect();
        assert_eq!(actual, words);
        assert_eq!(cpu.clock().0, bus.clock);
    }
}
