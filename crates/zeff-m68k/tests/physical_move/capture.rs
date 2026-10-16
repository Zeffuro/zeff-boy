use super::*;

fn assert_operand_memory(
    device: &Device,
    expected: &BTreeMap<u32, u16>,
    plan: &[oracle::Transfer],
) {
    for transfer in plan
        .iter()
        .filter(|transfer| transfer.access.kind == AccessKind::Data)
    {
        assert_eq!(
            device.words.get(&transfer.access.bus_address),
            expected.get(&transfer.access.bus_address)
        );
    }
}

#[test]
fn every_operand_and_refill_phase_preserves_its_literal_fault_snapshot() {
    let mut positions = 0;
    for form in FORMS {
        positions += form.count();
        for failed in 0..form.count() {
            for failure in [Failure::Host, Failure::Boundary, Failure::Unavailable] {
                for supervisor in [false, true] {
                    let value = match form.size {
                        Size::Byte => 0x80,
                        Size::Word => 0x8000,
                        Size::Long => 0x0000_8000,
                    };
                    let (mut cpu, mut bus, initial) =
                        seed(form, registers(supervisor, 31), 3, 2, value, 0x1234_6000);
                    bus.elapsed = [7, 13, 19];
                    bus.failure = Some((failed, failure));
                    let plan = oracle::plan(form, initial, 3, 2, value, 0x7201);
                    let expected_memory = oracle::memory(bus.words.clone(), &plan[..=failed]);
                    let transfer = plan[failed];
                    let captured = oracle::capture(transfer, 2);
                    let clocks = bus.elapsed[..=failed]
                        .iter()
                        .map(|&n| u64::from(n))
                        .sum::<u64>();
                    let result = cpu.step_physical_instruction(&mut bus);
                    assert_eq!(
                        &bus.events[..=failed],
                        oracle::events(&plan[..=failed], &bus.elapsed, 37)
                    );
                    assert_eq!(bus.count, failed + 1);
                    assert_operand_memory(&bus, &expected_memory, &plan);
                    if failure == Failure::Boundary {
                        assert_eq!(
                            result,
                            Ok(PhysicalInstructionStep::Group0 {
                                opcode: form.opcode(3, 2),
                                capture: captured,
                                clocks: clocks + 50
                            })
                        );
                        oracle::frame(&bus, initial.ssp, captured);
                        let mut expected = transfer.registers;
                        expected.ssp -= 14;
                        expected.pc = 0x4321_4000;
                        expected.set_status_register(expected.status_register() | SUPERVISOR);
                        assert_eq!(cpu.registers, expected);
                        assert_eq!(cpu.queued_words(), Some([0x4e71, 0x7201]));
                        assert_eq!(bus.entry_count, 11);
                        assert_eq!(cpu.clock().0, 37 + clocks + 50);
                    } else {
                        let cause = CriticalFault::BusAbort {
                            access: transfer.access,
                            direction: captured.direction,
                        };
                        let expected_error = if failure == Failure::Host {
                            InstructionFault::Entry(EntryFault::HostFailure {
                                access: transfer.access,
                                direction: captured.direction,
                                source: "host",
                            })
                        } else {
                            InstructionFault::TransitionTimingUnavailable {
                                phase: transfer.phase,
                                cause,
                            }
                        };
                        assert_eq!(result, Err(expected_error));
                        assert_eq!(cpu.registers, transfer.registers);
                        assert_eq!(cpu.queued_words(), None);
                        assert_eq!(bus.entry_count, 0);
                        assert_eq!(cpu.clock().0, 37 + clocks);
                        if failure == Failure::Unavailable {
                            assert_eq!(
                                cpu.physical_entry_state(),
                                PhysicalEntryState::FaultUnqualified { cause }
                            );
                            assert_eq!(cpu.physical_inspection(), PhysicalInspection::Unavailable);
                        } else {
                            assert_eq!(cpu.physical_entry_state(), PhysicalEntryState::Idle);
                        }
                        let before = bus.events.clone();
                        assert!(cpu.step_physical_instruction(&mut bus).is_err());
                        assert_eq!(bus.events, before);
                    }
                    assert_eq!(cpu.clock().0, bus.clock);
                }
            }
        }
    }
    assert_eq!(positions, 14);
}

#[test]
fn long_store_flag_stages_distinguish_low_word_from_full_value_for_all_old_ccrs() {
    let form = Form {
        size: Size::Long,
        write: true,
    };
    for value in [0x0001_0000, 0x0000_8000, 0x8000_0001, 0] {
        for old_ccr in 0..32 {
            for supervisor in [false, true] {
                for failed in 0..3 {
                    let (mut cpu, mut bus, initial) = seed(
                        form,
                        registers(supervisor, old_ccr),
                        7,
                        7,
                        value,
                        0x1234_6000,
                    );
                    bus.failure = Some((failed, Failure::Boundary));
                    let plan = oracle::plan(form, initial, 7, 7, value, 0x7201);
                    let captured = oracle::capture(plan[failed], 2);
                    assert_eq!(
                        captured.phase_sr,
                        if failed == 0 {
                            initial.status_register()
                        } else if failed == 1 {
                            oracle::flags(initial.status_register(), Size::Word, value)
                        } else {
                            oracle::flags(initial.status_register(), Size::Long, value)
                        }
                    );
                    assert_eq!(
                        cpu.step_physical_instruction(&mut bus),
                        Ok(PhysicalInstructionStep::Group0 {
                            opcode: form.opcode(7, 7),
                            capture: captured,
                            clocks: (failed as u64 + 1) * 4 + 50
                        })
                    );
                    oracle::frame(&bus, initial.ssp, captured);
                    assert_eq!(cpu.registers.d, initial.d);
                    assert_eq!(cpu.registers.a, initial.a);
                    assert_eq!(cpu.registers.usp, initial.usp);
                    assert_eq!(
                        cpu.registers.status_register(),
                        captured.phase_sr | SUPERVISOR
                    );
                    assert_eq!(cpu.clock().0, bus.clock);
                }
            }
        }
    }
}

#[test]
fn long_load_keeps_the_whole_destination_unchanged_through_both_operand_reads() {
    let form = Form {
        size: Size::Long,
        write: false,
    };
    for failed in 0..3 {
        for value in [0, 0x0001_0000, 0x8000_0001, 0x1234_5678] {
            let (mut cpu, mut bus, initial) =
                seed(form, registers(false, 31), 6, 2, value, 0x1234_6000);
            bus.failure = Some((failed, Failure::Host));
            let result = cpu.step_physical_instruction(&mut bus);
            assert!(matches!(
                result,
                Err(InstructionFault::Entry(EntryFault::HostFailure { .. }))
            ));
            assert_eq!(
                cpu.registers.d[6],
                if failed < 2 { initial.d[6] } else { value }
            );
            assert_eq!(
                cpu.registers.status_register(),
                if failed < 2 {
                    initial.status_register()
                } else {
                    oracle::flags(initial.status_register(), Size::Long, value)
                }
            );
            assert_eq!(bus.count, failed + 1);
            assert_eq!(cpu.clock().0, bus.clock);
        }
    }
}

#[test]
fn secondary_entry_failures_retain_long_store_effects_without_recursive_frames() {
    let form = Form {
        size: Size::Long,
        write: true,
    };
    for failed in 0..11 {
        for failure in [Failure::Host, Failure::Boundary, Failure::Unavailable] {
            let (mut cpu, mut bus, initial) =
                seed(form, registers(false, 31), 3, 2, 0x8000_0001, 0x1234_6000);
            bus.failure = Some((1, Failure::Boundary));
            bus.entry_failure = Some((failed, failure));
            let plan = oracle::plan(form, initial, 3, 2, 0x8000_0001, 0x7201);
            let result = cpu.step_physical_instruction(&mut bus);
            assert!(matches!(result, Err(InstructionFault::Entry(_))));
            assert_eq!(cpu.registers, plan[1].registers);
            assert_eq!(bus.words[&0x0034_6000], 0x8000);
            assert_eq!(bus.words[&0x0034_6002], 1);
            assert_eq!(bus.count, 2);
            assert_eq!(bus.entry_count, failed + 1);
            assert_eq!(cpu.queued_words(), None);
            assert_eq!(
                cpu.clock().0,
                37 + 8 + 4 + 4 * (failed as u64 + 1) + if failed == 10 { 2 } else { 0 }
            );
            assert_eq!(cpu.clock().0, bus.clock);
            if failure != Failure::Host {
                assert_eq!(cpu.physical_inspection(), PhysicalInspection::Unavailable);
                assert!(matches!(
                    cpu.physical_entry_state(),
                    PhysicalEntryState::DoubleFaultHalted { .. }
                ));
            }
            let before = bus.events.clone();
            assert!(cpu.step_physical_instruction(&mut bus).is_err());
            assert_eq!(bus.events, before);
        }
    }
}

#[test]
fn four_odd_operand_frames_match_literal_word_sequences() {
    let cases = [
        (
            Form {
                size: Size::Word,
                write: true,
            },
            7,
            6,
            0x0034_9e9a,
            0xc8fa_6cff,
            0x051f,
            [0x9e9e, 0x0510, 0x0034, 0x3c87, 0x6cff, 0x3c81, 0xc8fa],
        ),
        (
            Form {
                size: Size::Word,
                write: false,
            },
            7,
            2,
            0x0066_098e,
            0x5105_e5ef,
            0x201c,
            [0x0990, 0x201c, 0x0066, 0x3e12, 0xe5ef, 0x3e15, 0x5105],
        ),
        (
            Form {
                size: Size::Long,
                write: true,
            },
            6,
            3,
            0x00da_05b8,
            0xdb92_b483,
            0x230a,
            [0x05bc, 0x230a, 0x00da, 0x2686, 0xb483, 0x2685, 0xdb92],
        ),
        (
            Form {
                size: Size::Long,
                write: false,
            },
            6,
            3,
            0x00d4_2586,
            0xade7_cbf7,
            0x2618,
            [0x2588, 0x2618, 0x00d4, 0x2c13, 0xcbf7, 0x2c15, 0xade7],
        ),
    ];
    for (form, dn, an, pc, address, sr, frame_words) in cases {
        let mut initial = registers(sr & SUPERVISOR != 0, sr & 31);
        initial.pc = pc;
        initial.set_status_register(sr);
        let (mut cpu, mut bus, initial) = seed(form, initial, dn, an, 1, address);
        bus.detection = Some((Failure::Boundary, 8));
        assert!(matches!(
            cpu.step_physical_instruction(&mut bus),
            Ok(PhysicalInstructionStep::Group0 { clocks: 58, .. })
        ));
        let writes: Vec<_> = bus
            .events
            .iter()
            .filter_map(|event| match event {
                Event::EntryWrite(_, value, _) => Some(*value),
                _ => None,
            })
            .collect();
        assert_eq!(writes, frame_words);
        assert_eq!(bus.count, 0);
        assert_eq!(cpu.registers.ssp, initial.ssp - 14);
        assert_eq!(cpu.clock().0, bus.clock);
    }
}
