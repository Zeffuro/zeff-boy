use super::*;
use zeff_m68k::timing::physical::{EntryKind, ImportedGroup0Boundary};

fn critical_cpu(reset: bool, opcode: u16, next: u16) -> (PrefetchCpu, Device) {
    let initial = registers(0x001f, 0x2000);
    let (_, _, _, captured, _, _) = capture::scenario(0, initial, 0);
    let mut cpu = PrefetchCpu::from_group0_boundary(ImportedGroup0Boundary {
        registers: initial,
        clock: CpuClocks(0),
        capture: captured,
    })
    .unwrap();
    let mut bus = Device::new(0);
    bus.word(0x1234_4000, opcode);
    bus.word(0x1234_4002, next);
    if reset {
        bus.word(0, 0x8765);
        bus.word(2, 0x6000);
        bus.word(4, 0x1234);
        bus.word(6, 0x4000);
        cpu.external_warm_reset(
            &mut bus,
            ExternalResetBoundary {
                sampled_release: CpuClocks(0),
                qualification: ResetQualification::QualifiedSafeAssertionAndRelease,
            },
        )
        .unwrap();
    } else {
        cpu.enter_imported_group0(&mut bus).unwrap();
    }
    assert_eq!(
        cpu.physical_entry_state(),
        PhysicalEntryState::Critical(if reset {
            EntryKind::ExternalWarmReset
        } else {
            EntryKind::ImportedGroup0
        })
    );
    assert_eq!(cpu.queued_words(), Some([opcode, next]));
    bus.events.clear();
    bus.entry_count = 0;
    (cpu, bus)
}

#[test]
fn first_admitted_handler_instruction_fault_starts_a_fresh_group0_frame() {
    for reset in [false, true] {
        for case in 0..4 {
            let opcode = [0x4e71, 0x7aa5, 0x6008, 0x6008][case];
            let (mut cpu, mut bus) = critical_cpu(reset, opcode, 0x4e71);
            let initial = cpu.registers;
            let start = cpu.clock();
            bus.word(initial.pc + 10, 0x7480);
            bus.instruction_elapsed = [7, 13];
            let failed = usize::from(case == 3);
            bus.instruction_elapsed[failed] = 13;
            bus.instruction_failure = Some((failed, Failure::Boundary));
            let mut expected = initial;
            if case == 1 {
                moveq(&mut expected, 5, 0xa5);
            }
            let captured = Group0Capture {
                vector: 2,
                internal_pc: initial.pc
                    + if case < 2 {
                        4
                    } else if case == 2 {
                        2
                    } else {
                        12
                    },
                phase_sr: expected.status_register(),
                ird: if case < 2 {
                    0x4e71
                } else if case == 2 {
                    0x6008
                } else {
                    0x7480
                },
                logical_address: initial.pc
                    + if case < 2 {
                        4
                    } else if case == 2 {
                        10
                    } else {
                        12
                    },
                function_code: FunctionCode::SupervisorProgram,
                direction: Direction::Read,
                processing_class: ProcessingClass::InstructionOrGroup2,
                ssw_profile: SswProfile::SelectedIrdUpperBits,
            };
            let prior = if case < 2 {
                0
            } else if case == 2 {
                2
            } else {
                9
            };
            assert_eq!(
                cpu.step_physical_instruction(&mut bus),
                Ok(PhysicalInstructionStep::Group0 {
                    opcode,
                    capture: captured,
                    clocks: prior + 13 + 50
                })
            );
            assert_frame(&bus, initial.ssp, captured);
            expected.ssp -= 14;
            expected.pc = 0x1234_4000;
            assert_eq!(cpu.registers, expected);
            assert_eq!(cpu.queued_words(), Some([opcode, 0x4e71]));
            assert_eq!(cpu.clock().0, start.0 + prior + 13 + 50);
            assert_eq!(cpu.clock().0, bus.clock);
            assert_eq!(bus.entry_count, 11);
            assert_eq!(
                cpu.physical_entry_state(),
                PhysicalEntryState::Critical(EntryKind::ImportedGroup0)
            );
            assert_eq!(
                cpu.physical_inspection(),
                PhysicalInspection::DiagnosticCompleteValues
            );
        }
    }
}

#[test]
fn unsupported_trace_and_irq_handler_preflight_preserve_the_critical_boundary() {
    for reset in [false, true] {
        for gate in 0..3 {
            let opcode = if gate == 0 { 0x4e70 } else { 0x4e71 };
            let (mut cpu, mut bus) = critical_cpu(reset, opcode, 0x7201);
            if gate == 1 {
                cpu.registers
                    .set_status_register(cpu.registers.status_register() | TRACE);
            }
            if gate == 2 {
                cpu.interrupt_input().set_level(7);
            }
            let before = cpu.clone();
            let expected = match gate {
                0 => InstructionFault::Execution(TimingFault::UnsupportedInstruction {
                    pc: 0x1234_4000,
                    opcode,
                }),
                1 => InstructionFault::TraceUnqualified,
                _ => InstructionFault::InterruptUnqualified,
            };
            assert_eq!(cpu.step_physical_instruction(&mut bus), Err(expected));
            assert_eq!(cpu, before);
            assert!(bus.events.is_empty());
            assert_eq!(cpu.clock().0, bus.clock);
        }
    }
}

#[test]
fn supplied_subtransfer_entry_boundaries_add_only_the_entry_schedule() {
    for elapsed in [0, 1, 3] {
        for case in 0..4 {
            let initial = registers(0x001f, 0x2000);
            let (mut cpu, mut bus, _, captured, mut expected, prior) =
                capture::scenario(case, initial, 101);
            bus.instruction_elapsed[usize::from(case == 3)] = elapsed;
            let opcode = [0x4e71, 0x7aa5, 0x6008, 0x6008][case];
            assert_eq!(
                cpu.step_physical_instruction(&mut bus),
                Ok(PhysicalInstructionStep::Group0 {
                    opcode,
                    capture: captured,
                    clocks: prior + u64::from(elapsed) + 50
                })
            );
            assert_frame(&bus, initial.ssp, captured);
            expected.ssp -= 14;
            expected.pc = 0x1234_4000;
            expected.set_status_register(expected.status_register() | SUPERVISOR);
            assert_eq!(cpu.registers, expected);
            assert_eq!(cpu.clock().0, 101 + prior + u64::from(elapsed) + 50);
            assert_eq!(cpu.clock().0, bus.clock);
            assert_eq!(bus.entry_count, 11);
        }
    }
}

#[test]
fn maximal_total_abort_and_host_reports_are_consumed_before_terminal_propagation() {
    let origin = u64::MAX - u64::from(u32::MAX);
    for outcome in [Failure::Boundary, Failure::Host] {
        let initial = registers(0x001f, 0x2000);
        let mut cpu =
            PrefetchCpu::from_prefetched(initial, [0x7aa5, 0x4e71], CpuClocks(origin)).unwrap();
        let mut bus = Device::new(origin);
        bus.instruction_elapsed[0] = u32::MAX;
        bus.instruction_failure = Some((0, outcome));
        let result = cpu.step_physical_instruction(&mut bus);
        let expected_error = if outcome == Failure::Boundary {
            EntryFault::ClockOverflow
        } else {
            EntryFault::HostFailure {
                access: access(0x2004, false),
                direction: Direction::Read,
                source: "host",
            }
        };
        assert_eq!(result, Err(InstructionFault::Entry(expected_error)));
        let mut expected = initial;
        moveq(&mut expected, 5, 0xa5);
        assert_eq!(cpu.registers, expected);
        assert_eq!(cpu.clock(), CpuClocks(u64::MAX));
        assert_eq!(cpu.queued_words(), None);
        assert_eq!(bus.clock, u64::MAX);
        assert_eq!(
            bus.events,
            vec![Event::Instruction(access(0x2004, false), origin)]
        );
        assert_eq!(bus.instruction_count, 1);
        assert_eq!(bus.entry_count, 0);
        let before = bus.events.clone();
        assert_eq!(
            cpu.step_physical_instruction(&mut bus),
            Err(InstructionFault::Execution(TimingFault::Frozen))
        );
        assert_eq!(bus.events, before);
        assert_eq!(
            cpu.external_warm_reset(
                &mut bus,
                ExternalResetBoundary {
                    sampled_release: CpuClocks(u64::MAX),
                    qualification: ResetQualification::QualifiedSafeAssertionAndRelease
                }
            ),
            Err(EntryFault::HostFrozen)
        );
        assert_eq!(bus.events, before);
    }
}
