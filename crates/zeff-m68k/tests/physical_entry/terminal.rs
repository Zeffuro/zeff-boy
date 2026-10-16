use super::*;

fn attempted_prefix(events: Vec<Event>, attempted: usize) -> Vec<Event> {
    let end = events
        .iter()
        .enumerate()
        .filter(|(_, event)| matches!(event, Event::Read(..) | Event::Write(..)))
        .nth(attempted - 1)
        .unwrap()
        .0;
    events[..=end].to_vec()
}

fn guarded(cpu: &mut PrefetchCpu, bus: &mut Device, physical: bool) {
    let before = bus.events.clone();
    let clock = cpu.clock();
    let expected = if physical {
        TimingFault::DoubleFaultHalted
    } else {
        TimingFault::Frozen
    };
    cpu.registers.pc = 0x800;
    cpu.registers.set_status_register(SUPERVISOR | TRACE);
    cpu.interrupt_input().set_level(7);
    assert_eq!(cpu.prime(bus), Err(expected.clone()));
    assert_eq!(cpu.step_instruction(bus), Err(expected.clone()));
    assert_eq!(cpu.step(bus), Err(expected.clone()));
    assert_eq!(cpu.service_pending_trace(bus), Err(expected.clone()));
    assert_eq!(cpu.step_boundary(bus), Err(expected));
    assert_eq!(bus.events, before);
    assert_eq!(cpu.clock(), clock);
    assert_eq!(bus.ownership_edges, 0);
}

fn assert_failure(
    cpu: &PrefetchCpu,
    result: &Result<zeff_m68k::timing::physical::EntryStep, EntryFault<&'static str>>,
    injection: Injection,
    access: BusAccess,
    direction: Direction,
) {
    match injection {
        Injection::Host => {
            assert_eq!(
                result,
                &Err(EntryFault::HostFailure {
                    access,
                    direction,
                    source: "host"
                })
            );
            assert!(!matches!(
                cpu.physical_entry_state(),
                PhysicalEntryState::DoubleFaultHalted { .. }
            ));
        }
        Injection::Released | Injection::Unavailable => {
            let cause = CriticalFault::BusAbort { access, direction };
            let timing = if injection == Injection::Released {
                TimingQualification::Qualified
            } else {
                TimingQualification::Unavailable
            };
            let expected = if injection == Injection::Released {
                EntryFault::DoubleFaultHalted(cause)
            } else {
                EntryFault::TimingUnavailable(cause)
            };
            assert_eq!(result, &Err(expected));
            assert_eq!(
                cpu.physical_entry_state(),
                PhysicalEntryState::DoubleFaultHalted { cause, timing }
            );
            assert_eq!(cpu.physical_inspection(), PhysicalInspection::Unavailable);
        }
        Injection::ShortComplete => assert_eq!(
            result,
            &Err(EntryFault::InvalidCompleteDuration { elapsed: 3 })
        ),
    }
}

#[test]
fn every_group0_critical_transfer_is_consumed_once_without_recursive_entry() {
    for injection in [Injection::Host, Injection::Released, Injection::Unavailable] {
        for failed in 0..11 {
            let initial = registers();
            let captured = capture();
            let mut cpu = imported(initial, captured, 91);
            let mut bus = Device::new(91);
            bus.handler(2, 0x1020);
            bus.elapsed = (0..11).map(|i| 6 + i).collect();
            bus.inject = Some((failed, injection));
            let result = cpu.enter_imported_group0(&mut bus);
            let expected_frame = group0::frame(
                captured.internal_pc,
                captured.phase_sr,
                captured.ird,
                captured.logical_address,
                0xbad2,
            );
            assert_eq!(
                bus.events,
                attempted_prefix(
                    group0::schedule(initial.ssp, 2, 0x1020, expected_frame, &bus.elapsed, 91),
                    failed + 1
                )
            );
            let (failed_access, value, _) = bus.transfers()[failed];
            assert_failure(
                &cpu,
                &result,
                injection,
                failed_access,
                if value.is_some() {
                    Direction::Write
                } else {
                    Direction::Read
                },
            );
            assert_eq!(bus.transfers, failed + 1);
            assert_eq!(
                cpu.clock().0,
                91 + 4
                    + bus.elapsed[..=failed]
                        .iter()
                        .map(|&n| u64::from(n))
                        .sum::<u64>()
                    + if failed == 10 { 2 } else { 0 }
            );
            assert_eq!(cpu.clock().0, bus.clock);
            assert_eq!(cpu.registers, initial);
            assert_eq!(cpu.queued_words(), None);
            for (written, value, _) in bus.transfers().into_iter().filter(|(_, v, _)| v.is_some()) {
                assert_eq!(bus.words[&written.bus_address], value.unwrap());
            }
            let before = bus.events.clone();
            assert!(cpu.enter_imported_group0(&mut bus).is_err());
            assert_eq!(bus.events, before);
            guarded(&mut cpu, &mut bus, injection != Injection::Host);
        }
    }
}

#[test]
fn every_reset_read_is_critical_and_failed_reset_preserves_whole_diagnostics() {
    for injection in [Injection::Host, Injection::Released, Injection::Unavailable] {
        for failed in 0..6 {
            let initial = registers();
            let mut cpu =
                PrefetchCpu::from_prefetched(initial, [0x4e70, 0x4e71], CpuClocks(19)).unwrap();
            let mut bus = Device::new(45);
            bus.reset_vectors(0x1020_3001, 0xabcd_4000);
            bus.elapsed = (0..6).map(|i| 5 + i * 2).collect();
            bus.inject = Some((failed, injection));
            let result = cpu.external_warm_reset(&mut bus, reset_boundary(45));
            assert_eq!(
                bus.events,
                attempted_prefix(
                    reset::expected_schedule(0xabcd_4000, &bus.elapsed, 45),
                    failed + 1
                )
            );
            let failed_access = bus.transfers()[failed].0;
            assert_failure(&cpu, &result, injection, failed_access, Direction::Read);
            assert_eq!(bus.transfers, failed + 1);
            assert!(
                !bus.events
                    .iter()
                    .any(|event| matches!(event, Event::Write(..)))
            );
            assert_eq!(
                cpu.clock().0,
                45 + 14
                    + bus.elapsed[..=failed]
                        .iter()
                        .map(|&n| u64::from(n))
                        .sum::<u64>()
                    + if failed == 5 { 2 } else { 0 }
            );
            assert_eq!(cpu.clock().0, bus.clock);
            assert_eq!(cpu.registers, initial);
            assert_eq!(cpu.queued_words(), None);
            guarded(&mut cpu, &mut bus, injection != Injection::Host);
        }
    }
}

#[test]
fn odd_critical_addresses_use_detection_and_never_aligned_data_strobes() {
    for reset in [false, true] {
        for odd_stack in [false, true] {
            if reset && odd_stack {
                continue;
            }
            for unavailable in [false, true] {
                let mut initial = registers();
                if odd_stack {
                    initial.ssp |= 1;
                }
                let mut cpu = if reset {
                    PrefetchCpu::from_prefetched(initial, [0x4e71; 2], CpuClocks(10)).unwrap()
                } else {
                    imported(initial, capture(), 10)
                };
                let mut bus = Device::new(10);
                bus.detect_supported = !unavailable;
                let handler = if odd_stack { 0x2000 } else { 0x2001 };
                if reset {
                    bus.reset_vectors(0x3000, handler);
                } else {
                    bus.handler(2, handler);
                }
                let result = if reset {
                    cpu.external_warm_reset(&mut bus, reset_boundary(10))
                } else {
                    cpu.enter_imported_group0(&mut bus)
                };
                let address = if odd_stack { initial.ssp - 2 } else { handler };
                let direction = if odd_stack {
                    Direction::Write
                } else {
                    Direction::Read
                };
                let mut odd_access = access(
                    address,
                    if odd_stack {
                        FunctionCode::SupervisorData
                    } else {
                        FunctionCode::SupervisorProgram
                    },
                    if odd_stack {
                        AccessKind::Stack
                    } else {
                        AccessKind::Prefetch
                    },
                );
                odd_access.bus_address &= !1;
                let cause = CriticalFault::AddressError {
                    access: odd_access,
                    direction,
                };
                assert_eq!(
                    result,
                    Err(if unavailable {
                        EntryFault::TimingUnavailable(cause)
                    } else {
                        EntryFault::DoubleFaultHalted(cause)
                    })
                );
                assert_eq!(
                    bus.transfers,
                    if odd_stack {
                        0
                    } else if reset {
                        4
                    } else {
                        9
                    }
                );
                assert_eq!(
                    bus.events
                        .iter()
                        .filter(|event| matches!(event, Event::Detect(..)))
                        .count(),
                    usize::from(!unavailable)
                );
                if !unavailable {
                    assert_eq!(
                        bus.events.last(),
                        Some(&Event::Detect(odd_access, direction, cpu.clock().0 - 9))
                    );
                }
                assert_eq!(
                    cpu.clock().0,
                    10 + if odd_stack {
                        4
                    } else if reset {
                        30
                    } else {
                        40
                    } + if unavailable { 0 } else { 9 }
                );
                assert_eq!(cpu.clock().0, bus.clock);
                assert_eq!(cpu.registers, initial);
                assert_eq!(cpu.physical_inspection(), PhysicalInspection::Unavailable);
            }
        }
    }
}

#[test]
fn malformed_complete_elapsed_is_consumed_and_fails_closed() {
    for reset in [false, true] {
        let mut cpu = if reset {
            PrefetchCpu::default()
        } else {
            imported(registers(), capture(), 0)
        };
        let mut bus = Device::new(0);
        bus.reset_vectors(0x3000, 0x2000);
        bus.inject = Some((0, Injection::ShortComplete));
        let result = if reset {
            cpu.external_warm_reset(&mut bus, reset_boundary(0))
        } else {
            cpu.enter_imported_group0(&mut bus)
        };
        assert_eq!(
            result,
            Err(EntryFault::InvalidCompleteDuration { elapsed: 3 })
        );
        assert_eq!(bus.transfers, 1);
        assert_eq!(cpu.clock().0, bus.clock);
        guarded(&mut cpu, &mut bus, false);
    }
}

#[test]
fn upper_bound_capacity_is_reserved_before_callbacks() {
    for reset in [false, true] {
        for available in [0, 3, 13, u64::from(u32::MAX) - 1] {
            let clock = u64::MAX - available;
            let mut cpu = if reset {
                PrefetchCpu::from_prefetched(registers(), [0x4e71; 2], CpuClocks(clock)).unwrap()
            } else {
                imported(registers(), capture(), clock)
            };
            let mut bus = Device::new(clock);
            let result = if reset {
                cpu.external_warm_reset(&mut bus, reset_boundary(clock))
            } else {
                cpu.enter_imported_group0(&mut bus)
            };
            assert_eq!(result, Err(EntryFault::ClockOverflow));
            assert_eq!(bus.transfers, 0);
            assert_eq!(cpu.clock().0, bus.clock);
            assert!(
                bus.events
                    .iter()
                    .all(|event| matches!(event, Event::Idle(..)))
            );
        }
    }
    let origin = u64::MAX - u64::from(u32::MAX) - 4;
    let mut cpu = imported(registers(), capture(), origin);
    let mut bus = Device::new(origin);
    bus.elapsed[0] = u32::MAX;
    bus.inject = Some((0, Injection::Host));
    assert!(matches!(
        cpu.enter_imported_group0(&mut bus),
        Err(EntryFault::HostFailure { .. })
    ));
    assert_eq!(cpu.clock(), CpuClocks(u64::MAX));
    assert_eq!(bus.clock, u64::MAX);
}

#[test]
fn invalid_capture_is_rejected_before_an_engine_exists() {
    for (changed, expected) in [
        (
            Group0Capture {
                vector: 4,
                ..capture()
            },
            EntryInputError::InvalidVector,
        ),
        (
            Group0Capture {
                phase_sr: 0xffff,
                ..capture()
            },
            EntryInputError::InvalidStatus,
        ),
        (
            Group0Capture {
                function_code: FunctionCode::CpuSpace,
                ..capture()
            },
            EntryInputError::UnsupportedFunctionCode,
        ),
    ] {
        assert_eq!(
            PrefetchCpu::from_group0_boundary(ImportedGroup0Boundary {
                registers: registers(),
                capture: changed,
                clock: CpuClocks(0)
            }),
            Err(expected)
        );
    }
}

#[test]
fn legacy_host_failure_retains_four_plus_wait_and_never_becomes_physical_abort() {
    let initial = registers();
    let mut cpu = PrefetchCpu::from_prefetched(initial, [0x4e71; 2], CpuClocks(37)).unwrap();
    let mut bus = Device::new(37);
    bus.elapsed[0] = 17;
    bus.inject = Some((0, Injection::Host));
    assert!(matches!(
        cpu.step_instruction(&mut bus),
        Err(TimingFault::Bus { source: "host", .. })
    ));
    assert_eq!(cpu.clock(), CpuClocks(54));
    assert_eq!(bus.clock, 54);
    assert_eq!(cpu.physical_entry_state(), PhysicalEntryState::Idle);
    assert_eq!(cpu.registers, initial);
    assert_eq!(bus.transfers, 1);
    guarded(&mut cpu, &mut bus, false);
}

#[test]
fn default_address_detection_is_explicitly_unqualified_without_data_effects() {
    let mut initial = registers();
    initial.ssp |= 1;
    let mut cpu = imported(initial, capture(), 17);
    let mut bus = DefaultDetection(Device::new(17));
    assert!(matches!(
        cpu.enter_imported_group0(&mut bus),
        Err(EntryFault::TimingUnavailable(
            CriticalFault::AddressError { .. }
        ))
    ));
    assert_eq!(bus.0.events, vec![Event::Idle(17, 4)]);
    assert_eq!(cpu.clock(), CpuClocks(21));
    assert_eq!(cpu.physical_inspection(), PhysicalInspection::Unavailable);
    assert!(matches!(
        cpu.physical_entry_state(),
        PhysicalEntryState::DoubleFaultHalted {
            timing: TimingQualification::Unavailable,
            ..
        }
    ));
}

#[test]
fn detection_complete_and_host_failure_keep_distinct_terminal_states() {
    for (reset, odd_stack) in [(false, true), (false, false), (true, false)] {
        for host in [false, true] {
            let mut initial = registers();
            if odd_stack {
                initial.ssp |= 1;
            }
            let mut cpu = if reset {
                PrefetchCpu::from_prefetched(initial, [0x4e71; 2], CpuClocks(17)).unwrap()
            } else {
                imported(initial, capture(), 17)
            };
            let mut bus = Device::new(17);
            bus.detect_elapsed = 13;
            bus.detect_outcome = if host { Some(Injection::Host) } else { None };
            if reset {
                bus.reset_vectors(0x3000, 0x2001);
            } else {
                bus.handler(2, 0x2001);
            }
            let result = if reset {
                cpu.external_warm_reset(&mut bus, reset_boundary(17))
            } else {
                cpu.enter_imported_group0(&mut bus)
            };
            let address = if odd_stack { initial.ssp - 2 } else { 0x2001 };
            let direction = if odd_stack {
                Direction::Write
            } else {
                Direction::Read
            };
            let mut detected = access(
                address,
                if odd_stack {
                    FunctionCode::SupervisorData
                } else {
                    FunctionCode::SupervisorProgram
                },
                if odd_stack {
                    AccessKind::Stack
                } else {
                    AccessKind::Prefetch
                },
            );
            detected.bus_address &= !1;
            let cause = CriticalFault::AddressError {
                access: detected,
                direction,
            };
            let expected = if host {
                EntryFault::HostFailure {
                    access: detected,
                    direction,
                    source: "host",
                }
            } else {
                EntryFault::DoubleFaultHalted(cause)
            };
            assert_eq!(result, Err(expected));
            assert_eq!(
                bus.transfers,
                if odd_stack {
                    0
                } else if reset {
                    4
                } else {
                    9
                }
            );
            let detection_start = 17
                + if odd_stack {
                    4
                } else if reset {
                    30
                } else {
                    40
                };
            assert_eq!(
                bus.events.last(),
                Some(&Event::Detect(detected, direction, detection_start))
            );
            assert!(
                !bus.transfers()
                    .iter()
                    .any(|(access, _, _)| access.logical_address == address
                        || access.logical_address == address & !1)
            );
            assert_eq!(cpu.clock(), CpuClocks(detection_start + 13));
            assert_eq!(cpu.clock().0, bus.clock);
            assert_eq!(cpu.registers, initial);
            assert_eq!(cpu.queued_words(), None);
            if host {
                assert_eq!(
                    cpu.physical_entry_state(),
                    PhysicalEntryState::Critical(if reset {
                        EntryKind::ExternalWarmReset
                    } else {
                        EntryKind::ImportedGroup0
                    })
                );
                assert_eq!(
                    cpu.physical_inspection(),
                    PhysicalInspection::DiagnosticCompleteValues
                );
            } else {
                assert_eq!(
                    cpu.physical_entry_state(),
                    PhysicalEntryState::DoubleFaultHalted {
                        cause,
                        timing: TimingQualification::Qualified
                    }
                );
                assert_eq!(cpu.physical_inspection(), PhysicalInspection::Unavailable);
            }
            guarded(&mut cpu, &mut bus, !host);
        }
    }
}

#[test]
fn maximal_completed_write_survives_next_capacity_rejection_without_retry() {
    let initial = registers();
    let origin = u64::MAX - u64::from(u32::MAX) - 4;
    let mut cpu = imported(initial, capture(), origin);
    let mut bus = Device::new(origin);
    bus.elapsed[0] = u32::MAX;
    assert_eq!(
        cpu.enter_imported_group0(&mut bus),
        Err(EntryFault::ClockOverflow)
    );
    let written = access(
        initial.ssp - 2,
        FunctionCode::SupervisorData,
        AccessKind::Stack,
    );
    assert_eq!(
        bus.events,
        vec![
            Event::Idle(origin, 4),
            Event::Write(written, 0x3210, origin + 4)
        ]
    );
    assert_eq!(bus.words[&written.bus_address], 0x3210);
    assert_eq!(bus.transfers, 1);
    assert_eq!(cpu.clock(), CpuClocks(u64::MAX));
    assert_eq!(bus.clock, u64::MAX);
    assert_eq!(cpu.registers, initial);
    assert_eq!(cpu.queued_words(), None);
    let before = bus.events.clone();
    assert_eq!(
        cpu.enter_imported_group0(&mut bus),
        Err(EntryFault::HostFrozen)
    );
    assert_eq!(bus.events, before);
    guarded(&mut cpu, &mut bus, false);
}
