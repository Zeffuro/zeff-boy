use super::*;

#[test]
fn every_reservation_precedes_its_address_data_or_flag_publication() {
    let mut positions = 0;
    for form in FORMS {
        for blocked in 0..form.count() {
            positions += 1;
            for supervisor in [false, true] {
                let (_, mut bus, initial) = seed(
                    form,
                    registers(supervisor, 31),
                    3,
                    7,
                    0x0001_0000,
                    0x1234_6000,
                );
                let origin = u64::MAX - u64::from(u32::MAX) + 1
                    - u64::from(form.idle())
                    - blocked as u64 * 4;
                let mut cpu = PrefetchCpu::from_prefetched(
                    initial,
                    [form.opcode(3, 7), 0x7201],
                    CpuClocks(origin),
                )
                .unwrap();
                bus.clock = origin;
                let plan = oracle::plan(form, initial, 3, 7, 0x0001_0000, 0x7201);
                let memory = oracle::memory(bus.words.clone(), &plan[..blocked]);
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
                assert_eq!(bus.words, memory);
                assert_eq!(
                    bus.events,
                    oracle::events(form, &plan[..blocked], &bus.elapsed, origin)
                );
                assert_eq!(bus.count, blocked);
                assert_eq!(bus.entry_count, 0);
                assert_eq!(
                    cpu.clock().0,
                    origin + u64::from(form.idle()) + blocked as u64 * 4
                );
                assert_eq!(cpu.clock().0, bus.clock);
                guarded(&mut cpu, &mut bus, TimingFault::Frozen);
            }
        }
    }
    assert_eq!(positions, 28);
}

#[test]
fn initial_predecrement_idle_checks_only_its_own_capacity_and_keeps_old_registers() {
    for size in [Size::Byte, Size::Word, Size::Long] {
        let form = Form {
            size,
            route: Route::PreLoad,
        };
        let (_, mut bus, initial) = seed(form, registers(true, 31), 3, 7, 0x8000_0001, 0x1234_6000);
        let origin = u64::MAX - 1;
        let mut cpu =
            PrefetchCpu::from_prefetched(initial, [form.opcode(3, 7), 0x7201], CpuClocks(origin))
                .unwrap();
        bus.clock = origin;
        let memory = bus.words.clone();
        assert_eq!(
            cpu.step_physical_instruction(&mut bus),
            Err(InstructionFault::Entry(EntryFault::ClockOverflow))
        );
        assert_eq!(cpu.registers, initial);
        assert_eq!(cpu.clock().0, origin);
        assert_eq!(bus.clock, origin);
        assert!(bus.events.is_empty());
        assert_eq!(bus.words, memory);
        guarded(&mut cpu, &mut bus, TimingFault::Frozen);
    }
}

#[test]
fn final_predecrement_write_can_complete_at_maximum_clock_without_another_reserve() {
    for size in [Size::Byte, Size::Word, Size::Long] {
        let form = Form {
            size,
            route: Route::PreStore,
        };
        let (_, mut bus, initial) = seed(form, registers(true, 31), 3, 7, 0x8000_0001, 0x1234_6000);
        let last = form.count() - 1;
        let origin = u64::MAX - u64::from(u32::MAX) - last as u64 * 4;
        let mut cpu =
            PrefetchCpu::from_prefetched(initial, [form.opcode(3, 7), 0x7201], CpuClocks(origin))
                .unwrap();
        bus.clock = origin;
        bus.elapsed[last] = u32::MAX;
        let plan = oracle::plan(form, initial, 3, 7, 0x8000_0001, 0x7201);
        let memory = oracle::memory(bus.words.clone(), &plan);
        let next = oracle::fetched(bus.words.clone(), &plan);
        let clocks = u64::from(u32::MAX) + last as u64 * 4;
        assert_eq!(
            cpu.step_physical_instruction(&mut bus),
            Ok(PhysicalInstructionStep::Instruction(
                zeff_m68k::timing::TimedStep {
                    opcode: form.opcode(3, 7),
                    clocks,
                    exception: None
                }
            ))
        );
        let mut expected = plan[last].registers;
        expected.pc = initial.pc + 2;
        assert_eq!(cpu.registers, expected);
        assert_eq!(cpu.queued_words(), Some([0x7201, next]));
        assert_eq!(
            bus.events,
            oracle::events(form, &plan, &bus.elapsed, origin)
        );
        assert_eq!(bus.words, memory);
        assert_eq!(bus.entry_count, 0);
        assert_eq!(cpu.clock().0, u64::MAX);
        assert_eq!(bus.clock, u64::MAX);
    }
}

#[test]
fn malformed_completion_keeps_consumed_phase_effects_and_freezes_without_entry() {
    for form in FORMS {
        for failed in 0..form.count() {
            let (mut cpu, mut bus, initial) =
                seed(form, registers(false, 31), 3, 2, 0x8000_0001, 0x1234_6000);
            bus.elapsed[failed] = 3;
            let plan = oracle::plan(form, initial, 3, 2, 0x8000_0001, 0x7201);
            let memory = oracle::memory(bus.words.clone(), &plan[..=failed]);
            assert_eq!(
                cpu.step_physical_instruction(&mut bus),
                Err(InstructionFault::Entry(
                    EntryFault::InvalidCompleteDuration { elapsed: 3 }
                ))
            );
            assert_eq!(cpu.registers, plan[failed].registers);
            assert_eq!(bus.words, memory);
            assert_eq!(
                bus.events,
                oracle::events(form, &plan[..=failed], &bus.elapsed, 37)
            );
            assert_eq!(bus.count, failed + 1);
            assert_eq!(bus.entry_count, 0);
            assert_eq!(cpu.clock().0, bus.clock);
            guarded(&mut cpu, &mut bus, TimingFault::Frozen);
        }
    }
}

#[test]
fn zero_elapsed_boundaries_add_only_entry_and_keep_all_completed_device_writes() {
    for form in FORMS {
        for failed in 0..form.count() {
            let (mut cpu, mut bus, initial) =
                seed(form, registers(false, 31), 3, 2, 0x8000_0001, 0x1234_6000);
            bus.elapsed[failed] = 0;
            bus.failure = Some((failed, Failure::Boundary));
            let plan = oracle::plan(form, initial, 3, 2, 0x8000_0001, 0x7201);
            let capture = oracle::capture(plan[failed], 2);
            let clocks = u64::from(form.idle()) + failed as u64 * 4;
            let memory = oracle::entry_memory(
                oracle::memory(bus.words.clone(), &plan[..=failed]),
                initial.ssp,
                capture,
            );
            assert_eq!(
                cpu.step_physical_instruction(&mut bus),
                Ok(PhysicalInstructionStep::Group0 {
                    opcode: form.opcode(3, 2),
                    capture,
                    clocks: clocks + 50
                })
            );
            assert_eq!(cpu.registers, oracle::entered(plan[failed].registers));
            assert_eq!(bus.words, memory);
            oracle::frame(&bus, initial.ssp, capture, 37 + clocks);
            assert_eq!(cpu.clock().0, 37 + clocks + 50);
            assert_eq!(cpu.clock().0, bus.clock);
        }
    }
}

#[test]
fn maximal_final_write_abort_retains_consumption_before_entry_capacity_failure() {
    let form = Form {
        size: Size::Long,
        route: Route::PreStore,
    };
    for failure in [Failure::Host, Failure::Boundary, Failure::Unavailable] {
        let (_, mut bus, initial) = seed(form, registers(true, 31), 3, 7, 0x0001_0000, 0x1234_6000);
        let origin = u64::MAX - u64::from(u32::MAX) - 8;
        let mut cpu =
            PrefetchCpu::from_prefetched(initial, [form.opcode(3, 7), 0x7201], CpuClocks(origin))
                .unwrap();
        bus.clock = origin;
        bus.elapsed[2] = u32::MAX;
        bus.failure = Some((2, failure));
        let plan = oracle::plan(form, initial, 3, 7, 0x0001_0000, 0x7201);
        let memory = oracle::memory(bus.words.clone(), &plan);
        let result = cpu.step_physical_instruction(&mut bus);
        assert_eq!(
            result,
            Err(match failure {
                Failure::Host => InstructionFault::Entry(EntryFault::HostFailure {
                    access: plan[2].access,
                    direction: Direction::Write,
                    source: "host"
                }),
                Failure::Boundary => InstructionFault::Entry(EntryFault::ClockOverflow),
                _ => InstructionFault::TransitionTimingUnavailable {
                    phase: InstructionPhase::OperandWriteHigh,
                    cause: CriticalFault::BusAbort {
                        access: plan[2].access,
                        direction: Direction::Write
                    }
                },
            })
        );
        assert_eq!(cpu.registers, plan[2].registers);
        assert_eq!(bus.words, memory);
        assert_eq!(
            bus.events,
            oracle::events(form, &plan, &bus.elapsed, origin)
        );
        assert_eq!(bus.count, 3);
        assert_eq!(bus.entry_count, 0);
        assert_eq!(cpu.clock().0, u64::MAX);
        assert_eq!(cpu.clock().0, bus.clock);
        guarded(
            &mut cpu,
            &mut bus,
            if failure == Failure::Unavailable {
                TimingFault::PhysicalFaultUnqualified
            } else {
                TimingFault::Frozen
            },
        );
    }
}

#[test]
fn trace_and_accepted_interrupt_preflight_preserve_every_update_form() {
    for form in FORMS {
        for trace in [false, true] {
            let (mut cpu, mut bus, _) =
                seed(form, registers(true, 31), 3, 7, 0x8000_0001, 0x1234_6000);
            if trace {
                cpu.registers
                    .set_status_register(cpu.registers.status_register() | TRACE);
            } else {
                cpu.interrupt_input().set_level(7);
            }
            let before = cpu.clone();
            let memory = bus.words.clone();
            assert_eq!(
                cpu.step_physical_instruction(&mut bus),
                Err(if trace {
                    InstructionFault::TraceUnqualified
                } else {
                    InstructionFault::InterruptUnqualified
                })
            );
            assert_eq!(cpu, before);
            assert_eq!(bus.words, memory);
            assert!(bus.events.is_empty());
            assert_eq!(bus.clock, 37);
        }
    }
}
