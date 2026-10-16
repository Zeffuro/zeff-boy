use super::*;

fn guard_unqualified(cpu: &mut PrefetchCpu, bus: &mut Device, cause: CriticalFault) {
    let before = bus.events.clone();
    let clock = cpu.clock();
    cpu.registers.set_status_register(SUPERVISOR);
    cpu.interrupt_input().set_level(7);
    assert_eq!(cpu.prime(bus), Err(TimingFault::PhysicalFaultUnqualified));
    assert_eq!(
        cpu.step_instruction(bus),
        Err(TimingFault::PhysicalFaultUnqualified)
    );
    assert_eq!(cpu.step(bus), Err(TimingFault::PhysicalFaultUnqualified));
    assert_eq!(
        cpu.step_boundary(bus),
        Err(TimingFault::PhysicalFaultUnqualified)
    );
    assert_eq!(
        cpu.service_pending_trace(bus),
        Err(TimingFault::PhysicalFaultUnqualified)
    );
    assert_eq!(
        cpu.step_physical_instruction(bus),
        Err(InstructionFault::Execution(
            TimingFault::PhysicalFaultUnqualified
        ))
    );
    assert_eq!(
        cpu.enter_imported_group0(bus),
        Err(EntryFault::FaultUnqualified(cause))
    );
    assert_eq!(cpu.clock(), clock);
    assert_eq!(bus.events, before);
}

#[test]
fn consumed_instruction_host_errors_and_unavailable_transitions_are_distinct() {
    for case in 0..4 {
        for outcome in [Failure::Host, Failure::Unavailable] {
            let initial = registers(0x001f, 0x2000);
            let (mut cpu, mut bus, phase, captured, expected, prior) =
                capture::scenario(case, initial, 43);
            let failed = usize::from(case == 3);
            bus.instruction_failure = Some((failed, outcome));
            let result = cpu.step_physical_instruction(&mut bus);
            let cause = CriticalFault::BusAbort {
                access: access(captured.logical_address, false),
                direction: Direction::Read,
            };
            if outcome == Failure::Host {
                assert_eq!(
                    result,
                    Err(InstructionFault::Entry(EntryFault::HostFailure {
                        access: access(captured.logical_address, false),
                        direction: Direction::Read,
                        source: "host"
                    }))
                );
                assert_eq!(cpu.physical_entry_state(), PhysicalEntryState::Idle);
                assert_eq!(
                    cpu.physical_inspection(),
                    PhysicalInspection::DiagnosticCompleteValues
                );
            } else {
                assert_eq!(
                    result,
                    Err(InstructionFault::TransitionTimingUnavailable { phase, cause })
                );
                assert_eq!(
                    cpu.physical_entry_state(),
                    PhysicalEntryState::FaultUnqualified { cause }
                );
                assert_eq!(cpu.physical_inspection(), PhysicalInspection::Unavailable);
            }
            assert_eq!(cpu.registers, expected);
            assert_eq!(cpu.queued_words(), None);
            assert_eq!(bus.entry_count, 0);
            assert_eq!(bus.instruction_count, failed + 1);
            assert_eq!(cpu.clock().0, 43 + prior + 17);
            assert_eq!(cpu.clock().0, bus.clock);
            if outcome == Failure::Unavailable {
                guard_unqualified(&mut cpu, &mut bus, cause);
            } else {
                let before = bus.events.clone();
                assert_eq!(
                    cpu.step_physical_instruction(&mut bus),
                    Err(InstructionFault::Execution(TimingFault::Frozen))
                );
                assert_eq!(
                    cpu.external_warm_reset(
                        &mut bus,
                        ExternalResetBoundary {
                            sampled_release: cpu.clock(),
                            qualification: ResetQualification::QualifiedSafeAssertionAndRelease
                        }
                    ),
                    Err(EntryFault::HostFrozen)
                );
                assert_eq!(bus.events, before);
            }
        }
    }
}

#[test]
fn unknown_promoted_words_block_capture_only_after_the_consumed_failure() {
    for case in [0, 1, 3] {
        for unknown in [0x103d, 0x4e70, 0x4e72, 0x4afc] {
            let initial = registers(0x001f, 0x2000);
            let (mut cpu, mut bus, phase, captured, expected, prior) =
                capture::scenario(case, initial, 61);
            if case == 3 {
                bus.word(0x200a, unknown);
            } else {
                let opcode = if case == 0 { 0x4e71 } else { 0x7aa5 };
                cpu = PrefetchCpu::from_prefetched(initial, [opcode, unknown], CpuClocks(61))
                    .unwrap();
            }
            let cause = CriticalFault::BusAbort {
                access: access(captured.logical_address, false),
                direction: Direction::Read,
            };
            assert_eq!(
                cpu.step_physical_instruction(&mut bus),
                Err(InstructionFault::CaptureUnavailable { phase, cause })
            );
            assert_eq!(cpu.registers, expected);
            assert_eq!(bus.entry_count, 0);
            assert_eq!(cpu.queued_words(), None);
            assert_eq!(cpu.clock().0, 61 + prior + 17);
            assert_eq!(cpu.clock().0, bus.clock);
            assert_eq!(cpu.physical_inspection(), PhysicalInspection::Unavailable);
            guard_unqualified(&mut cpu, &mut bus, cause);
            bus.word(0, 0x5555);
            bus.word(2, 0x7001);
            bus.word(4, 0x1234);
            bus.word(6, 0x4000);
            let release = cpu.clock();
            assert_eq!(
                cpu.external_warm_reset(
                    &mut bus,
                    ExternalResetBoundary {
                        sampled_release: release,
                        qualification: ResetQualification::QualifiedSafeAssertionAndRelease
                    }
                )
                .unwrap()
                .clocks,
                40
            );
            assert_eq!(cpu.registers.ssp, 0x5555_7001);
            assert_eq!(cpu.registers.d, expected.d);
            assert_eq!(cpu.registers.a, expected.a);
            assert_eq!(cpu.registers.usp, expected.usp);
            assert_eq!(cpu.queued_words(), Some([0x4e71, 0x7201]));
            assert_eq!(
                cpu.physical_inspection(),
                PhysicalInspection::DiagnosticCompleteValues
            );
        }
    }
}

#[test]
fn odd_branch_target_requires_an_explicit_entry_boundary_and_no_aligned_fetch() {
    for outcome in [
        None,
        Some(Failure::Boundary),
        Some(Failure::Host),
        Some(Failure::ShortComplete),
    ] {
        let initial = registers(0x0015, 0x2000);
        let mut cpu =
            PrefetchCpu::from_prefetched(initial, [0x6001, 0x7201], CpuClocks(71)).unwrap();
        let mut bus = Device::new(71);
        bus.detection = outcome.map(|outcome| (outcome, 19));
        let detected = access(0x2003, false);
        let cause = CriticalFault::AddressError {
            access: detected,
            direction: Direction::Read,
        };
        let result = cpu.step_physical_instruction(&mut bus);
        assert_eq!(bus.instruction_count, 0);
        assert!(
            !bus.events
                .iter()
                .any(|event| matches!(event, Event::Instruction(..)))
        );
        if outcome == Some(Failure::Boundary) {
            let captured = Group0Capture {
                vector: 3,
                internal_pc: 0x2002,
                phase_sr: 0x15,
                ird: 0x6001,
                logical_address: 0x2003,
                function_code: FunctionCode::UserProgram,
                direction: Direction::Read,
                processing_class: ProcessingClass::InstructionOrGroup2,
                ssw_profile: SswProfile::SelectedIrdUpperBits,
            };
            assert_eq!(
                result,
                Ok(PhysicalInstructionStep::Group0 {
                    opcode: 0x6001,
                    capture: captured,
                    clocks: 71
                })
            );
            assert_frame(&bus, initial.ssp, captured);
            assert_eq!(cpu.clock(), CpuClocks(142));
        } else if outcome == Some(Failure::Host) {
            assert_eq!(
                result,
                Err(InstructionFault::Entry(EntryFault::HostFailure {
                    access: detected,
                    direction: Direction::Read,
                    source: "host"
                }))
            );
            assert_eq!(bus.entry_count, 0);
            assert_eq!(cpu.clock(), CpuClocks(92));
        } else {
            assert_eq!(
                result,
                Err(InstructionFault::TransitionTimingUnavailable {
                    phase: InstructionPhase::BranchFirst,
                    cause
                })
            );
            assert_eq!(bus.entry_count, 0);
            assert_eq!(cpu.clock().0, 73 + if outcome.is_some() { 19 } else { 0 });
            assert_eq!(cpu.physical_inspection(), PhysicalInspection::Unavailable);
        }
        assert_eq!(cpu.clock().0, bus.clock);
        if outcome.is_some() {
            assert_eq!(bus.events[1], Event::Detection(detected, 73));
        } else {
            assert_eq!(bus.events, vec![Event::Idle(71, 2)]);
        }
    }
}

#[test]
fn trace_irq_and_unsupported_instruction_preflight_are_side_effect_free() {
    for (sr, irq, opcode, expected) in [
        (
            TRACE | SUPERVISOR,
            0,
            0x4e71,
            InstructionFault::TraceUnqualified,
        ),
        (
            SUPERVISOR,
            1,
            0x4e71,
            InstructionFault::InterruptUnqualified,
        ),
        (
            SUPERVISOR | 0x0700,
            7,
            0x4e71,
            InstructionFault::InterruptUnqualified,
        ),
        (
            SUPERVISOR,
            0,
            0x4e70,
            InstructionFault::Execution(TimingFault::UnsupportedInstruction {
                pc: 0x2000,
                opcode: 0x4e70,
            }),
        ),
    ] {
        let initial = registers(sr, 0x2000);
        let mut cpu =
            PrefetchCpu::from_prefetched(initial, [opcode, 0x7201], CpuClocks(0)).unwrap();
        cpu.interrupt_input().set_level(irq);
        let mut bus = Device::new(0);
        let before_cpu = cpu.clone();
        assert_eq!(cpu.step_physical_instruction(&mut bus), Err(expected));
        assert_eq!(cpu, before_cpu);
        assert_eq!(cpu.registers, initial);
        assert_eq!(cpu.queued_words(), Some([opcode, 0x7201]));
        assert_eq!(cpu.clock(), CpuClocks(0));
        assert!(bus.events.is_empty());
    }
    for (trace, stopped) in [(true, false), (false, true)] {
        let initial = registers(SUPERVISOR | if trace { TRACE } else { 0 }, 0x2000);
        let mut cpu = PrefetchCpu::from_prefetched(
            initial,
            if stopped {
                [0x4e72, 0x2700]
            } else {
                [0x4e71, 0x7201]
            },
            CpuClocks(0),
        )
        .unwrap();
        let mut bus = Device::new(0);
        cpu.step_instruction(&mut bus).unwrap();
        let before = bus.events.clone();
        assert_eq!(
            cpu.step_physical_instruction(&mut bus),
            Err(InstructionFault::Execution(if trace {
                TimingFault::PendingTrace
            } else {
                TimingFault::Stopped
            }))
        );
        assert_eq!(bus.events, before);
    }
}

#[test]
fn mask_import_unprimed_and_doublehalt_gates_preserve_their_boundaries() {
    let initial = registers(SUPERVISOR | 0x0300, 0x2000);
    let mut cpu = PrefetchCpu::from_prefetched(initial, [0x4e71, 0x7201], CpuClocks(0)).unwrap();
    cpu.interrupt_input().set_level(3);
    let mut bus = Device::new(0);
    assert!(matches!(
        cpu.step_physical_instruction(&mut bus),
        Ok(PhysicalInstructionStep::Instruction(_))
    ));
    assert_eq!(cpu.interrupt_input().level(), 3);
    let mut cpu = PrefetchCpu::default();
    let mut bus = Device::new(0);
    assert_eq!(
        cpu.step_physical_instruction(&mut bus),
        Err(InstructionFault::Execution(TimingFault::Unprimed))
    );
    assert!(bus.events.is_empty());
    let (_, _, _, captured, _, _) = capture::scenario(0, initial, 0);
    let mut cpu =
        PrefetchCpu::from_group0_boundary(zeff_m68k::timing::physical::ImportedGroup0Boundary {
            registers: initial,
            clock: CpuClocks(0),
            capture: captured,
        })
        .unwrap();
    assert_eq!(
        cpu.step_physical_instruction(&mut bus),
        Err(InstructionFault::Execution(
            TimingFault::ImportedBoundaryPending
        ))
    );
    assert!(bus.events.is_empty());
    bus.entry_failure = Some((0, Failure::Boundary));
    cpu.enter_imported_group0(&mut bus).unwrap_err();
    let before = bus.events.clone();
    let clock = cpu.clock();
    assert_eq!(
        cpu.step_physical_instruction(&mut bus),
        Err(InstructionFault::Execution(TimingFault::DoubleFaultHalted))
    );
    assert_eq!(cpu.clock(), clock);
    assert_eq!(bus.events, before);
}

#[test]
fn completed_branch_transfer_reaching_clock_limit_is_never_retried() {
    let initial = registers(0x001f, 0x2000);
    let origin = u64::MAX - u64::from(u32::MAX) - 2;
    let mut cpu =
        PrefetchCpu::from_prefetched(initial, [0x6008, 0x7201], CpuClocks(origin)).unwrap();
    let mut bus = Device::new(origin);
    bus.word(0x200a, 0x4e71);
    bus.instruction_elapsed[0] = u32::MAX;
    assert_eq!(
        cpu.step_physical_instruction(&mut bus),
        Err(InstructionFault::Entry(EntryFault::ClockOverflow))
    );
    assert_eq!(cpu.registers, initial);
    assert_eq!(cpu.clock(), CpuClocks(u64::MAX));
    assert_eq!(bus.clock, u64::MAX);
    assert_eq!(
        bus.events,
        vec![
            Event::Idle(origin, 2),
            Event::Instruction(access(0x200a, false), origin + 2)
        ]
    );
    assert_eq!(bus.instruction_count, 1);
    assert_eq!(bus.entry_count, 0);
    let before = bus.events.clone();
    assert_eq!(
        cpu.step_physical_instruction(&mut bus),
        Err(InstructionFault::Execution(TimingFault::Frozen))
    );
    assert_eq!(bus.events, before);
}

#[test]
fn moveq_physical_host_snapshot_does_not_change_the_legacy_host_policy() {
    let initial = registers(0x001f, 0x2000);
    for physical in [false, true] {
        let mut cpu =
            PrefetchCpu::from_prefetched(initial, [0x7aa5, 0x4e71], CpuClocks(0)).unwrap();
        let mut bus = Device::new(0);
        bus.instruction_elapsed[0] = 17;
        bus.instruction_failure = Some((0, Failure::Host));
        if physical {
            assert!(matches!(
                cpu.step_physical_instruction(&mut bus),
                Err(InstructionFault::Entry(EntryFault::HostFailure { .. }))
            ));
        } else {
            assert!(matches!(
                cpu.step_instruction(&mut bus),
                Err(TimingFault::Bus { .. })
            ));
        }
        let mut expected = initial;
        if physical {
            moveq(&mut expected, 5, 0xa5);
        }
        assert_eq!(cpu.registers, expected);
        assert_eq!(cpu.clock(), CpuClocks(17));
        assert_eq!(bus.clock, 17);
        assert_eq!(bus.entry_count, 0);
        assert_eq!(bus.instruction_count, 1);
    }
}

#[test]
fn actual_default_odd_detection_preserves_observed_time_without_data_strobes() {
    let initial = registers(0x001f, 0x2000);
    let mut cpu = PrefetchCpu::from_prefetched(initial, [0x6001, 0x7201], CpuClocks(71)).unwrap();
    let mut bus = DefaultInstructionDetection(Device::new(71));
    let cause = CriticalFault::AddressError {
        access: access(0x2003, false),
        direction: Direction::Read,
    };
    assert_eq!(
        cpu.step_physical_instruction(&mut bus),
        Err(InstructionFault::TransitionTimingUnavailable {
            phase: InstructionPhase::BranchFirst,
            cause
        })
    );
    assert_eq!(bus.0.events, vec![Event::Idle(71, 2)]);
    assert_eq!(cpu.clock(), CpuClocks(73));
    assert_eq!(cpu.clock().0, bus.0.clock);
    assert_eq!(cpu.registers, initial);
    assert_eq!(cpu.queued_words(), None);
    assert_eq!(
        cpu.physical_entry_state(),
        PhysicalEntryState::FaultUnqualified { cause }
    );
    assert_eq!(cpu.physical_inspection(), PhysicalInspection::Unavailable);
}

#[test]
fn capacity_and_malformed_reports_fail_closed_without_repeating_callbacks() {
    let initial = registers(0x001f, 0x2000);
    let origin = u64::MAX - u64::from(u32::MAX) + 1;
    let mut cpu =
        PrefetchCpu::from_prefetched(initial, [0x7aa5, 0x4e71], CpuClocks(origin)).unwrap();
    let mut bus = Device::new(origin);
    assert_eq!(
        cpu.step_physical_instruction(&mut bus),
        Err(InstructionFault::Entry(EntryFault::ClockOverflow))
    );
    assert_eq!(cpu.registers, initial);
    assert!(bus.events.is_empty());
    for case in 0..4 {
        let (mut cpu, mut bus, _, _, expected, prior) = capture::scenario(case, initial, 0);
        bus.instruction_failure = Some((usize::from(case == 3), Failure::ShortComplete));
        assert_eq!(
            cpu.step_physical_instruction(&mut bus),
            Err(InstructionFault::Entry(
                EntryFault::InvalidCompleteDuration { elapsed: 3 }
            ))
        );
        assert_eq!(cpu.clock().0, prior + 3);
        assert_eq!(cpu.clock().0, bus.clock);
        assert_eq!(cpu.registers, expected);
        assert_eq!(bus.entry_count, 0);
        let before = bus.events.clone();
        assert_eq!(
            cpu.step_physical_instruction(&mut bus),
            Err(InstructionFault::Execution(TimingFault::Frozen))
        );
        assert_eq!(bus.events, before);
    }
}
