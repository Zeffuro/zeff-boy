use super::*;

#[derive(Clone, Copy, Debug)]
pub(super) struct Transfer {
    pub access: BusAccess,
    pub phase: InstructionPhase,
    pub registers: Registers,
    pub internal_pc: u32,
    pub ird: u16,
}

pub(super) fn sample(words: &Memory, address: u32) -> u16 {
    words
        .get(&(address & BUS_ADDRESS_MASK & !1))
        .copied()
        .unwrap_or(0x4e71)
}

pub(super) fn access(address: u32, supervisor: bool, program: bool, kind: AccessKind) -> BusAccess {
    BusAccess {
        logical_address: address,
        bus_address: address & BUS_ADDRESS_MASK & !1,
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
    let base = if matches!(case.form.ea, Ea::PcDisplacement | Ea::PcIndex) {
        case.r.pc.wrapping_add(2)
    } else {
        case.r.address_register(usize::from(case.register)).unwrap()
    };
    match case.form.ea {
        Ea::Indirect | Ea::Post => base,
        Ea::Pre => base.wrapping_sub(case.form.stride()),
        Ea::Displacement | Ea::PcDisplacement => {
            base.wrapping_add_signed(i32::from(case.extension as i16))
        }
        Ea::Index | Ea::PcIndex => {
            let selector = usize::from(case.extension >> 12 & 7);
            let raw = if case.extension & 0x8000 != 0 {
                case.r.address_register(selector).unwrap()
            } else {
                case.r.d[selector]
            };
            let index = if case.extension & 0x800 != 0 {
                raw
            } else {
                i32::from(raw as i16) as u32
            };
            base.wrapping_add(index)
                .wrapping_add_signed(i32::from(case.extension as u8 as i8))
        }
        Ea::AbsoluteWord => i32::from(case.extension as i16) as u32,
        Ea::AbsoluteLong => {
            u32::from(case.extension) << 16 | u32::from(sample(words, case.r.pc.wrapping_add(4)))
        }
    }
}

pub(super) fn value(case: Case, words: &Memory) -> u32 {
    let ea = address(case, words);
    let high = u32::from(sample(words, ea));
    match case.form.size {
        Size::Byte if ea & 1 == 0 => high >> 8,
        Size::Byte => high & 255,
        Size::Word => high,
        Size::Long => high << 16 | u32::from(sample(words, ea.wrapping_add(2))),
    }
}

pub(super) fn arithmetic(case: Case, source: u32) -> (u32, u16) {
    let (mask, sign) = if case.form.size == Size::Long {
        (u32::MAX, 0x8000_0000)
    } else {
        (65535, 32768)
    };
    let src = source & mask;
    let dst = case.r.d[usize::from(case.destination)] & mask;
    let result = match case.form.op {
        Op::Add => dst.wrapping_add(src),
        Op::Sub => dst.wrapping_sub(src),
        Op::And => dst & src,
        Op::Or => dst | src,
    } & mask;
    let (carry, overflow) = match case.form.op {
        Op::Add => (
            u64::from(dst) + u64::from(src) > u64::from(mask),
            (!(dst ^ src) & (dst ^ result) & sign) != 0,
        ),
        Op::Sub => (src > dst, ((dst ^ src) & (dst ^ result) & sign) != 0),
        _ => (false, false),
    };
    let old = case.r.status_register();
    let x = if matches!(case.form.op, Op::Add | Op::Sub) {
        u16::from(carry) << 4
    } else {
        old & 16
    };
    let flags = (old & !31)
        | x
        | (u16::from(result & sign != 0) * 8)
        | (u16::from(result == 0) * 4)
        | (u16::from(overflow) * 2)
        | u16::from(carry);
    (result, flags)
}

pub(super) fn next(case: Case, words: &Memory) -> u16 {
    if case.form.extensions() == 0 {
        case.next
    } else {
        sample(words, case.r.pc.wrapping_add(case.form.length()))
    }
}

pub(super) fn plan(case: Case, words: &Memory) -> Vec<Transfer> {
    let mut result = Vec::new();
    let mut r = case.r;
    let opcode = case.opcode();
    for i in 0..case.form.extensions() {
        result.push(Transfer {
            access: access(
                case.r.pc.wrapping_add(4 + 2 * i as u32),
                r.supervisor(),
                true,
                AccessKind::Prefetch,
            ),
            phase: if i == 0 {
                InstructionPhase::ExtensionFirst
            } else {
                InstructionPhase::ExtensionSecond
            },
            registers: r,
            internal_pc: case.r.pc.wrapping_add(if i == 1 {
                6
            } else if case.form.ea == Ea::AbsoluteWord {
                4
            } else {
                2
            }),
            ird: opcode,
        });
    }
    let source = address(case, words);
    for low in 0..=usize::from(case.form.size == Size::Long) {
        if case.form.ea == Ea::Pre && low == 0 {
            r.set_address_register(usize::from(case.register), source);
        }
        if case.form.ea == Ea::Post && (case.form.size == Size::Word || low == 1) {
            r.set_address_register(
                usize::from(case.register),
                source.wrapping_add(case.form.stride()),
            );
        }
        result.push(Transfer {
            access: access(
                source.wrapping_add(low as u32 * 2),
                r.supervisor(),
                matches!(case.form.ea, Ea::PcDisplacement | Ea::PcIndex),
                AccessKind::Data,
            ),
            phase: if low == 0 {
                InstructionPhase::OperandReadHigh
            } else {
                InstructionPhase::OperandReadLow
            },
            registers: r,
            internal_pc: case.r.pc.wrapping_add(match case.form.ea {
                Ea::AbsoluteLong => 6,
                Ea::AbsoluteWord => 4,
                Ea::Pre if case.form.size == Size::Word => 4,
                _ => 2,
            }),
            ird: opcode,
        });
    }
    let (full, flags) = arithmetic(case, value(case, words));
    let destination = usize::from(case.destination);
    r.d[destination] = (r.d[destination] & 0xffff_0000) | (full & 65535);
    r.set_status_register(flags);
    let address = case.r.pc.wrapping_add(case.form.length() + 2);
    result.push(Transfer {
        access: access(address, r.supervisor(), true, AccessKind::Prefetch),
        phase: InstructionPhase::OperandRefill,
        registers: r,
        internal_pc: address,
        ird: if case.form.size == Size::Long {
            opcode
        } else {
            next(case, words)
        },
    });
    result
}

pub(super) fn clocks(case: Case, elapsed: &[u32; 5], count: usize, idle: bool) -> u64 {
    elapsed[..count].iter().map(|n| u64::from(*n)).sum::<u64>()
        + if idle && case.form.idle() { 2 } else { 0 }
}

pub(super) fn events(case: Case, plan: &[Transfer], elapsed: &[u32; 5], idle: bool) -> Vec<Event> {
    let mut clock = case.start;
    let mut result = Vec::new();
    if idle && case.form.idle() {
        result.push(Event::Idle(clock, 2));
        clock += 2;
    }
    for (t, elapsed) in plan.iter().zip(elapsed) {
        result.push(Event::Read(t.access, clock));
        clock += u64::from(*elapsed);
    }
    result
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
        processing_class: ProcessingClass::InstructionOrGroup2,
        ssw_profile: SswProfile::SelectedIrdUpperBits,
    }
}

pub(super) fn final_registers(case: Case, words: &Memory, plan: &[Transfer]) -> Registers {
    let mut r = plan.last().unwrap().registers;
    if case.form.size == Size::Long {
        r.d[usize::from(case.destination)] = arithmetic(case, value(case, words)).0;
    }
    r.pc = case.r.pc.wrapping_add(case.form.length());
    r
}
pub(super) fn data_known(word: u16) -> bool {
    let op = word >> 12;
    let size = word >> 6 & 7;
    let mode = word >> 3 & 7;
    let reg = word & 7;
    matches!(op, 8 | 9 | 12 | 13)
        && size <= 2
        && (mode != 1 || (matches!(op, 9 | 13) && size != 0))
        && (mode != 7 || reg <= 4)
}
pub(super) fn known(word: u16) -> bool {
    if data_known(word) || word == 0x4e71 || word & 0xf100 == 0x7000 || word & 0xff00 == 0x6000 {
        return true;
    }
    let top = word >> 12;
    if (1..=3).contains(&top) {
        let mode = word >> 3 & 7;
        let reg = word & 7;
        let dm = word >> 6 & 7;
        let dr = word >> 9 & 7;
        return (mode != 7 || reg <= 4)
            && (top != 1 || mode != 1)
            && (dm <= 6)
            && (top != 1 || dm != 1)
            || ((mode != 7 || reg <= 4) && (top != 1 || mode != 1) && dm == 7 && dr <= 1);
    }
    false
}
