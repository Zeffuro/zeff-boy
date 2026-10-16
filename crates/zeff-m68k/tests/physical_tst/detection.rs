use super::*;

fn odd(mut case: Case) -> Case {
    match case.form.ea {
        Ea::Indirect | Ea::Post | Ea::Pre => {
            let value = case.r.address_register(usize::from(case.register)).unwrap();
            case.r
                .set_address_register(usize::from(case.register), value | 1);
        }
        Ea::AbsoluteLong => case.low |= 1,
        _ => case.extension |= 1,
    }
    case
}

#[test]
fn all_14_first_odd_routes_preserve_every_ccr_bank_and_default_or_selected_detection() {
    let mut routes = 0;
    let mut rows = 0;
    for form in forms() {
        routes += 1;
        for supervisor in [false, true] {
            for ccr in 0..32 {
                for value in completion::values(form.size) {
                    for detection in [
                        None,
                        Some((Failure::Complete, 17)),
                        Some((Failure::Host, 17)),
                        Some((Failure::Unavailable, 17)),
                        Some((Failure::Boundary, 0)),
                        Some((Failure::Boundary, 3)),
                        Some((Failure::Boundary, 17)),
                    ] {
                        let mut case = Case::new(form);
                        case.r = registers(supervisor, ccr);
                        case.value = value;
                        case = odd(case);
                        let (mut cpu, mut bus) = case.seed();
                        let initial = bus.words.clone();
                        let plan = oracle::plan(case, &initial);
                        let at = form.extensions();
                        let t = plan[at];
                        assert_eq!(t.access.logical_address & 1, 1);
                        assert_eq!(t.registers.status_register(), case.r.status_register());
                        assert_eq!(t.ird, form.opcode(case.register));
                        let c = oracle::capture(t, 3);
                        let prefix = oracle::clocks(case, &bus.elapsed, at, true);
                        let clocks = prefix + u64::from(detection.map_or(0, |(_, n)| n));
                        let mut events = oracle::events(case, &plan[..at], &bus.elapsed, true);
                        let result = if let Some(report) = detection {
                            bus.detection = Some(report);
                            events.push(Event::Detect(
                                t.access,
                                Direction::Read,
                                case.start + prefix,
                            ));
                            cpu.step_physical_instruction(&mut bus)
                        } else {
                            cpu.step_physical_instruction(&mut DefaultDetection(&mut bus))
                        };
                        assert_eq!(bus.count, at);
                        if detection.is_some_and(|(f, _)| f == Failure::Boundary) {
                            assert_eq!(
                                result,
                                Ok(PhysicalInstructionStep::Group0 {
                                    opcode: form.opcode(case.register),
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
                            assert_eq!(
                                bus.words,
                                oracle::entry_memory(initial, t.registers.ssp, c)
                            );
                            oracle::frame(&bus, t.registers.ssp, c, case.start + clocks);
                            assert_eq!(cpu.clock().0, case.start + clocks + 50);
                        } else {
                            let cause = CriticalFault::AddressError {
                                access: t.access,
                                direction: Direction::Read,
                            };
                            let host = detection.is_some_and(|(f, _)| f == Failure::Host);
                            assert_eq!(
                                result,
                                Err(if host {
                                    InstructionFault::Entry(EntryFault::HostFailure {
                                        access: t.access,
                                        direction: Direction::Read,
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
                            assert_eq!(bus.words, initial);
                            assert_eq!(bus.entry_count, 0);
                            assert_eq!(cpu.clock().0, case.start + clocks);
                            if !host {
                                assert_eq!(
                                    cpu.physical_entry_state(),
                                    PhysicalEntryState::FaultUnqualified { cause }
                                );
                                assert_eq!(
                                    cpu.physical_inspection(),
                                    PhysicalInspection::Unavailable
                                );
                            }
                            guarded(
                                &mut cpu,
                                &mut bus,
                                if host {
                                    TimingFault::Frozen
                                } else {
                                    TimingFault::PhysicalFaultUnqualified
                                },
                            );
                        }
                        assert_eq!(bus.events, events);
                        assert_eq!(cpu.clock().0, bus.clock);
                        rows += 1;
                    }
                }
            }
        }
    }
    assert_eq!(routes, 14);
    assert_eq!(rows, 25_088);
}

struct CriticalDetection<'a>(&'a mut BusDevice, bool);
impl EntryBus for CriticalDetection<'_> {
    type Error = &'static str;
    fn read_entry(&mut self, a: BusAccess, c: CpuClocks) -> EntryReport<u16, Self::Error> {
        self.0.read_entry(a, c)
    }
    fn write_entry(&mut self, a: BusAccess, v: u16, c: CpuClocks) -> EntryReport<(), Self::Error> {
        self.0.write_entry(a, v, c)
    }
    fn idle_entry(&mut self, c: CpuClocks, n: u32) {
        self.0.idle_entry(c, n);
    }
    fn detect_address_error(
        &mut self,
        a: BusAccess,
        d: Direction,
        c: CpuClocks,
    ) -> EntryReport<(), Self::Error> {
        assert_eq!(c.0, self.0.clock);
        self.0.events.push(Event::EntryDetect(a, d, c.0));
        let elapsed = if self.1 { 7 } else { 0 };
        self.0.clock += u64::from(elapsed);
        EntryReport {
            elapsed,
            outcome: if self.1 {
                EntryOutcome::CriticalAbortReleased
            } else {
                EntryOutcome::TimingUnavailable
            },
        }
    }
}
impl InstructionBus for CriticalDetection<'_> {
    fn read_instruction(
        &mut self,
        a: BusAccess,
        c: CpuClocks,
    ) -> InstructionReport<u16, Self::Error> {
        self.0.read_instruction(a, c)
    }
    fn write_instruction(
        &mut self,
        a: BusAccess,
        v: u16,
        c: CpuClocks,
    ) -> InstructionReport<(), Self::Error> {
        self.0.write_instruction(a, v, c)
    }
    fn detect_instruction_address_error(
        &mut self,
        a: BusAccess,
        d: Direction,
        c: CpuClocks,
    ) -> InstructionReport<(), Self::Error> {
        self.0.detect_instruction_address_error(a, d, c)
    }
}

#[test]
fn actual_odd_a7_bank_keeps_source_publication_and_exact_secondary_ssp_qualification() {
    for size in [Size::Word, Size::Long] {
        for ea in [Ea::Indirect, Ea::Post, Ea::Pre] {
            for supervisor in [false, true] {
                for qualified in [false, true] {
                    let mut case = Case::new(Form { size, ea });
                    case.r = registers(supervisor, 31);
                    case.register = 7;
                    case = odd(case);
                    let (mut cpu, mut bus) = case.seed();
                    bus.detection = Some((Failure::Boundary, 8));
                    let initial = bus.words.clone();
                    let plan = oracle::plan(case, &initial);
                    let t = plan[0];
                    assert_eq!(t.access.logical_address & 1, 1);
                    let c = oracle::capture(t, 3);
                    let prefix = u64::from(case.form.idle()) * 2;
                    let clocks = prefix + 8;
                    let result =
                        cpu.step_physical_instruction(&mut CriticalDetection(&mut bus, qualified));
                    let mut events = oracle::events(case, &[], &bus.elapsed, true);
                    events.push(Event::Detect(
                        t.access,
                        Direction::Read,
                        case.start + prefix,
                    ));
                    if supervisor {
                        let access = oracle::access(
                            t.registers.ssp.wrapping_sub(2),
                            true,
                            false,
                            AccessKind::Stack,
                        );
                        let cause = CriticalFault::AddressError {
                            access,
                            direction: Direction::Write,
                        };
                        assert_eq!(
                            result,
                            Err(InstructionFault::Entry(if qualified {
                                EntryFault::DoubleFaultHalted(cause)
                            } else {
                                EntryFault::TimingUnavailable(cause)
                            }))
                        );
                        assert_eq!(cpu.registers, t.registers);
                        assert_eq!(bus.words, initial);
                        assert_eq!(bus.entry_count, 0);
                        events.extend([
                            Event::Idle(case.start + clocks, 4),
                            Event::EntryDetect(access, Direction::Write, case.start + clocks + 4),
                        ]);
                        assert_eq!(
                            cpu.clock().0,
                            case.start + clocks + 4 + if qualified { 7 } else { 0 }
                        );
                        assert_eq!(
                            cpu.physical_entry_state(),
                            PhysicalEntryState::DoubleFaultHalted {
                                cause,
                                timing: if qualified {
                                    TimingQualification::Qualified
                                } else {
                                    TimingQualification::Unavailable
                                }
                            }
                        );
                        assert_eq!(cpu.physical_inspection(), PhysicalInspection::Unavailable);
                        guarded(&mut cpu, &mut bus, TimingFault::DoubleFaultHalted);
                    } else {
                        assert_eq!(
                            result,
                            Ok(PhysicalInstructionStep::Group0 {
                                opcode: case.form.opcode(7),
                                capture: c,
                                clocks: clocks + 50
                            })
                        );
                        assert_eq!(cpu.registers, oracle::entered(t.registers));
                        assert_eq!(bus.words, oracle::entry_memory(initial, t.registers.ssp, c));
                        events.extend(oracle::entry_events(
                            t.registers.ssp,
                            c,
                            case.start + clocks,
                        ));
                        oracle::frame(&bus, t.registers.ssp, c, case.start + clocks);
                        assert_eq!(cpu.clock().0, case.start + clocks + 50);
                    }
                    assert_eq!(
                        cpu.queued_words(),
                        if supervisor {
                            None
                        } else {
                            Some([0x4e71, 0x7201])
                        }
                    );
                    assert_eq!(bus.count, 0);
                    assert_eq!(bus.events, events);
                    assert_eq!(cpu.clock().0, bus.clock);
                }
            }
        }
    }
}
