use super::*;

#[test]
fn all_360_positions_cover_outcomes_banks_every_ccr_and_distinct_actual_source_values() {
    let mut positions = 0;
    let mut checked = 0;
    for form in forms() {
        for at in 0..form.count() {
            positions += 1;
            for supervisor in [false, true] {
                for ccr in 0..32 {
                    for value in [0, 0x0001_0000, 0x0000_8000, 0x8000_0001] {
                        for failure in [Failure::Host, Failure::Boundary, Failure::Unavailable] {
                            let mut case = Case::new(form);
                            case.r = registers(supervisor, ccr);
                            case.source = 7;
                            case.destination = 7;
                            case.value = value;
                            let (_, bus, _) = case.seed();
                            let mask = match form.size {
                                Size::Byte => 255,
                                Size::Word => 65535,
                                Size::Long => u32::MAX,
                            };
                            assert_eq!(oracle::value(case, &bus.words), value & mask);
                            failed(case, at, failure);
                            checked += 1;
                        }
                    }
                }
            }
        }
    }
    assert_eq!(positions, 360);
    assert_eq!(checked, 276_480);
}

#[test]
fn aliased_source_index_a7_faults_keep_old_index_bank_and_exact_phase_ssp() {
    for size in [Size::Byte, Size::Word, Size::Long] {
        for source in [Source::Indexed, Source::PcIndexed] {
            for supervisor in [false, true] {
                for long in [false, true] {
                    let form = Form {
                        size,
                        source,
                        destination: Destination::Pre,
                    };
                    for at in 0..form.count() {
                        let mut case = Case::new(form);
                        case.r = registers(supervisor, 31);
                        case.source = 7;
                        case.destination = 7;
                        case.extension = 0xf6f0 | if long { 0x800 } else { 0 };
                        failed(case, at, Failure::Boundary);
                    }
                }
            }
        }
    }
}

#[test]
fn unknown_retained_actual_next_allows_earlier_o_and_consumed_n_blocks_all_apis() {
    for form in forms() {
        let mut case = Case::new(form);
        case.next = 0x4afc;
        for at in 0..form.count() {
            let (mut cpu, mut bus, _) = case.seed();
            let plan = oracle::plan(case, &bus.words);
            let t = plan[at];
            if t.ird != 0x4afc {
                failed(case, at, Failure::Boundary);
                continue;
            }
            let mut memory = bus.words.clone();
            if form.extensions() != 0 {
                let address = case.r.pc.wrapping_add(form.length());
                bus.word(address, 0x7201);
                bus.mutate_read = Some((address, 0x4afc));
                memory.insert(address & BUS_ADDRESS_MASK & !1, 0x4afc);
            }
            let memory = oracle::memory(memory, &plan[..=at]);
            bus.failure = Some((at, Failure::Boundary));
            let cause = CriticalFault::BusAbort {
                access: t.access,
                direction: oracle::direction(t),
            };
            assert_eq!(
                cpu.step_physical_instruction(&mut bus),
                Err(InstructionFault::CaptureUnavailable {
                    phase: t.phase,
                    cause
                })
            );
            assert_eq!(cpu.registers, t.registers);
            assert_eq!(bus.words, memory);
            assert_eq!(cpu.queued_words(), None);
            assert_eq!(
                bus.events,
                oracle::events(case, &plan[..=at], &bus.elapsed, true)
            );
            assert_eq!(bus.count, at + 1);
            assert_eq!(bus.entry_count, 0);
            assert_eq!(
                cpu.clock().0,
                case.start + oracle::clocks(case, &bus.elapsed, at + 1, true)
            );
            assert_eq!(cpu.clock().0, bus.clock);
            assert_eq!(
                cpu.physical_entry_state(),
                PhysicalEntryState::FaultUnqualified { cause }
            );
            assert_eq!(cpu.physical_inspection(), PhysicalInspection::Unavailable);
            guarded(&mut cpu, &mut bus, TimingFault::PhysicalFaultUnqualified);
            let before = cpu.clone();
            let events = bus.events.clone();
            let memory = bus.words.clone();
            let clock = bus.clock;
            assert_eq!(
                cpu.enter_imported_group0(&mut bus),
                Err(EntryFault::FaultUnqualified(cause))
            );
            assert_eq!(cpu, before);
            assert_eq!(bus.events, events);
            assert_eq!(bus.words, memory);
            assert_eq!(bus.clock, clock);
        }
        completion::completed(case, false);
    }
}

#[test]
fn retained_next_survives_exception_stack_overlapping_the_source_code_stream() {
    for form in forms() {
        let mut case = Case::new(form);
        case.r = registers(true, 31);
        case.r.pc = 0x4000;
        case.r.ssp = 0x4000 + form.length() + 2;
        case.next = 0x7602;
        let (mut cpu, mut bus, _) = case.seed();
        let initial = bus.words.clone();
        let plan = oracle::plan(case, &initial);
        let last = plan.len() - 1;
        let t = plan[last];
        let c = oracle::capture(t, 2);
        assert_eq!(c.ird, 0x7602);
        bus.failure = Some((last, Failure::Boundary));
        let clocks = oracle::clocks(case, &bus.elapsed, plan.len(), true);
        assert_eq!(
            cpu.step_physical_instruction(&mut bus),
            Ok(PhysicalInstructionStep::Group0 {
                opcode: form.opcode(case.source, case.destination),
                capture: c,
                clocks: clocks + 50
            })
        );
        assert_eq!(cpu.registers, oracle::entered(t.registers));
        assert_eq!(
            bus.words,
            oracle::entry_memory(oracle::memory(initial, &plan), t.registers.ssp, c)
        );
        oracle::frame(&bus, t.registers.ssp, c, case.start + clocks);
        assert_eq!(cpu.clock().0, bus.clock);
    }
}

pub(super) fn failed(case: Case, at: usize, failure: Failure) {
    let (mut cpu, mut bus, case) = case.seed();
    bus.elapsed = [7, 11, 5, 13, 9, 6, 8];
    bus.failure = Some((at, failure));
    let plan = oracle::plan(case, &bus.words);
    let t = plan[at];
    let c = oracle::capture(t, 2);
    let memory = oracle::memory(bus.words.clone(), &plan[..=at]);
    let idle = true;
    let clocks = oracle::clocks(case, &bus.elapsed, at + 1, idle);
    let mut events = oracle::events(case, &plan[..=at], &bus.elapsed, idle);
    let result = cpu.step_physical_instruction(&mut bus);
    if failure == Failure::Boundary {
        assert_eq!(
            result,
            Ok(PhysicalInstructionStep::Group0 {
                opcode: case.form.opcode(case.source, case.destination),
                capture: c,
                clocks: clocks + 50
            })
        );
        events.extend(oracle::entry_events(
            t.registers.ssp,
            c,
            case.start + clocks,
        ));
        assert_eq!(cpu.registers, oracle::entered(t.registers));
        assert_eq!(cpu.queued_words(), Some([0x4e71, 0x7201]));
        assert_eq!(bus.words, oracle::entry_memory(memory, t.registers.ssp, c));
        oracle::frame(&bus, t.registers.ssp, c, case.start + clocks);
        assert_eq!(cpu.clock().0, case.start + clocks + 50);
    } else {
        let cause = CriticalFault::BusAbort {
            access: t.access,
            direction: oracle::direction(t),
        };
        assert_eq!(
            result,
            Err(if failure == Failure::Host {
                InstructionFault::Entry(EntryFault::HostFailure {
                    access: t.access,
                    direction: oracle::direction(t),
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
        assert_eq!(cpu.queued_words(), None);
        assert_eq!(bus.words, memory);
        assert_eq!(bus.entry_count, 0);
        assert_eq!(cpu.clock().0, case.start + clocks);
        if failure == Failure::Unavailable {
            assert_eq!(
                cpu.physical_entry_state(),
                PhysicalEntryState::FaultUnqualified { cause }
            );
            assert_eq!(cpu.physical_inspection(), PhysicalInspection::Unavailable);
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
    assert_eq!(bus.count, at + 1);
    assert_eq!(bus.events, events);
    assert_eq!(cpu.clock().0, bus.clock);
}

#[test]
fn secondary_entry_failures_retain_prefix_stores_current_phase_ssp_and_all_state() {
    for supervisor in [false, true] {
        for at in 0..11 {
            for failure in [Failure::Host, Failure::Boundary, Failure::Unavailable] {
                let mut case = Case::new(Form {
                    size: Size::Long,
                    source: Source::Post,
                    destination: Destination::Post,
                });
                case.r = registers(supervisor, 31);
                case.source = 7;
                case.destination = 7;
                let (mut cpu, mut bus, case) = case.seed();
                let plan = oracle::plan(case, &bus.words);
                let last = plan.len() - 1;
                let t = plan[last];
                let c = oracle::capture(t, 2);
                bus.failure = Some((last, Failure::Boundary));
                bus.entry_failure = Some((at, failure));
                let start = case.start + oracle::clocks(case, &bus.elapsed, plan.len(), true);
                let schedule = oracle::entry_events(t.registers.ssp, c, start);
                let end = schedule
                    .iter()
                    .enumerate()
                    .filter(|(_, e)| matches!(e, Event::EntryRead(..) | Event::EntryWrite(..)))
                    .nth(at)
                    .unwrap()
                    .0;
                let mut memory = oracle::memory(bus.words.clone(), &plan);
                for e in &schedule[..=end] {
                    if let Event::EntryWrite(a, v, _) = e {
                        memory.insert(a.bus_address, *v);
                    }
                }
                let (access, direction) = match schedule[end] {
                    Event::EntryRead(access, _) => (access, Direction::Read),
                    Event::EntryWrite(access, _, _) => (access, Direction::Write),
                    _ => unreachable!(),
                };
                let cause = CriticalFault::BusAbort { access, direction };
                let fault = match failure {
                    Failure::Host => EntryFault::HostFailure {
                        access,
                        direction,
                        source: "host",
                    },
                    Failure::Boundary => EntryFault::DoubleFaultHalted(cause),
                    Failure::Unavailable => EntryFault::TimingUnavailable(cause),
                    Failure::Complete => unreachable!(),
                };
                assert_eq!(
                    cpu.step_physical_instruction(&mut bus),
                    Err(InstructionFault::Entry(fault))
                );
                assert_eq!(cpu.registers, t.registers);
                assert_eq!(cpu.queued_words(), None);
                assert_eq!(bus.words, memory);
                let mut events = oracle::events(case, &plan, &bus.elapsed, true);
                events.extend_from_slice(&schedule[..=end]);
                assert_eq!(bus.events, events);
                assert_eq!(bus.entry_count, at + 1);
                assert_eq!(bus.count, plan.len());
                assert_eq!(
                    cpu.clock().0,
                    start + 4 + (at as u64 + 1) * 4 + if at == 10 { 2 } else { 0 }
                );
                assert_eq!(cpu.clock().0, bus.clock);
                if failure != Failure::Host {
                    assert_eq!(
                        cpu.physical_entry_state(),
                        PhysicalEntryState::DoubleFaultHalted {
                            cause,
                            timing: if failure == Failure::Boundary {
                                TimingQualification::Qualified
                            } else {
                                TimingQualification::Unavailable
                            }
                        }
                    );
                    assert_eq!(cpu.physical_inspection(), PhysicalInspection::Unavailable);
                } else {
                    assert_eq!(
                        cpu.physical_entry_state(),
                        PhysicalEntryState::Critical(
                            zeff_m68k::timing::physical::EntryKind::ImportedGroup0
                        )
                    );
                    assert_eq!(
                        cpu.physical_inspection(),
                        PhysicalInspection::DiagnosticCompleteValues
                    );
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
}

#[test]
fn negative_byte_source_values_reach_every_fault_phase_and_preserve_x_and_lanes() {
    let mut checked = 0;
    for form in forms().into_iter().filter(|f| f.size == Size::Byte) {
        for supervisor in [false, true] {
            for value in [0x80, 0xff] {
                for at in 0..form.count() {
                    for failure in [Failure::Host, Failure::Boundary, Failure::Unavailable] {
                        let mut case = Case::new(form);
                        case.r = registers(supervisor, 31);
                        case.source = 0;
                        case.destination = 0;
                        case.value = value;
                        failed(case, at, failure);
                        checked += 1;
                    }
                }
            }
        }
    }
    assert_eq!(checked, 1224);
}

#[test]
fn primary_source_prefetch_berr_with_unrelated_odd_ssp_reaches_actual_secondary_detection() {
    for supervisor in [false, true] {
        let form = Form {
            size: Size::Long,
            source: Source::PcIndexed,
            destination: Destination::Post,
        };
        let mut case = Case::new(form);
        case.r = registers(supervisor, 31);
        case.r.ssp |= 1;
        let (mut cpu, mut bus, _) = case.seed();
        let memory = bus.words.clone();
        let plan = oracle::plan(case, &memory);
        let t = plan[0];
        let c = oracle::capture(t, 2);
        bus.failure = Some((0, Failure::Boundary));
        let clocks = oracle::clocks(case, &bus.elapsed, 1, true);
        assert!(matches!(
            cpu.step_physical_instruction(&mut bus),
            Err(InstructionFault::Entry(EntryFault::TimingUnavailable(_)))
        ));
        assert_eq!(cpu.registers, t.registers);
        assert_eq!(bus.words, memory);
        assert_eq!(bus.entry_count, 0);
        assert_eq!(bus.count, 1);
        let mut events = oracle::events(case, &plan[..1], &bus.elapsed, true);
        events.extend([
            Event::Idle(case.start + clocks, 4),
            Event::EntryDetect(
                oracle::access(
                    t.registers.ssp.wrapping_sub(2),
                    true,
                    false,
                    AccessKind::Stack,
                ),
                Direction::Write,
                case.start + clocks + 4,
            ),
        ]);
        assert_eq!(bus.events, events);
        assert_eq!(c.phase_sr, case.r.status_register());
        assert_eq!(cpu.clock().0, case.start + clocks + 4);
        assert_eq!(cpu.clock().0, bus.clock);
        assert!(matches!(
            cpu.physical_entry_state(),
            PhysicalEntryState::DoubleFaultHalted { .. }
        ));
        guarded(&mut cpu, &mut bus, TimingFault::DoubleFaultHalted);
    }
}

#[test]
fn group0_stack_publication_retains_full_32_bit_ssp_and_24_bit_wrap_addresses() {
    for supervisor in [false, true] {
        for sp in [2, 0x0100_0002] {
            let mut case = Case::new(Form {
                size: Size::Long,
                source: Source::Post,
                destination: Destination::Post,
            });
            case.r = registers(supervisor, 31);
            case.r.ssp = sp;
            case.source = 0;
            case.destination = 1;
            failed(case, 0, Failure::Boundary);
        }
    }
}
