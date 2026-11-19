use super::*;
use fixture::Case;

pub(super) fn arithmetic(family: u16, long: bool, old: u32, source: u32, sr: u16) -> (u32, u16) {
    let (mask, sign) = if long {
        (u32::MAX, 0x8000_0000)
    } else {
        (0xffff, 0x8000)
    };
    let source = source & mask;
    let destination = old & mask;
    let (result, carry, overflow) = match family {
        13 => {
            let total = u64::from(destination) + u64::from(source);
            let result = total as u32 & mask;
            (
                result,
                total > u64::from(mask),
                !(destination ^ source) & (destination ^ result) & sign != 0,
            )
        }
        9 => {
            let result = destination.wrapping_sub(source) & mask;
            (
                result,
                source > destination,
                (destination ^ source) & (destination ^ result) & sign != 0,
            )
        }
        12 => (destination & source, false, false),
        8 => (destination | source, false, false),
        _ => unreachable!(),
    };
    let mut flags = (u16::from(result & sign != 0) * 8)
        | (u16::from(result == 0) * 4)
        | (u16::from(overflow) * 2)
        | u16::from(carry);
    let sr = if matches!(family, 9 | 13) {
        flags |= u16::from(carry) * 16;
        (sr & !31) | flags
    } else {
        (sr & !15) | flags
    };
    ((old & !mask) | result, sr)
}

pub(super) fn access(address: u32, supervisor: bool, program: bool) -> BusAccess {
    BusAccess {
        logical_address: address,
        bus_address: address & 0xff_ffff,
        function_code: match (supervisor, program) {
            (false, false) => FunctionCode::UserData,
            (false, true) => FunctionCode::UserProgram,
            (true, false) => FunctionCode::SupervisorData,
            (true, true) => FunctionCode::SupervisorProgram,
        },
        kind: if program {
            AccessKind::Prefetch
        } else {
            AccessKind::Data
        },
        lanes: ByteLanes::Both,
    }
}

pub(super) fn an(r: Registers, index: usize) -> u32 {
    if index < 7 {
        r.a[index]
    } else if r.status_register() & 0x2000 != 0 {
        r.ssp
    } else {
        r.usp
    }
}
pub(super) fn set_an(r: &mut Registers, index: usize, value: u32) {
    if index < 7 {
        r.a[index] = value;
    } else if r.status_register() & 0x2000 != 0 {
        r.ssp = value;
    } else {
        r.usp = value;
    }
}
pub(super) fn address(c: Case, r: Registers, low: u16) -> u32 {
    let base = if c.ea >= 9 {
        c.pc.wrapping_add(2)
    } else {
        an(r, c.source)
    };
    match c.ea {
        2 | 3 => base,
        4 => base.wrapping_sub(c.stride()),
        5 | 9 => base.wrapping_add_signed(i32::from(c.extension as i16)),
        6 | 10 => {
            let selector = usize::from(c.extension >> 12 & 7);
            let raw = if c.extension & 0x8000 == 0 {
                r.d[selector]
            } else {
                an(r, selector)
            };
            let index = if c.extension & 0x800 == 0 {
                i32::from(raw as i16) as u32
            } else {
                raw
            };
            base.wrapping_add(index)
                .wrapping_add_signed(i32::from(c.extension as u8 as i8))
        }
        7 => i32::from(c.absolute as i16) as u32,
        8 => (c.absolute & 0xffff0000) | u32::from(low),
        _ => unreachable!(),
    }
}

#[derive(Clone, Copy, Debug)]
pub(super) struct Transfer {
    pub access: BusAccess,
    pub registers: Registers,
    pub word: u16,
}
pub(super) struct Plan {
    pub phases: Vec<Transfer>,
    pub before_result: Registers,
    pub full: Registers,
    pub queue: [u16; 2],
    pub shadow: BTreeMap<u32, u16>,
    pub source: u32,
}
fn read(c: Case, r: Registers, a: BusAccess, p: &mut Plan) -> u16 {
    if let Some((_, word)) = c.mutate.filter(|(at, _)| *at == p.phases.len()) {
        put(&mut p.shadow, a.logical_address, word);
    }
    let word = sample(&p.shadow, a.logical_address);
    p.phases.push(Transfer {
        access: a,
        registers: r,
        word,
    });
    word
}
pub(super) fn plan(c: Case, memory: &BTreeMap<u32, u16>) -> Plan {
    let mut r = c.initial();
    let mut p = Plan {
        phases: Vec::new(),
        before_result: r,
        full: r,
        queue: [NEXT, FETCH],
        shadow: memory.clone(),
        source: 0,
    };
    let mut low = c.absolute as u16;
    let mut next = c.retained();
    for at in 0..c.extensions() {
        let word = read(
            c,
            r,
            access(c.pc.wrapping_add(4 + 2 * at as u32), c.supervisor, true),
            &mut p,
        );
        if c.ea == 8 && at == 0 {
            low = word;
        }
        next = word;
    }
    let ea = address(c, r, low);
    if c.ea == 4 {
        set_an(&mut r, c.source, ea);
    }
    if c.ea == 3 && !c.long {
        set_an(&mut r, c.source, ea.wrapping_add(2));
    }
    let mut operand_access = access(ea, c.supervisor, c.ea >= 9);
    operand_access.kind = AccessKind::Data;
    let high = read(c, r, operand_access, &mut p);
    p.source = if c.long {
        if c.ea == 3 {
            set_an(&mut r, c.source, ea.wrapping_add(4));
        }
        operand_access.logical_address = ea.wrapping_add(2);
        operand_access.bus_address = operand_access.logical_address & 0xffffff;
        let low = read(c, r, operand_access, &mut p);
        (u32::from(high) << 16) | u32::from(low)
    } else {
        u32::from(high)
    };
    p.before_result = r;
    let old = r.d[c.destination];
    let (result, sr) = arithmetic(c.operation, c.long, old, p.source, r.status_register());
    r.d[c.destination] = (old & 0xffff0000) | (result & 65535);
    r.set_status_register(sr);
    let fetched = read(
        c,
        r,
        access(c.end().wrapping_add(2), c.supervisor, true),
        &mut p,
    );
    r.d[c.destination] = result;
    r.pc = c.end();
    p.full = r;
    p.queue = [next, fetched];
    p
}
