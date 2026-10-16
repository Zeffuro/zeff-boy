use super::*;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Transfer {
    pub access: BusAccess,
    pub phase: InstructionPhase,
    pub registers: Registers,
    pub internal_pc: u32,
    pub ird: u16,
}
pub(super) fn sample(words: &Memory, address: u32) -> u16 {
    words.get(&(address & 0xfffffe)).copied().unwrap_or(0x4e71)
}
pub(super) fn access(address: u32, supervisor: bool, program: bool, kind: AccessKind) -> BusAccess {
    BusAccess {
        logical_address: address,
        bus_address: address & 0xfffffe,
        function_code: match (program, supervisor) {
            (false, false) => FunctionCode::UserData,
            (false, true) => FunctionCode::SupervisorData,
            (true, false) => FunctionCode::UserProgram,
            (true, true) => FunctionCode::SupervisorProgram,
        },
        kind,
        lanes: ByteLanes::Both,
    }
}
pub(super) fn address(case: Case, words: &Memory) -> u32 {
    let b = case.r.pc.wrapping_add(case.form.stride());
    let ext = sample(words, b.wrapping_add(2));
    let base = case.r.address_register(usize::from(case.register)).unwrap();
    match case.form.ea {
        Ea::Dn => 0,
        Ea::Indirect | Ea::Post => base,
        Ea::Pre => base.wrapping_sub(step(case)),
        Ea::Displacement => base.wrapping_add_signed(i32::from(ext as i16)),
        Ea::Index => {
            let selector = usize::from(ext >> 12 & 7);
            let raw = if ext & 0x8000 != 0 {
                case.r.address_register(selector).unwrap()
            } else {
                case.r.d[selector]
            };
            let index = if ext & 0x800 != 0 {
                raw
            } else {
                i32::from(raw as i16) as u32
            };
            base.wrapping_add(index)
                .wrapping_add_signed(i32::from(ext as u8 as i8))
        }
        Ea::AbsoluteWord => i32::from(ext as i16) as u32,
        Ea::AbsoluteLong => u32::from(ext) << 16 | u32::from(sample(words, b.wrapping_add(4))),
    }
}
pub(super) fn value(case: Case, words: &Memory) -> u32 {
    if case.form.ea == Ea::Dn {
        return case.r.d[usize::from(case.register)];
    }
    let ea = address(case, words);
    let high = u32::from(sample(words, ea));
    if case.form.size == Size::Long {
        high << 16 | u32::from(sample(words, ea.wrapping_add(2)))
    } else if case.form.size == Size::Byte {
        (high >> if ea & 1 == 0 { 8 } else { 0 }) & 255
    } else {
        high
    }
}
pub(super) fn immediate(case: Case, words: &Memory) -> u32 {
    if case.form.size == Size::Long {
        u32::from(case.retained()) << 16 | u32::from(sample(words, case.r.pc.wrapping_add(4)))
    } else {
        u32::from(case.retained())
    }
}
pub(super) fn flags(case: Case, words: &Memory) -> u16 {
    let (mask, sign) = match case.form.size {
        Size::Byte => (255, 128),
        Size::Word => (65535, 32768),
        Size::Long => (u32::MAX, 0x8000_0000),
    };
    let src = immediate(case, words) & mask;
    let dst = value(case, words) & mask;
    let result = dst.wrapping_sub(src) & mask;
    (case.r.status_register() & !15)
        | (u16::from(result & sign != 0) * 8)
        | (u16::from(result == 0) * 4)
        | (u16::from((dst ^ src) & (dst ^ result) & sign != 0) * 2)
        | u16::from(src > dst)
}
pub(super) fn next(case: Case, words: &Memory) -> u16 {
    sample(words, case.r.pc.wrapping_add(case.form.length()))
}
pub(super) fn step(case: Case) -> u32 {
    match case.form.size {
        Size::Byte if case.register != 7 => 1,
        Size::Long => 4,
        _ => 2,
    }
}
pub(super) fn plan(case: Case, words: &Memory) -> Vec<Transfer> {
    let mut out = Vec::new();
    let mut r = case.r;
    let p = r.pc;
    let b = p.wrapping_add(case.form.stride());
    let mut push = |address, pc, phase, registers: Registers, ird, program| {
        out.push(Transfer {
            access: access(
                address,
                registers.supervisor(),
                program,
                if program {
                    AccessKind::Prefetch
                } else {
                    AccessKind::Data
                },
            ),
            phase,
            registers,
            internal_pc: pc,
            ird,
        });
    };
    for i in 0..case.form.immediates() {
        push(
            p.wrapping_add(4 + 2 * i as u32),
            p.wrapping_add(if case.form.size != Size::Long {
                4
            } else if i == 0 {
                2
            } else {
                6
            }),
            if i == 0 {
                InstructionPhase::ImmediateFirst
            } else {
                InstructionPhase::ImmediateSecond
            },
            r,
            case.opcode(),
            true,
        );
    }
    for i in 0..case.form.extensions() {
        push(
            b.wrapping_add(4 + 2 * i as u32),
            b.wrapping_add(if i == 1 {
                6
            } else if case.form.ea == Ea::AbsoluteWord {
                4
            } else {
                2
            }),
            if i == 0 {
                InstructionPhase::ExtensionFirst
            } else {
                InstructionPhase::ExtensionSecond
            },
            r,
            case.opcode(),
            true,
        );
    }
    let ea = address(case, words);
    for low in 0..if case.form.ea == Ea::Dn {
        0
    } else {
        1 + usize::from(case.form.size == Size::Long)
    } {
        if (case.form.ea == Ea::Pre && low == 0)
            || (case.form.ea == Ea::Post && (case.form.size != Size::Long || low == 1))
        {
            r.set_address_register(
                usize::from(case.register),
                if case.form.ea == Ea::Pre {
                    ea
                } else {
                    ea.wrapping_add(step(case))
                },
            );
        }
        let pc = b.wrapping_add(match case.form.ea {
            Ea::AbsoluteLong => 6,
            Ea::AbsoluteWord => 4,
            Ea::Pre if case.form.size != Size::Long => 4,
            _ => 2,
        });
        push(
            ea.wrapping_add(2 * low as u32),
            pc,
            if low == 0 {
                InstructionPhase::OperandReadHigh
            } else {
                InstructionPhase::OperandReadLow
            },
            r,
            case.opcode(),
            false,
        );
    }
    r.set_status_register(flags(case, words));
    let f = p.wrapping_add(case.form.length() + 2);
    push(
        f,
        f,
        InstructionPhase::OperandRefill,
        r,
        if case.form.tail() {
            case.opcode()
        } else {
            next(case, words)
        },
        true,
    );
    if case.form.size == Size::Byte && case.form.ea != Ea::Dn {
        let t = out
            .iter_mut()
            .find(|t| t.phase == InstructionPhase::OperandReadHigh)
            .unwrap();
        t.access.lanes = if ea & 1 == 0 {
            ByteLanes::Upper
        } else {
            ByteLanes::Lower
        };
    }
    out
}
pub(super) fn clocks(case: Case, elapsed: &[u32; 7], count: usize, idle: bool) -> u64 {
    elapsed[..count].iter().map(|n| u64::from(*n)).sum::<u64>()
        + if idle && case.form.idle() && count > case.form.immediates() {
            2
        } else {
            0
        }
}
pub(super) fn events(case: Case, plan: &[Transfer], elapsed: &[u32; 7], idle: bool) -> Vec<Event> {
    let mut clock = case.start;
    let mut out = Vec::new();
    for (i, (t, elapsed)) in plan.iter().zip(elapsed).enumerate() {
        if idle && case.form.idle() && i == case.form.immediates() {
            out.push(Event::Idle(clock, 2));
            clock += 2;
        }
        out.push(Event::Read(t.access, clock));
        clock += u64::from(*elapsed);
    }
    out
}
pub(super) fn capture(t: Transfer, vector: u8) -> Group0Capture {
    Group0Capture {
        vector,
        internal_pc: t.internal_pc,
        phase_sr: t.registers.status_register(),
        ird: t.ird,
        logical_address: t.access.logical_address,
        function_code: t.access.function_code,
        direction: Direction::Read,
        processing_class: if known(t.ird) {
            ProcessingClass::InstructionOrGroup2
        } else {
            ProcessingClass::Group0OrGroup1
        },
        ssw_profile: SswProfile::SelectedIrdUpperBits,
    }
}
pub(super) fn final_registers(case: Case, _words: &Memory, plan: &[Transfer]) -> Registers {
    let mut r = plan.last().unwrap().registers;
    r.pc = case.r.pc.wrapping_add(case.form.length());
    r
}
pub(super) fn cmpi_known(word: u16) -> bool {
    let mode = word >> 3 & 7;
    let reg = word & 7;
    word & 0xff00 == 0x0c00
        && word >> 6 & 3 != 3
        && (mode == 0 || (2..=6).contains(&mode) || mode == 7 && reg <= 1)
}
pub(super) fn known(word: u16) -> bool {
    let top = word >> 12;
    let size = word >> 6 & 7;
    let mode = word >> 3 & 7;
    let reg = word & 7;
    if cmpi_known(word) || word == 0x4e71 || word & 0xf100 == 0x7000 || top == 6 {
        return true;
    }
    if matches!(top, 8 | 9 | 12 | 13)
        && size <= 2
        && (mode != 7 || reg <= 4)
        && (mode != 1 || size != 0 && matches!(top, 9 | 13))
    {
        return true;
    }
    let destination_mode = word >> 6 & 7;
    let destination_register = word >> 9 & 7;
    (1..=3).contains(&top)
        && (mode != 7 || reg <= 4)
        && (destination_mode != 7 || destination_register <= 1)
        && (top != 1 || mode != 1 && destination_mode != 1)
}
