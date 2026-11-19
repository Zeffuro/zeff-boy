use super::*;
use bulk_device::Device;
use bulk_device::Kind;
use bulk_model::Spec;
use zeff_m68k::timing::physical::ImportedGroup0Boundary;
#[path = "../support/unary_completion.rs"]
mod unary_completion;

pub(super) fn known(w: u16) -> bool {
    let (top, op, mode, reg) = (w >> 12, w >> 6 & 7, w >> 3 & 7, w & 7);
    let data = mode == 0 || matches!(mode, 2..=6) || mode == 7 && reg <= 1;
    let source = mode != 7 || reg <= 4;
    if w == 0x4e71 || w & 0xf100 == 0x7000 || top == 6 {
        return true;
    }
    if matches!(
        w & 0xff00,
        0x4a00 | 0x0c00 | 0x4000 | 0x4200 | 0x4400 | 0x4600
    ) && w >> 6 & 3 < 3
        && data
    {
        return true;
    }
    if w & 0xffc0 == 0x40c0 && data {
        return true;
    }
    if matches!(w & 0xffc0, 0x44c0 | 0x46c0)
        && (mode == 0 || matches!(mode, 2..=6) || mode == 7 && reg <= 4)
    {
        return true;
    }
    if top == 11 && source && (op <= 2 && (op != 0 || mode != 1) || matches!(op, 3 | 7)) {
        return true;
    }
    if matches!(top, 9 | 13) && source && matches!(op, 3 | 7) {
        return true;
    }
    if matches!(top, 8 | 9 | 12 | 13)
        && op <= 2
        && source
        && !(mode == 1 && (op == 0 || matches!(top, 8 | 12)))
    {
        return true;
    }
    matches!(top, 1..=3)
        && source
        && (w >> 6 & 7 != 7 || w >> 9 & 7 <= 1)
        && !(top == 1 && (mode == 1 || w >> 6 & 7 == 1))
}

fn selected(w: u16, supervisor: bool) -> bool {
    let (top, op, mode, reg) = (w >> 12, w >> 6 & 7, w >> 3 & 7, w & 7);
    let memory = matches!(mode, 2..=6) || mode == 7 && reg <= 3;
    let alterable = matches!(mode, 2..=6) || mode == 7 && reg <= 1;
    w == 0x4e71
        || w & 0xf100 == 0x7000
        || w & 0xff00 == 0x6000
        || matches!(top, 1..=3) && known(w)
        || w & 0xff00 == 0x4a00 && w >> 6 & 3 < 3 && (mode == 0 || alterable)
        || w & 0xff00 == 0x0c00 && w >> 6 & 3 < 3 && (mode == 0 || alterable)
        || matches!(w & 0xff00, 0x4000 | 0x4200 | 0x4400 | 0x4600)
            && matches!(w >> 6 & 3, 1 | 2)
            && alterable
        || matches!(w & 0xff00, 0x4000 | 0x4200 | 0x4400 | 0x4600)
            && w >> 6 & 3 < 3
            && (mode == 0 || alterable)
        || top == 11 && known(w)
        || matches!(top, 9 | 13) && matches!(op, 3 | 7) && memory
        || matches!(top, 8 | 9 | 12 | 13) && matches!(op, 1 | 2) && memory
        || (w & 0xffc0 == 0x44c0 || supervisor && w & 0xffc0 == 0x46c0)
            && (mode == 0 || memory || mode == 7 && reg == 4)
        || w & 0xffc0 == 0x40c0 && (mode == 0 || alterable)
}

fn rejected(cpu: &mut PrefetchCpu, b: &mut Device, error: InstructionFault<&'static str>) -> bool {
    let before = cpu.clone();
    let old = b.clone();
    cpu.step_physical_instruction(b) == Err(error) && *cpu == before && *b == old
}

fn unsupported(s: Spec, w: u16) -> bool {
    let (_, mut b) = s.seed();
    let mut cpu = PrefetchCpu::from_prefetched(s.r, [w, s.queued()], CpuClocks(s.start)).unwrap();
    rejected(
        &mut cpu,
        &mut b,
        InstructionFault::Execution(TimingFault::UnsupportedInstruction {
            pc: s.r.pc,
            opcode: w,
        }),
    )
}

pub(super) fn masks() -> (u32, u32) {
    let (mut count, mut bad, mut processing, mut user, mut supervisor) = (0, 0, 0, 0, 0);
    for w in 0..=u16::MAX {
        let mut s = Spec::new(0x4000, 2, 0, 0, false);
        s.first = w;
        let (_, mut b) = s.seed();
        b.report = Some((s.count() - 1, Kind::Boundary, 8));
        let mut good = bulk_oracle::run(s, b);
        processing += u32::from(known(w));
        for bank in [false, true] {
            let mut probe = Spec::new(0x4000, 2, 0, 0, bank);
            probe.start = u64::MAX;
            let (_, mut b) = probe.seed();
            let mut cpu =
                PrefetchCpu::from_prefetched(probe.r, [w, 0x4e71], CpuClocks(u64::MAX)).unwrap();
            if selected(w, bank) {
                if bank {
                    supervisor += 1;
                } else {
                    user += 1;
                }
                let old = b.clone();
                good &= cpu.step_physical_instruction(&mut b)
                    == Err(InstructionFault::Entry(EntryFault::ClockOverflow))
                    && b == old
                    && cpu.clock() == CpuClocks(u64::MAX)
                    && bulk_oracle::freeze(&mut cpu, &mut b, TimingFault::Frozen);
            } else {
                good &= rejected(
                    &mut cpu,
                    &mut b,
                    InstructionFault::Execution(TimingFault::UnsupportedInstruction {
                        pc: probe.r.pc,
                        opcode: w,
                    }),
                );
            }
        }
        count += 1;
        bad += u32::from(!good);
    }
    (
        count,
        bad + u32::from((processing, user, supervisor) != (26599, 19634, 19687)),
    )
}

fn completed_remainder(s: Spec, w: u16) -> bool {
    let (_, mut b) = s.seed();
    let plan = unary_completion::plan(w, s.r, s.queued(), s.start, b.words.clone());
    let mut expected = b.clone();
    expected.clock = plan.clock;
    expected.words = plan.words;
    expected.events = plan
        .events
        .into_iter()
        .map(|event| match event {
            unary_completion::Event::Read(access, value, start) => bulk_device::Event::Transfer(
                bulk_device::Stage::Instruction,
                access,
                Direction::Read,
                value,
                start,
                4,
                Kind::Complete,
                false,
            ),
            unary_completion::Event::Write(access, value, start) => bulk_device::Event::Transfer(
                bulk_device::Stage::Instruction,
                access,
                Direction::Write,
                value,
                start,
                4,
                Kind::Complete,
                true,
            ),
            unary_completion::Event::Idle(start, clocks) => bulk_device::Event::Idle(start, clocks),
        })
        .collect();
    expected.count = expected
        .events
        .iter()
        .filter(|event| matches!(event, bulk_device::Event::Transfer(..)))
        .count();
    let final_cpu =
        PrefetchCpu::from_prefetched(plan.registers, plan.queue, CpuClocks(plan.clock)).unwrap();
    let mut cpu = PrefetchCpu::from_prefetched(s.r, [w, s.queued()], CpuClocks(s.start)).unwrap();
    cpu.step_physical_instruction(&mut b)
        == Ok(PhysicalInstructionStep::Instruction(TimedStep {
            opcode: w,
            clocks: plan.clock - s.start,
            exception: None,
        }))
        && cpu == final_cpu
        && b == expected
}

fn newly_admitted(s: Spec, w: u16) -> bool {
    let (_, mut b) = s.seed();
    let old = b.clone();
    let mut cpu = PrefetchCpu::from_prefetched(s.r, [w, s.queued()], CpuClocks(u64::MAX)).unwrap();
    let mut good = cpu.step_physical_instruction(&mut b)
        == Err(InstructionFault::Entry(EntryFault::ClockOverflow))
        && cpu.registers == s.r
        && cpu.queued_words() == Some([w, s.queued()])
        && cpu.clock() == CpuClocks(u64::MAX)
        && b == old;
    let saved = cpu.clone();
    for path in 0..5 {
        let rejected = match path {
            0 => {
                cpu.step_physical_instruction(&mut b)
                    == Err(InstructionFault::Execution(TimingFault::Frozen))
            }
            1 => cpu.step_instruction(&mut b) == Err(TimingFault::Frozen),
            2 => cpu.step_boundary(&mut b) == Err(TimingFault::Frozen),
            3 => cpu.prime(&mut b) == Err(TimingFault::Frozen),
            _ => cpu.service_pending_trace(&mut b) == Err(TimingFault::Frozen),
        };
        good &= rejected && cpu == saved && b == old;
    }
    let mut copied = saved.clone();
    let mut device = old.clone();
    good &= copied.step_physical_instruction(&mut device)
        == Err(InstructionFault::Execution(TimingFault::Frozen));
    good &= copied == saved && device == old;
    good
}

pub(super) fn family_rejected() -> (u32, u32) {
    let (mut count, mut bad) = (0, 0);
    for bank in [false, true] {
        let s = Spec::new(0x4000, 2, 0, 0, bank);
        for family in [0x4000, 0x4400] {
            for low in 0..256 {
                let w = family | low;
                if w & 0xffc0 == 0x40c0 && selected(w, bank) {
                    let (_, mut b) = s.seed();
                    let old = b.clone();
                    let mut cpu =
                        PrefetchCpu::from_prefetched(s.r, [w, s.queued()], CpuClocks(u64::MAX))
                            .unwrap();
                    let good = cpu.step_physical_instruction(&mut b)
                        == Err(InstructionFault::Entry(EntryFault::ClockOverflow))
                        && cpu.registers == s.r
                        && cpu.queued_words() == Some([w, s.queued()])
                        && cpu.clock() == CpuClocks(u64::MAX)
                        && b == old
                        && bulk_oracle::freeze(&mut cpu, &mut b, TimingFault::Frozen);
                    count += 1;
                    bad += u32::from(!good);
                    continue;
                }
                if w & 0xffc0 == 0x44c0 && (w & 63 < 8 || w & 63 == 60) {
                    count += 1;
                    bad += u32::from(!newly_admitted(s, w));
                    continue;
                }
                if unary_completion::admitted(w) {
                    count += 1;
                    bad += u32::from(!completed_remainder(s, w));
                    continue;
                }
                if selected(w, bank) {
                    continue;
                }
                count += 1;
                bad += u32::from(!unsupported(s, w));
            }
        }
    }
    (count, bad)
}

fn prepared(s: Spec, gate: u8) -> (PrefetchCpu, Device, InstructionFault<&'static str>) {
    let (mut cpu, mut b) = s.seed();
    let error = match gate {
        0 => {
            cpu = PrefetchCpu::default();
            cpu.registers = s.r;
            InstructionFault::Execution(TimingFault::Unprimed)
        }
        1 => {
            cpu.registers.pc = cpu.registers.pc.wrapping_add(2);
            InstructionFault::Execution(TimingFault::Unprimed)
        }
        2 | 3 => {
            let mut r = s.r;
            r.set_status_register(if gate == 2 {
                r.status_register() | 0x8000
            } else {
                0x271f
            });
            cpu = PrefetchCpu::from_prefetched(
                r,
                [
                    if gate == 2 { 0x4e71 } else { 0x4e72 },
                    if gate == 2 {
                        s.opcode()
                    } else {
                        s.r.status_register()
                    },
                ],
                CpuClocks(s.start),
            )
            .unwrap();
            b.word(r.pc.wrapping_add(4), s.queued());
            cpu.step_instruction(&mut b).unwrap();
            cpu.registers = s.r;
            InstructionFault::Execution(if gate == 2 {
                TimingFault::PendingTrace
            } else {
                TimingFault::Stopped
            })
        }
        4 | 8 | 9 => {
            b.report = Some((
                0,
                if gate == 4 {
                    Kind::Host
                } else if gate == 8 {
                    Kind::Unavailable
                } else {
                    Kind::Boundary
                },
                8,
            ));
            if gate == 9 {
                b.entry_failure = Some((0, Kind::Boundary, 8));
            }
            assert!(cpu.step_physical_instruction(&mut b).is_err());
            InstructionFault::Execution(match gate {
                4 => TimingFault::Frozen,
                8 => TimingFault::PhysicalFaultUnqualified,
                _ => TimingFault::DoubleFaultHalted,
            })
        }
        5 => {
            cpu.registers
                .set_status_register(s.r.status_register() | 0x8000);
            InstructionFault::TraceUnqualified
        }
        6 => {
            cpu.interrupt_input().set_level(7);
            cpu.interrupt_input().set_level(0);
            InstructionFault::InterruptUnqualified
        }
        7 => {
            cpu = PrefetchCpu::from_group0_boundary(ImportedGroup0Boundary {
                registers: s.r,
                clock: CpuClocks(s.start),
                capture: Group0Capture {
                    vector: 2,
                    internal_pc: s.r.pc.wrapping_add(2),
                    phase_sr: s.r.status_register(),
                    ird: s.opcode(),
                    logical_address: s.address(s.low),
                    function_code: if s.r.supervisor() {
                        FunctionCode::SupervisorData
                    } else {
                        FunctionCode::UserData
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
    (cpu, b, error)
}

pub(super) fn preflight() -> (u32, u32) {
    let (mut count, mut bad) = (0, 0);
    for base in bulk_model::encoded() {
        for bank in [false, true] {
            let mut s = base;
            s.r.set_status_register(if bank { 0x201f } else { 0x001f });
            for gate in 0..10 {
                let (mut cpu, mut b, error) = prepared(s, gate);
                count += 1;
                bad += u32::from(!rejected(&mut cpu, &mut b, error));
            }
        }
    }
    (count, bad)
}
