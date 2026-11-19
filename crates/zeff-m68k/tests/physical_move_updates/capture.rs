use super::*;

#[test]
fn all_aligned_phases_capture_exact_partial_state_and_retain_consumed_effects() {
    let mut positions = 0;
    for form in FORMS {
        positions += form.count();
        for failed in 0..form.count() {
            for failure in [Failure::Host, Failure::Boundary, Failure::Unavailable] {
                for supervisor in [false, true] {
                    for an in [2, 7] {
                        for ccr in [15, 31] {
                            let value = match form.size {
                                Size::Byte => 0x80,
                                Size::Word => 0x8000,
                                Size::Long => 0x0000_8000,
                            };
                            let (mut cpu, mut bus, initial) =
                                seed(form, registers(supervisor, ccr), 3, an, value, 0x1234_6000);
                            bus.elapsed = [7, 13, 19];
                            bus.failure = Some((failed, failure));
                            let plan = oracle::plan(form, initial, 3, an, value, 0x7201);
                            let transfer = plan[failed];
                            let captured = oracle::capture(transfer, 2);
                            let clocks = u64::from(form.idle())
                                + bus.elapsed[..=failed]
                                    .iter()
                                    .map(|&n| u64::from(n))
                                    .sum::<u64>();
                            let mut memory = oracle::memory(bus.words.clone(), &plan[..=failed]);
                            let mut events =
                                oracle::events(form, &plan[..=failed], &bus.elapsed, 37);
                            let result = cpu.step_physical_instruction(&mut bus);
                            assert_eq!(bus.count, failed + 1);
                            if failure == Failure::Boundary {
                                assert_eq!(
                                    result,
                                    Ok(PhysicalInstructionStep::Group0 {
                                        opcode: form.opcode(3, an),
                                        capture: captured,
                                        clocks: clocks + 50,
                                    })
                                );
                                let sp = transfer.registers.ssp;
                                memory = oracle::entry_memory(memory, sp, captured);
                                events.extend(oracle::entry_events(sp, captured, 37 + clocks));
                                oracle::frame(&bus, sp, captured, 37 + clocks);
                                assert_eq!(cpu.registers, oracle::entered(transfer.registers));
                                assert_eq!(cpu.queued_words(), Some([0x4e71, 0x7201]));
                                assert_eq!(bus.entry_count, 11);
                                assert!(matches!(
                                    cpu.physical_entry_state(),
                                    PhysicalEntryState::Critical(_)
                                ));
                                assert_eq!(
                                    cpu.physical_inspection(),
                                    PhysicalInspection::DiagnosticCompleteValues
                                );
                                assert_eq!(cpu.clock().0, 37 + clocks + 50);
                            } else {
                                let cause = CriticalFault::BusAbort {
                                    access: transfer.access,
                                    direction: captured.direction,
                                };
                                assert_eq!(
                                    result,
                                    Err(if failure == Failure::Host {
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
                                    })
                                );
                                assert_eq!(cpu.registers, transfer.registers);
                                assert_eq!(cpu.queued_words(), None);
                                assert_eq!(bus.entry_count, 0);
                                assert_eq!(cpu.clock().0, 37 + clocks);
                                if failure == Failure::Unavailable {
                                    assert_eq!(
                                        cpu.physical_entry_state(),
                                        PhysicalEntryState::FaultUnqualified { cause }
                                    );
                                    assert_eq!(
                                        cpu.physical_inspection(),
                                        PhysicalInspection::Unavailable
                                    );
                                } else {
                                    assert_eq!(
                                        cpu.physical_entry_state(),
                                        PhysicalEntryState::Idle
                                    );
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
                            assert_eq!(bus.words, memory);
                            assert_eq!(bus.events, events);
                            assert_eq!(cpu.clock().0, bus.clock);
                        }
                    }
                }
            }
        }
    }
    assert_eq!(positions, 28);
}

#[test]
fn long_store_stages_distinguish_low_word_full_value_and_address_updates() {
    for route in [Route::PostStore, Route::PreStore] {
        let form = Form {
            size: Size::Long,
            route,
        };
        for value in [0x0001_0000, 0x0000_8000, 0x8000_0001, 0] {
            for ccr in 0..32 {
                for supervisor in [false, true] {
                    for failed in 0..3 {
                        let (mut cpu, mut bus, initial) =
                            seed(form, registers(supervisor, ccr), 7, 7, value, 0x1234_6000);
                        bus.failure = Some((failed, Failure::Boundary));
                        let plan = oracle::plan(form, initial, 7, 7, value, 0x7201);
                        let captured = oracle::capture(plan[failed], 2);
                        let expected_sr = match (route, failed) {
                            (Route::PostStore, 0) => initial.status_register(),
                            (Route::PostStore, 1) | (Route::PreStore, 0) => {
                                oracle::flags(initial.status_register(), Size::Word, value)
                            }
                            _ => oracle::flags(initial.status_register(), Size::Long, value),
                        };
                        assert_eq!(captured.phase_sr, expected_sr);
                        assert_eq!(
                            cpu.step_physical_instruction(&mut bus),
                            Ok(PhysicalInstructionStep::Group0 {
                                opcode: form.opcode(7, 7),
                                capture: captured,
                                clocks: (failed as u64 + 1) * 4 + 50,
                            })
                        );
                        oracle::frame(
                            &bus,
                            plan[failed].registers.ssp,
                            captured,
                            37 + (failed as u64 + 1) * 4,
                        );
                        assert_eq!(cpu.registers, oracle::entered(plan[failed].registers));
                        assert_eq!(cpu.clock().0, bus.clock);
                    }
                }
            }
        }
    }
}

#[test]
fn long_loads_never_publish_half_data_even_after_the_address_update() {
    for route in [Route::PostLoad, Route::PreLoad] {
        let form = Form {
            size: Size::Long,
            route,
        };
        for value in [0, 0x0001_0000, 0x8000_0001, 0x1234_5678] {
            for failed in 0..3 {
                let (mut cpu, mut bus, initial) =
                    seed(form, registers(false, 31), 6, 2, value, 0x1234_6000);
                bus.failure = Some((failed, Failure::Host));
                assert!(matches!(
                    cpu.step_physical_instruction(&mut bus),
                    Err(InstructionFault::Entry(EntryFault::HostFailure { .. }))
                ));
                assert_eq!(
                    cpu.registers,
                    oracle::plan(form, initial, 6, 2, value, 0x7201)[failed].registers
                );
                assert_eq!(
                    cpu.registers.d[6],
                    if failed < 2 { initial.d[6] } else { value }
                );
                assert_eq!(cpu.clock().0, bus.clock);
            }
        }
    }
}

#[test]
fn critical_entry_failures_keep_predecrement_fetch_and_writes_without_retry() {
    let form = Form {
        size: Size::Long,
        route: Route::PreStore,
    };
    for failed in 0..11 {
        for failure in [Failure::Host, Failure::Boundary, Failure::Unavailable] {
            let (mut cpu, mut bus, initial) =
                seed(form, registers(true, 31), 3, 7, 0x8000_0001, 0x1234_6000);
            bus.failure = Some((2, Failure::Boundary));
            bus.entry_failure = Some((failed, failure));
            let plan = oracle::plan(form, initial, 3, 7, 0x8000_0001, 0x7201);
            let memory = oracle::memory(bus.words.clone(), &plan);
            let result = cpu.step_physical_instruction(&mut bus);
            assert!(matches!(result, Err(InstructionFault::Entry(_))));
            assert_eq!(cpu.registers, plan[2].registers);
            oracle::assert_operand_memory(&bus, &memory, &plan);
            assert_eq!(bus.events[0], Event::Read(plan[0].access, 37));
            assert_eq!(bus.count, 3);
            assert_eq!(bus.entry_count, failed + 1);
            assert_eq!(cpu.queued_words(), None);
            assert_eq!(
                cpu.clock().0,
                37 + 12 + 4 + 4 * (failed as u64 + 1) + if failed == 10 { 2 } else { 0 }
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
