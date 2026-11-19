use super::*;
use bulk_device::Device;
use bulk_device::Kind;
use bulk_model::Spec;
use zeff_m68k::timing::physical::ImportedGroup0Boundary;

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

pub(super) fn rejected(
    cpu: &mut PrefetchCpu,
    b: &mut Device,
    error: InstructionFault<&'static str>,
) -> bool {
    let before = cpu.clone();
    let old = b.clone();
    let mut copy = before.clone();
    let mut device = old.clone();
    let mut good = copy.step_physical_instruction(&mut device) == Err(error.clone())
        && copy == before
        && device == old;
    good &= cpu.step_physical_instruction(b) == Err(error.clone()) && *cpu == before && *b == old;
    if let InstructionFault::Execution(fault) = error
        && matches!(
            fault,
            TimingFault::Frozen
                | TimingFault::ImportedBoundaryPending
                | TimingFault::PhysicalFaultUnqualified
                | TimingFault::DoubleFaultHalted
        )
    {
        good &= bulk_oracle::freeze(cpu, b, fault);
    }
    good
}
pub(super) fn masks() -> (u32, u32) {
    let (mut count, mut bad, mut processing, mut user, mut supervisor) = (0, 0, 0, 0, 0);
    for word in 0..=u16::MAX {
        let mut s = Spec::new(0);
        s.first = word;
        let (_, mut b) = s.seed();
        b.report = Some((1, Kind::Boundary, 8));
        let mut good = bulk_oracle::run(s, b);
        processing += u32::from(known(word));
        for bank in [false, true] {
            let mut s = Spec::new(if bank { 9 } else { 0 });
            s.start = u64::MAX;
            let (_, mut b) = s.seed();
            let mut cpu =
                PrefetchCpu::from_prefetched(s.r, [word, 0x4e71], CpuClocks(s.start)).unwrap();
            if selected(word, bank) {
                if bank {
                    supervisor += 1;
                } else {
                    user += 1;
                }
                let old = b.clone();
                let saved = cpu.clone();
                let text = format!("{saved:?}");
                good &= text.matches("frozen: false,").count() == 1;
                good &= cpu.step_physical_instruction(&mut b)
                    == Err(InstructionFault::Entry(EntryFault::ClockOverflow));
                good &= b == old
                    && format!("{cpu:?}") == text.replacen("frozen: false,", "frozen: true,", 1);
                good &= bulk_oracle::freeze(&mut cpu, &mut b, TimingFault::Frozen);
            } else {
                good &= rejected(
                    &mut cpu,
                    &mut b,
                    InstructionFault::Execution(TimingFault::UnsupportedInstruction {
                        pc: s.r.pc,
                        opcode: word,
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
pub(super) fn illegal() -> (u32, u32) {
    let (mut count, mut bad) = (0, 0);
    for bank in [false, true] {
        for family in [0x44c0, 0x46c0] {
            for ea in [8, 9, 10, 11, 12, 13, 14, 15, 61, 62, 63] {
                let s = Spec::new(if bank { 9 } else { 0 });
                let (_, mut b) = s.seed();
                let word = family | ea;
                let mut cpu =
                    PrefetchCpu::from_prefetched(s.r, [word, s.queued()], CpuClocks(s.start))
                        .unwrap();
                count += 1;
                bad += u32::from(!rejected(
                    &mut cpu,
                    &mut b,
                    InstructionFault::Execution(TimingFault::UnsupportedInstruction {
                        pc: s.r.pc,
                        opcode: word,
                    }),
                ));
            }
        }
    }
    (count, bad)
}
fn prepared(s: Spec, gate: u8) -> (PrefetchCpu, Device, InstructionFault<&'static str>) {
    let (mut cpu, mut b) = s.seed();
    if matches!(gate, 4 | 8 | 9) && s.family == 0x46c0 && !s.r.supervisor() {
        let mut setup = s;
        setup.family = 0x44c0;
        (cpu, b) = setup.seed();
    }
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
                    logical_address: s.anchor(),
                    function_code: if s.r.supervisor() {
                        FunctionCode::SupervisorProgram
                    } else {
                        FunctionCode::UserProgram
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
    for s in bulk_model::all_banks() {
        for gate in 0..10 {
            let (mut cpu, mut b, error) = prepared(s, gate);
            count += 1;
            bad += u32::from(!rejected(&mut cpu, &mut b, error));
        }
    }
    (count, bad)
}
pub(super) fn priorities() -> (u32, u32) {
    let (mut count, mut bad) = (0, 0);
    for s in bulk_model::all_banks() {
        for gate in [4, 8, 9, 7, 2, 3, 5, 6] {
            let (mut cpu, mut b, error) = prepared(s, gate);
            cpu.interrupt_input().set_level(7);
            cpu.interrupt_input().set_level(0);
            if gate != 6 {
                cpu.registers
                    .set_status_register(cpu.registers.status_register() | 0x8000);
            }
            count += 1;
            bad += u32::from(!rejected(&mut cpu, &mut b, error));
        }
    }
    let (n, b) = retained_inputs();
    count += n;
    bad += b;
    (count, bad)
}
fn retained_inputs() -> (u32, u32) {
    let (mut count, mut bad) = (0, 0);
    for family in [0x44c0, 0x46c0] {
        for ea in [0, 1, 2, 3, 4, 5, 6, 7, 60] {
            for bank in [false, true] {
                let mut s = Spec::new(0);
                s.family = family;
                s.immediate = ea == 60;
                s.reg = (ea & 7) as usize;
                s.queued = 0x4afc;
                let mut r = Registers::new();
                r.d = std::array::from_fn(|i| 0xa100_0100 + i as u32 * 0x20);
                r.d[3] = 2;
                r.a = std::array::from_fn(|i| 0x10000 + i as u32 * 0x1000);
                r.usp = 0x40000;
                r.ssp = 0x80000;
                r.pc = 0x1000;
                r.set_status_register(if bank { 0x271b } else { 0x071b });
                s.r = r;
                let mut b = Device::new(0);
                b.words.clear();
                b.default_word = 0x4e71;
                for (a, v) in [
                    (8, 0),
                    (10, 0x6000),
                    (12, 0),
                    (14, 0x6000),
                    (0x6000, 0x4e71),
                    (0x6002, 0x7201),
                    (0x1000, 0x44d0),
                    (0x1002, 0x4e71),
                    (0x1004, 0x7201),
                    (0x10000, 0x001f),
                ] {
                    b.word(a, v);
                }
                count += 1;
                bad += u32::from(!bulk_oracle::run(s, b));
            }
        }
    }
    (count, bad)
}
