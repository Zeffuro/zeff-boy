use super::*;

fn guarded(cpu: &mut PrefetchCpu, bus: &mut Device, fault: TimingFault<&'static str>) {
    let before = bus.events.clone();
    let clock = cpu.clock();
    cpu.interrupt_input().set_level(7);
    assert_eq!(cpu.prime(bus), Err(fault.clone()));
    assert_eq!(cpu.step_instruction(bus), Err(fault.clone()));
    assert_eq!(cpu.step(bus), Err(fault.clone()));
    assert_eq!(cpu.step_boundary(bus), Err(fault.clone()));
    assert_eq!(cpu.service_pending_trace(bus), Err(fault.clone()));
    assert_eq!(
        cpu.step_physical_instruction(bus),
        Err(InstructionFault::Execution(fault))
    );
    assert_eq!(cpu.clock(), clock);
    assert_eq!(bus.events, before);
}

#[test]
fn odd_word_and_long_operands_detect_direction_without_touching_aligned_data() {
    for form in FORMS.into_iter().filter(|form| form.size != Size::Byte) {
        for detection in [
            None,
            Some(Failure::Complete),
            Some(Failure::Host),
            Some(Failure::Boundary),
        ] {
            let (mut cpu, mut bus, initial) =
                seed(form, registers(false, 31), 3, 2, 0x0000_8000, 0x1234_6001);
            bus.detection = detection.map(|failure| (failure, 17));
            let memory = bus.words.clone();
            let plan = oracle::plan(form, initial, 3, 2, 0x0000_8000, 0x7201);
            let transfer = plan[0];
            let captured = oracle::capture(transfer, 3);
            let cause = CriticalFault::AddressError {
                access: transfer.access,
                direction: captured.direction,
            };
            let result = cpu.step_physical_instruction(&mut bus);
            assert_eq!(bus.count, 0);
            assert!(
                !bus.events
                    .iter()
                    .any(|event| matches!(event, Event::Read(..) | Event::Write(..)))
            );
            assert_eq!(
                bus.words[&transfer.access.bus_address],
                memory[&transfer.access.bus_address]
            );
            if detection == Some(Failure::Boundary) {
                assert_eq!(
                    result,
                    Ok(PhysicalInstructionStep::Group0 {
                        opcode: form.opcode(3, 2),
                        capture: captured,
                        clocks: 67
                    })
                );
                oracle::frame(&bus, initial.ssp, captured);
                let mut expected = transfer.registers;
                expected.ssp -= 14;
                expected.pc = 0x4321_4000;
                expected.set_status_register(expected.status_register() | SUPERVISOR);
                assert_eq!(cpu.registers, expected);
                assert_eq!(cpu.clock(), CpuClocks(104));
            } else if detection == Some(Failure::Host) {
                assert_eq!(
                    result,
                    Err(InstructionFault::Entry(EntryFault::HostFailure {
                        access: transfer.access,
                        direction: captured.direction,
                        source: "host"
                    }))
                );
                assert_eq!(cpu.registers, transfer.registers);
                assert_eq!(cpu.clock(), CpuClocks(54));
                guarded(&mut cpu, &mut bus, TimingFault::Frozen);
            } else {
                assert_eq!(
                    result,
                    Err(InstructionFault::TransitionTimingUnavailable {
                        phase: transfer.phase,
                        cause
                    })
                );
                assert_eq!(cpu.registers, transfer.registers);
                assert_eq!(cpu.clock().0, 37 + if detection.is_some() { 17 } else { 0 });
                assert_eq!(
                    cpu.physical_entry_state(),
                    PhysicalEntryState::FaultUnqualified { cause }
                );
                assert_eq!(cpu.physical_inspection(), PhysicalInspection::Unavailable);
                guarded(&mut cpu, &mut bus, TimingFault::PhysicalFaultUnqualified);
            }
            if detection.is_some() {
                assert_eq!(
                    bus.events[0],
                    Event::Detect(transfer.access, captured.direction, 37)
                );
            } else {
                assert!(bus.events.is_empty());
            }
            assert_eq!(cpu.clock().0, bus.clock);
        }
    }
}

#[test]
fn unknown_promoted_refill_blocks_after_consumption_but_any_word_may_complete() {
    for form in FORMS {
        for failure in [false, true] {
            let (_, mut bus, initial) =
                seed(form, registers(false, 31), 3, 2, 0x1234_807f, 0x1234_6000);
            let mut cpu =
                PrefetchCpu::from_prefetched(initial, [form.opcode(3, 2), 0x4e70], CpuClocks(37))
                    .unwrap();
            if failure {
                bus.failure = Some((form.count() - 1, Failure::Boundary));
            }
            let plan = oracle::plan(form, initial, 3, 2, 0x1234_807f, 0x4e70);
            let expected_memory = oracle::memory(bus.words.clone(), &plan);
            let result = cpu.step_physical_instruction(&mut bus);
            assert_eq!(bus.words, expected_memory);
            assert_eq!(bus.count, form.count());
            assert_eq!(bus.entry_count, 0);
            let mut expected = plan.last().unwrap().registers;
            if failure {
                let last = plan.last().unwrap();
                let cause = CriticalFault::BusAbort {
                    access: last.access,
                    direction: Direction::Read,
                };
                assert_eq!(
                    result,
                    Err(InstructionFault::CaptureUnavailable {
                        phase: InstructionPhase::OperandRefill,
                        cause
                    })
                );
                assert_eq!(
                    cpu.physical_entry_state(),
                    PhysicalEntryState::FaultUnqualified { cause }
                );
                assert_eq!(cpu.queued_words(), None);
                guarded(&mut cpu, &mut bus, TimingFault::PhysicalFaultUnqualified);
            } else {
                assert!(matches!(
                    result,
                    Ok(PhysicalInstructionStep::Instruction(_))
                ));
                expected.pc += 2;
                assert_eq!(cpu.queued_words(), Some([0x4e70, 0x7480]));
            }
            assert_eq!(cpu.registers, expected);
            assert_eq!(cpu.clock().0, bus.clock);
        }
    }
}

#[test]
fn newly_admitted_move_decoder_words_are_known_at_the_operand_refill_boundary() {
    let form = Form {
        size: Size::Word,
        write: true,
    };
    for queued in FORMS.map(|form| form.opcode(7, 7)) {
        let (_, mut bus, initial) = seed(form, registers(false, 31), 1, 2, 0x1234, 0x1234_6000);
        let mut cpu =
            PrefetchCpu::from_prefetched(initial, [form.opcode(1, 2), queued], CpuClocks(37))
                .unwrap();
        bus.failure = Some((1, Failure::Boundary));
        let captured = oracle::capture(oracle::plan(form, initial, 1, 2, 0x1234, queued)[1], 2);
        assert_eq!(
            cpu.step_physical_instruction(&mut bus),
            Ok(PhysicalInstructionStep::Group0 {
                opcode: form.opcode(1, 2),
                capture: captured,
                clocks: 58
            })
        );
        oracle::frame(&bus, initial.ssp, captured);
    }
}

#[test]
fn phase_capacity_is_reserved_before_flags_register_updates_or_callbacks() {
    for form in FORMS {
        for blocked in 0..form.count() {
            let (_, mut bus, initial) =
                seed(form, registers(false, 31), 3, 2, 0x0001_0000, 0x1234_6000);
            let origin = u64::MAX - u64::from(u32::MAX) + 1 - blocked as u64 * 4;
            let mut cpu = PrefetchCpu::from_prefetched(
                initial,
                [form.opcode(3, 2), 0x7201],
                CpuClocks(origin),
            )
            .unwrap();
            bus.clock = origin;
            let plan = oracle::plan(form, initial, 3, 2, 0x0001_0000, 0x7201);
            let expected_memory = oracle::memory(bus.words.clone(), &plan[..blocked]);
            assert_eq!(
                cpu.step_physical_instruction(&mut bus),
                Err(InstructionFault::Entry(EntryFault::ClockOverflow))
            );
            assert_eq!(
                cpu.registers,
                if blocked == 0 {
                    initial
                } else {
                    plan[blocked - 1].registers
                }
            );
            assert_eq!(bus.words, expected_memory);
            assert_eq!(bus.count, blocked);
            assert_eq!(
                bus.events,
                oracle::events(&plan[..blocked], &bus.elapsed, origin)
            );
            assert_eq!(cpu.clock().0, bus.clock);
            guarded(&mut cpu, &mut bus, TimingFault::Frozen);
        }
    }
}

#[test]
fn malformed_completion_and_zero_elapsed_write_faults_preserve_consumed_effects() {
    for form in FORMS {
        for failed in 0..form.count() {
            let (mut cpu, mut bus, initial) =
                seed(form, registers(false, 31), 3, 2, 0x8000_0001, 0x1234_6000);
            bus.elapsed[failed] = 3;
            let plan = oracle::plan(form, initial, 3, 2, 0x8000_0001, 0x7201);
            let expected_memory = oracle::memory(bus.words.clone(), &plan[..=failed]);
            assert_eq!(
                cpu.step_physical_instruction(&mut bus),
                Err(InstructionFault::Entry(
                    EntryFault::InvalidCompleteDuration { elapsed: 3 }
                ))
            );
            assert_eq!(cpu.registers, plan[failed].registers);
            assert_eq!(bus.words, expected_memory);
            assert_eq!(bus.count, failed + 1);
            assert_eq!(bus.entry_count, 0);
            assert_eq!(cpu.clock().0, bus.clock);
            guarded(&mut cpu, &mut bus, TimingFault::Frozen);
        }
    }
    for form in FORMS.into_iter().filter(|form| form.write) {
        let (mut cpu, mut bus, initial) =
            seed(form, registers(false, 31), 3, 2, 0x8000_0001, 0x1234_6000);
        bus.failure = Some((0, Failure::Boundary));
        bus.elapsed[0] = 0;
        let plan = oracle::plan(form, initial, 3, 2, 0x8000_0001, 0x7201);
        let captured = oracle::capture(plan[0], 2);
        let expected_memory = oracle::memory(bus.words.clone(), &plan[..1]);
        assert_eq!(
            cpu.step_physical_instruction(&mut bus),
            Ok(PhysicalInstructionStep::Group0 {
                opcode: form.opcode(3, 2),
                capture: captured,
                clocks: 50
            })
        );
        assert_eq!(
            bus.words[&plan[0].access.bus_address],
            expected_memory[&plan[0].access.bus_address]
        );
        oracle::frame(&bus, initial.ssp, captured);
        assert_eq!(cpu.clock().0, bus.clock);
    }
}

#[test]
fn trace_and_accepted_irq_preflight_preserve_the_entire_move_boundary() {
    for form in FORMS {
        for trace in [false, true] {
            let (mut cpu, mut bus, _) =
                seed(form, registers(true, 31), 3, 2, 0x8000_0001, 0x1234_6000);
            if trace {
                cpu.registers
                    .set_status_register(cpu.registers.status_register() | TRACE);
            } else {
                cpu.interrupt_input().set_level(7);
            }
            let before = cpu.clone();
            assert_eq!(
                cpu.step_physical_instruction(&mut bus),
                Err(if trace {
                    InstructionFault::TraceUnqualified
                } else {
                    InstructionFault::InterruptUnqualified
                })
            );
            assert_eq!(cpu, before);
            assert!(bus.events.is_empty());
        }
    }
}
