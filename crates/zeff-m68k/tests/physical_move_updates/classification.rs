use super::*;

fn ordinary_move(word: u16) -> bool {
    let family = word >> 12;
    if !matches!(family, 1..=3) {
        return false;
    }
    let source_mode = (word >> 3) & 7;
    let destination_mode = (word >> 6) & 7;
    (source_mode != 7 || word & 7 <= 4)
        && (destination_mode != 7 || (word >> 9) & 7 <= 1)
        && (family != 1 || (source_mode != 1 && destination_mode != 1))
}

fn promoted(word: u16, known: bool) {
    let form = Form {
        size: Size::Word,
        route: Route::PreStore,
    };
    let (_, mut bus, initial) = seed(form, registers(false, 31), 3, 2, 0x8000, 0x1234_6000);
    let mut cpu =
        PrefetchCpu::from_prefetched(initial, [form.opcode(3, 2), word], CpuClocks(37)).unwrap();
    bus.failure = Some((1, Failure::Boundary));
    let plan = oracle::plan(form, initial, 3, 2, 0x8000, word);
    let captured = oracle::capture(plan[1], 2);
    let result = cpu.step_physical_instruction(&mut bus);
    assert_eq!(bus.count, 2);
    assert_eq!(bus.words[&plan[1].access.bus_address], 0x8000);
    if known {
        assert_eq!(
            result,
            Ok(PhysicalInstructionStep::Group0 {
                opcode: form.opcode(3, 2),
                capture: captured,
                clocks: 58
            })
        );
        assert_eq!(bus.entry_count, 11);
        assert_eq!(cpu.registers, oracle::entered(plan[1].registers));
        assert_eq!(cpu.clock().0, 95);
    } else {
        let cause = CriticalFault::BusAbort {
            access: plan[1].access,
            direction: Direction::Write,
        };
        assert_eq!(
            result,
            Err(InstructionFault::CaptureUnavailable {
                phase: InstructionPhase::OperandWriteHigh,
                cause
            })
        );
        assert_eq!(cpu.registers, plan[1].registers);
        assert_eq!(
            cpu.physical_entry_state(),
            PhysicalEntryState::FaultUnqualified { cause }
        );
        assert_eq!(cpu.physical_inspection(), PhysicalInspection::Unavailable);
        assert_eq!(cpu.queued_words(), None);
        assert_eq!(bus.entry_count, 0);
        assert_eq!(cpu.clock().0, 45);
        guarded(&mut cpu, &mut bus, TimingFault::PhysicalFaultUnqualified);
    }
    assert_eq!(cpu.clock().0, bus.clock);
}

#[test]
fn complete_move_movea_and_branch_class_masks_are_separate_from_execution() {
    let mut known_moves = 0;
    for family in [1, 2, 3] {
        for bits in 0..4096 {
            let word = family << 12 | bits;
            let known = ordinary_move(word);
            promoted(word, known);
            known_moves += usize::from(known);
        }
    }
    assert_eq!(known_moves, 9726);
    for bits in 0..4096 {
        promoted(0x6000 | bits, true);
    }
    for word in [0x4e71, 0x7000, 0x7eff, 0x2970, 0x6b5f, 0x8aa6, 0x9489] {
        promoted(word, true);
    }
    for word in [
        0x1008, 0x1040, 0x103d, 0x103e, 0x103f, 0x15c0, 0x17c0, 0x4e70, 0x4afc, 0xa000, 0xf000,
        0x80bd,
    ] {
        promoted(word, false);
    }
    for opcode in [
        0x0640, 0x6b5f, 0x0440, 0x0240, 0x0040, 0x0a40, 0x1008, 0x15c0,
    ] {
        let initial = registers(false, 31);
        let mut cpu =
            PrefetchCpu::from_prefetched(initial, [opcode, 0x7201], CpuClocks(37)).unwrap();
        let mut bus = Device::new(37);
        let before = cpu.clone();
        assert_eq!(
            cpu.step_physical_instruction(&mut bus),
            Err(InstructionFault::Execution(
                TimingFault::UnsupportedInstruction {
                    pc: initial.pc,
                    opcode
                }
            ))
        );
        assert_eq!(cpu, before);
        assert!(bus.events.is_empty());
        assert_eq!(bus.clock, 37);
    }
}

#[test]
fn unknown_queued_words_only_block_the_phase_that_promoted_them() {
    for size in [Size::Byte, Size::Word, Size::Long] {
        let form = Form {
            size,
            route: Route::PreStore,
        };
        for failed in 0..form.count() {
            for physical in [false, true] {
                let (_, mut bus, initial) =
                    seed(form, registers(false, 31), 3, 2, 0x8000_0001, 0x1234_6000);
                let mut cpu = PrefetchCpu::from_prefetched(
                    initial,
                    [form.opcode(3, 2), 0x4e70],
                    CpuClocks(37),
                )
                .unwrap();
                if physical {
                    bus.failure = Some((failed, Failure::Boundary));
                }
                let plan = oracle::plan(form, initial, 3, 2, 0x8000_0001, 0x4e70);
                let memory = oracle::memory(
                    bus.words.clone(),
                    &plan[..if physical { failed + 1 } else { form.count() }],
                );
                let next = oracle::fetched(bus.words.clone(), &plan);
                let result = cpu.step_physical_instruction(&mut bus);
                if !physical {
                    assert!(matches!(
                        result,
                        Ok(PhysicalInstructionStep::Instruction(_))
                    ));
                    let mut expected = plan.last().unwrap().registers;
                    expected.pc = initial.pc + 2;
                    assert_eq!(cpu.registers, expected);
                    assert_eq!(cpu.queued_words(), Some([0x4e70, next]));
                    assert_eq!(bus.words, memory);
                } else if failed + 1 == form.count() {
                    let cause = CriticalFault::BusAbort {
                        access: plan[failed].access,
                        direction: Direction::Write,
                    };
                    assert_eq!(
                        result,
                        Err(InstructionFault::CaptureUnavailable {
                            phase: plan[failed].phase,
                            cause
                        })
                    );
                    assert_eq!(cpu.registers, plan[failed].registers);
                    assert_eq!(bus.words, memory);
                    assert_eq!(bus.entry_count, 0);
                    guarded(&mut cpu, &mut bus, TimingFault::PhysicalFaultUnqualified);
                } else {
                    let capture = oracle::capture(plan[failed], 2);
                    assert_eq!(capture.ird, form.opcode(3, 2));
                    assert_eq!(
                        result,
                        Ok(PhysicalInstructionStep::Group0 {
                            opcode: form.opcode(3, 2),
                            capture,
                            clocks: (failed as u64 + 1) * 4 + 50
                        })
                    );
                    oracle::frame(&bus, initial.ssp, capture, 37 + (failed as u64 + 1) * 4);
                }
                assert_eq!(cpu.clock().0, bus.clock);
            }
        }
    }
}

#[test]
fn promoted_literal_predecrement_preserves_captured_opcode_and_complete_frame() {
    let form = Form {
        size: Size::Word,
        route: Route::PreStore,
    };
    let mut initial = registers(false, 24);
    initial.pc = 0x00d2_08ea;
    initial.set_status_register(0x0218);
    let (_, mut bus, initial) = seed(form, initial, 6, 3, 0x8000, 0xfe89_250b);
    let mut cpu = PrefetchCpu::from_prefetched(initial, [0x3706, 0x8aa6], CpuClocks(37)).unwrap();
    bus.detection = Some((Failure::Boundary, 8));
    let memory = bus.words.clone();
    let plan = oracle::plan(form, initial, 6, 3, 0x8000, 0x8aa6);
    let capture = oracle::capture(plan[1], 3);
    assert_eq!(
        cpu.step_physical_instruction(&mut bus),
        Ok(PhysicalInstructionStep::Group0 {
            opcode: 0x3706,
            capture,
            clocks: 62,
        })
    );
    assert_eq!(cpu.registers, oracle::entered(plan[1].registers));
    assert_eq!(cpu.registers.a[3], 0xfe89_2509);
    assert_eq!(bus.events, {
        let mut events = vec![
            Event::Read(plan[0].access, 37),
            Event::Detect(plan[1].access, Direction::Write, 41),
        ];
        events.extend(oracle::entry_events(initial.ssp, capture, 49));
        events
    });
    assert_eq!(bus.count, 1);
    assert_eq!(bus.entry_count, 11);
    oracle::frame(&bus, initial.ssp, capture, 49);
    assert_eq!(cpu.clock().0, 99);
    assert_eq!(cpu.clock().0, bus.clock);
    assert_eq!(
        bus.words,
        oracle::entry_memory(memory, initial.ssp, capture)
    );
    assert_eq!(cpu.queued_words(), Some([0x4e71, 0x7201]));
}

#[test]
fn last_predecrement_write_classifies_original_q_after_an_unrelated_actual_fetch() {
    for size in [Size::Byte, Size::Word, Size::Long] {
        for queued in [0x2970, 0x6b5f] {
            let form = Form {
                size,
                route: Route::PreStore,
            };
            let (_, mut bus, initial) =
                seed(form, registers(false, 31), 3, 2, 0x8000_0001, 0x1234_6000);
            let mut cpu =
                PrefetchCpu::from_prefetched(initial, [form.opcode(3, 2), queued], CpuClocks(37))
                    .unwrap();
            bus.word(initial.pc + 4, 0x4e70);
            let last = form.count() - 1;
            bus.failure = Some((last, Failure::Boundary));
            let plan = oracle::plan(form, initial, 3, 2, 0x8000_0001, queued);
            let captured = oracle::capture(plan[last], 2);
            assert_eq!(captured.ird, queued);
            assert_eq!(
                cpu.step_physical_instruction(&mut bus),
                Ok(PhysicalInstructionStep::Group0 {
                    opcode: form.opcode(3, 2),
                    capture: captured,
                    clocks: form.count() as u64 * 4 + 50,
                })
            );
            assert_eq!(bus.events[0], Event::Read(plan[0].access, 37));
            assert_eq!(bus.count, form.count());
            oracle::frame(&bus, initial.ssp, captured, 37 + form.count() as u64 * 4);
            assert_eq!(cpu.clock().0, bus.clock);
        }
    }
}
