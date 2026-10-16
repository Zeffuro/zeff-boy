use super::*;
use device::Kind;
use model::Spec;
use zeff_m68k::timing::physical::ImportedGroup0Boundary;

fn rejected(
    cpu: &mut PrefetchCpu,
    bus: &mut Device,
    fault: InstructionFault<&'static str>,
) -> bool {
    let before = cpu.clone();
    let device = bus.clone();
    cpu.step_physical_instruction(bus) == Err(fault) && *cpu == before && *bus == device
}

fn unsupported(s: Spec, word: u16) -> bool {
    let (_, mut bus) = s.seed();
    let mut cpu =
        PrefetchCpu::from_prefetched(s.r, [word, s.extension], CpuClocks(s.start)).unwrap();
    rejected(
        &mut cpu,
        &mut bus,
        InstructionFault::Execution(TimingFault::UnsupportedInstruction {
            pc: s.r.pc,
            opcode: word,
        }),
    )
}

fn legal(word: u16) -> bool {
    let mode = word >> 3 & 7;
    matches!(word & 0xffc0, 0x44c0 | 0x46c0)
        && (mode == 0 || matches!(mode, 2..=6) || mode == 7 && word & 7 <= 4)
}

fn prepared(s: Spec, gate: u8) -> (PrefetchCpu, Device, InstructionFault<&'static str>) {
    let (mut cpu, mut bus) = s.seed();
    let fault = match gate {
        0 | 1 | 9 => {
            let mut r = s.r;
            r.set_status_register(if gate == 0 {
                r.status_register() | 0x8000
            } else if gate == 1 {
                0x271b
            } else {
                0xa71b
            });
            let opcode = if gate == 0 { 0x4e71 } else { 0x4e72 };
            let next = if gate == 0 {
                s.opcode()
            } else {
                s.r.status_register()
            };
            cpu = PrefetchCpu::from_prefetched(r, [opcode, next], CpuClocks(s.start)).unwrap();
            bus.word(r.pc.wrapping_add(4), s.extension);
            cpu.step_instruction(&mut bus).unwrap();
            cpu.registers = s.r;
            InstructionFault::Execution(if gate == 1 {
                TimingFault::Stopped
            } else {
                TimingFault::PendingTrace
            })
        }
        2 => {
            cpu.interrupt_input().set_level(7);
            cpu.interrupt_input().set_level(0);
            InstructionFault::InterruptUnqualified
        }
        3 => {
            cpu = PrefetchCpu::default();
            cpu.registers = s.r;
            InstructionFault::Execution(TimingFault::Unprimed)
        }
        4 => {
            cpu.registers.pc = cpu.registers.pc.wrapping_add(2);
            InstructionFault::Execution(TimingFault::Unprimed)
        }
        5..=7 => {
            bus.report = Some((
                0,
                if gate == 5 {
                    Kind::Host
                } else if gate == 6 {
                    Kind::Boundary
                } else {
                    Kind::Unavailable
                },
                7,
            ));
            if gate == 6 {
                bus.entry_failure = Some((0, Kind::Boundary, 7));
            }
            let result = cpu.step_physical_instruction(&mut bus);
            assert!(result.is_err());
            InstructionFault::Execution(match gate {
                5 => TimingFault::Frozen,
                6 => TimingFault::DoubleFaultHalted,
                _ => TimingFault::PhysicalFaultUnqualified,
            })
        }
        8 => {
            cpu = PrefetchCpu::from_group0_boundary(ImportedGroup0Boundary {
                registers: s.r,
                clock: CpuClocks(s.start),
                capture: Group0Capture {
                    vector: 2,
                    internal_pc: s.r.pc.wrapping_add(2),
                    phase_sr: s.r.status_register(),
                    ird: s.opcode(),
                    logical_address: model::active(s.r, s.reg),
                    function_code: if s.r.status_register() & 0x2000 == 0 {
                        FunctionCode::UserData
                    } else {
                        FunctionCode::SupervisorData
                    },
                    direction: Direction::Read,
                    processing_class: ProcessingClass::InstructionOrGroup2,
                    ssw_profile: SswProfile::SelectedIrdUpperBits,
                },
            })
            .unwrap();
            InstructionFault::Execution(TimingFault::ImportedBoundaryPending)
        }
        _ => unreachable!(),
    };
    (cpu, bus, fault)
}

pub(super) fn old_trace() -> (u32, u32) {
    let (mut count, mut bad) = (0, 0);
    for mut s in model::encoded() {
        for ccr in 0..32 {
            for mask in 0..8 {
                s.r.set_status_register(
                    0x8000 | if s.context == 0 { 0 } else { 0x2000 } | mask << 8 | ccr,
                );
                let (mut cpu, mut bus) = s.seed();
                count += 1;
                bad += u32::from(!rejected(
                    &mut cpu,
                    &mut bus,
                    InstructionFault::TraceUnqualified,
                ));
            }
        }
    }
    (count, bad)
}

pub(super) fn user_sr() -> (u32, u32) {
    let (mut count, mut bad) = (0, 0);
    for mut s in model::encoded().into_iter().filter(|s| s.context == 2) {
        for ccr in 0..32 {
            for mask in 0..8 {
                s.r.set_status_register(mask << 8 | ccr);
                count += 1;
                bad += u32::from(!unsupported(s, s.opcode()));
            }
        }
    }
    (count, bad)
}

pub(super) fn preflight() -> (u32, u32) {
    let (mut count, mut bad) = (0, 0);
    for s in model::encoded() {
        for gate in 0..9 {
            let (mut cpu, mut bus, expected) = prepared(s, gate);
            count += 1;
            bad += u32::from(!rejected(&mut cpu, &mut bus, expected));
        }
    }
    (count, bad)
}

pub(super) fn priority() -> (u32, u32) {
    let (mut count, mut bad) = (0, 0);
    for s in model::encoded() {
        for pair in 0..8 {
            let gate = [5, 6, 7, 8, 9, 1, 3, 3][pair];
            let (mut cpu, mut bus, mut expected) = prepared(s, gate);
            cpu.registers
                .set_status_register(cpu.registers.status_register() | 0x8000);
            cpu.interrupt_input().set_level(7);
            cpu.interrupt_input().set_level(0);
            if pair == 6 {
                expected = InstructionFault::TraceUnqualified;
            }
            if pair == 7 {
                cpu.registers
                    .set_status_register(cpu.registers.status_register() & 0x7fff);
                expected = InstructionFault::InterruptUnqualified;
            }
            count += 1;
            bad += u32::from(!rejected(&mut cpu, &mut bus, expected));
        }
    }
    (count, bad)
}

pub(super) fn next_boundary() -> (u32, u32) {
    let (mut count, mut bad) = (0, 0);
    for base in model::encoded().into_iter().filter(|s| s.context == 2) {
        for mask in 0..8 {
            for pattern in 0..9 {
                for upper in [0, 0x2000, 0x8000, 0xa000] {
                    let mut s = base;
                    s.word = upper | mask << 8 | 0x1f;
                    let (mut cpu, mut bus) = s.seed();
                    if pattern < 7 {
                        cpu.interrupt_input().set_level(pattern);
                    }
                    let plan = model::plan(s, &bus);
                    let expected = oracle::predict(s, &bus, &plan);
                    let actual = cpu.step_physical_instruction(&mut bus);
                    let mut good = oracle::verify(&mut cpu, &mut bus, expected, actual);
                    if pattern >= 7 {
                        cpu.interrupt_input().set_level(7);
                        if pattern == 8 {
                            cpu.interrupt_input().set_level(0);
                        }
                    }
                    if upper & 0x8000 != 0 {
                        good &= rejected(&mut cpu, &mut bus, InstructionFault::TraceUnqualified);
                    } else if pattern >= 7 || u16::from(pattern) > mask {
                        good &=
                            rejected(&mut cpu, &mut bus, InstructionFault::InterruptUnqualified);
                    } else {
                        bus.count = 0;
                        let before = cpu.registers;
                        let old = bus.clone();
                        let at = before.pc.wrapping_add(4);
                        let value = old.sample(at);
                        let result = cpu.step_physical_instruction(&mut bus);
                        let mut r = before;
                        r.pc = r.pc.wrapping_add(2);
                        let mut events = old.events;
                        events.push(Event::Read(
                            access(
                                at,
                                if upper & 0x2000 == 0 { 2 } else { 6 },
                                AccessKind::Prefetch,
                            ),
                            old.clock,
                            value,
                        ));
                        good &= result
                            == Ok(PhysicalInstructionStep::Instruction(TimedStep {
                                opcode: 0x4e71,
                                clocks: 4,
                                exception: None,
                            }))
                            && cpu.registers == r
                            && cpu.queued_words() == Some([s.last, value])
                            && cpu.clock().0 == old.clock + 4
                            && bus.clock == old.clock + 4
                            && bus.count == 1
                            && bus.entry_count == old.entry_count
                            && bus.events == events
                            && bus.words == old.words
                            && !cpu.trace_pending()
                            && cpu.physical_entry_state() == PhysicalEntryState::Idle;
                    }
                    count += 1;
                    bad += u32::from(!good);
                }
            }
        }
    }
    (count, bad)
}

fn admitted_status(s: Spec, word: u16) -> bool {
    let (_, mut bus) = s.seed();
    let mut expected_bus = bus.clone();
    let immediate = word & 63 == 60;
    let value = if immediate {
        s.extension
    } else {
        s.r.d[usize::from(word & 7)] as u16
    };
    let mut r = s.r;
    r.set_status_register(if word & 0xffc0 == 0x46c0 {
        value & 0xa71f
    } else {
        (s.r.status_register() & 0xffe0) | (value & 31)
    });
    let anchor = s.r.pc.wrapping_add(if immediate { 4 } else { 2 });
    let first = bus.sample(anchor);
    let last = bus.sample(anchor.wrapping_add(2));
    let fc = if r.supervisor() { 6 } else { 2 };
    let before = if immediate { 4 } else { 0 };
    if immediate {
        let old_fc = if s.r.supervisor() { 6 } else { 2 };
        expected_bus.events.push(Event::Read(
            access(anchor, old_fc, AccessKind::Prefetch),
            0,
            first,
        ));
    }
    expected_bus.events.extend([
        Event::Idle(before, 2),
        Event::Idle(before + 2, 2),
        Event::Read(access(anchor, fc, AccessKind::Prefetch), before + 4, first),
        Event::Read(
            access(anchor.wrapping_add(2), fc, AccessKind::Prefetch),
            before + 8,
            last,
        ),
    ]);
    expected_bus.count = if immediate { 3 } else { 2 };
    expected_bus.clock = before + 12;
    r.pc = anchor;
    let expected = PrefetchCpu::from_prefetched(r, [first, last], CpuClocks(before + 12)).unwrap();
    let mut cpu =
        PrefetchCpu::from_prefetched(s.r, [word, s.extension], CpuClocks(s.start)).unwrap();
    cpu.step_physical_instruction(&mut bus)
        == Ok(PhysicalInstructionStep::Instruction(TimedStep {
            opcode: word,
            clocks: before + 12,
            exception: None,
        }))
        && cpu == expected
        && bus == expected_bus
}

pub(super) fn processing_only() -> (u32, u32) {
    let (mut count, mut bad) = (0, 0);
    for family in [0x44c0, 0x46c0] {
        for ea in 0..64 {
            let word = family | ea;
            if !legal(word) || !(ea < 8 || ea == 60) {
                continue;
            }
            for context in 0..2 {
                count += 1;
                let s = Spec::new(context, 0, 0);
                let good = if family == 0x46c0 && context == 0 {
                    unsupported(s, word)
                } else {
                    admitted_status(s, word)
                };
                bad += u32::from(!good);
            }
        }
    }
    (count, bad)
}

pub(super) fn illegal() -> (u32, u32) {
    let (mut count, mut bad) = (0, 0);
    for family in [0x44c0, 0x46c0] {
        for ea in 0..64 {
            let word = family | ea;
            if legal(word) {
                continue;
            }
            for context in 0..2 {
                count += 1;
                bad += u32::from(!unsupported(Spec::new(context, 0, 0), word));
            }
        }
    }
    (count, bad)
}

pub(super) fn other_status() -> (u32, u32) {
    let (mut count, mut bad) = (0, 0);
    for family in [0x40c0, 0x42c0] {
        for ea in 0..64 {
            for context in 0..2 {
                count += 1;
                let s = Spec::new(context, 0, 0);
                let word = family | ea;
                let mode = word >> 3 & 7;
                let from_sr = family == 0x40c0
                    && (mode == 0 || matches!(mode, 2..=6) || mode == 7 && word & 7 <= 1);
                let good = if from_sr {
                    let (_, mut bus) = s.seed();
                    let old = bus.clone();
                    let mut cpu =
                        PrefetchCpu::from_prefetched(s.r, [word, s.extension], CpuClocks(u64::MAX))
                            .unwrap();
                    cpu.step_physical_instruction(&mut bus)
                        == Err(InstructionFault::Entry(EntryFault::ClockOverflow))
                        && cpu.registers == s.r
                        && cpu.queued_words() == Some([word, s.extension])
                        && cpu.clock() == CpuClocks(u64::MAX)
                        && bus == old
                        && check::freeze(&mut cpu, &mut bus, TimingFault::Frozen)
                } else {
                    unsupported(s, word)
                };
                bad += u32::from(!good);
            }
        }
    }
    (count, bad)
}

pub(super) fn actual_r1() -> (u32, u32) {
    let (mut count, mut bad) = (0, 0);
    for base in model::forms() {
        for first in [
            0x44c0, 0x44fc, 0x46c0, 0x46fc, 0x44d0, 0x46d0, 0x44c8, 0xffff,
        ] {
            for kind in [Kind::Host, Kind::Boundary, Kind::Unavailable] {
                let mut s = base;
                s.first = first;
                let (_, mut bus) = s.seed();
                bus.report = Some((s.count() - 1, kind, 7));
                count += 1;
                bad += u32::from(!oracle::run(s, bus));
            }
        }
    }
    (count, bad)
}

pub(super) fn classifier() -> (u32, u32) {
    let (mut count, mut bad, mut recognized) = (0, 0, 0);
    for family in [0x44c0, 0x46c0] {
        for ea in 0..64 {
            let word = family | ea;
            recognized += u32::from(legal(word));
            for context in 0..3 {
                let mut s = Spec::new(context, 0, 0);
                s.first = word;
                let (_, mut bus) = s.seed();
                bus.report = Some((s.count() - 1, Kind::Boundary, 7));
                count += 1;
                bad += u32::from(model::known(word) != legal(word) || !oracle::run(s, bus));
            }
        }
    }
    (count, bad + u32::from(recognized != 106))
}
