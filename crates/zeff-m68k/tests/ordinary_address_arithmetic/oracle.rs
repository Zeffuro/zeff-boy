use super::*;

#[derive(Clone, Copy, Debug)]
pub(super) struct Transfer {
    pub access: BusAccess,
    pub registers: Registers,
}
pub(super) struct Plan {
    pub phases: Vec<Transfer>,
    pub full: Registers,
    pub queue: [u16; 2],
    pub shadow: BTreeMap<u32, u16>,
    pub source: u32,
    pub sampled: u32,
}

pub(super) fn address(case: Case, r: Registers, low: u16) -> u32 {
    let base = if case.family >= 9 {
        case.pc.wrapping_add(2)
    } else {
        r.address_register(case.source).unwrap()
    };
    match case.family {
        2 | 3 => base,
        4 => base.wrapping_sub(case.stride()),
        5 | 9 => base.wrapping_add_signed(i32::from(case.extension as i16)),
        6 | 10 => {
            let selector = usize::from(case.extension >> 12 & 7);
            let raw = if case.extension & 0x8000 == 0 {
                r.d[selector]
            } else {
                r.address_register(selector).unwrap()
            };
            let index = if case.extension & 0x800 == 0 {
                i32::from(raw as i16) as u32
            } else {
                raw
            };
            base.wrapping_add(index)
                .wrapping_add_signed(i32::from(case.extension as u8 as i8))
        }
        7 => i32::from(case.absolute as i16) as u32,
        8 => case.absolute & 0xffff0000 | u32::from(low),
        _ => unreachable!(),
    }
}
fn access(address: u32, r: Registers, program: bool, prefetch: bool) -> BusAccess {
    BusAccess {
        logical_address: address,
        bus_address: address & 0xffffff,
        function_code: match (r.supervisor(), program) {
            (false, false) => FunctionCode::UserData,
            (true, false) => FunctionCode::SupervisorData,
            (false, true) => FunctionCode::UserProgram,
            (true, true) => FunctionCode::SupervisorProgram,
        },
        kind: if prefetch {
            AccessKind::Prefetch
        } else {
            AccessKind::Data
        },
        lanes: ByteLanes::Both,
    }
}
fn read(case: Case, r: Registers, a: BusAccess, plan: &mut Plan) -> u16 {
    let at = plan.phases.len();
    if let Some((_, word)) = case.mutate.filter(|(index, _)| *index == at) {
        put(&mut plan.shadow, a.logical_address, word);
    }
    let value = sample(&plan.shadow, a.logical_address);
    plan.phases.push(Transfer {
        access: a,
        registers: r,
    });
    value
}
pub(super) fn plan(case: Case, memory: &BTreeMap<u32, u16>) -> Plan {
    let mut r = case.initial();
    let mut p = Plan {
        phases: Vec::new(),
        full: r,
        queue: [NEXT, FETCH],
        shadow: memory.clone(),
        source: 0,
        sampled: 0,
    };
    let mut low = case.absolute as u16;
    let mut next = case.retained();
    for i in 0..case.extensions() {
        let value = read(
            case,
            r,
            access(case.pc.wrapping_add(4 + 2 * i as u32), r, true, true),
            &mut p,
        );
        if case.family == 8 && i == 0 {
            low = value;
        }
        next = value;
    }
    let ea = address(case, r, low);
    if case.family == 4 {
        r.set_address_register(case.source, ea);
    }
    if case.family == 3 && !case.long {
        r.set_address_register(case.source, ea.wrapping_add(2));
    }
    let high = read(case, r, access(ea, r, case.family >= 9, false), &mut p);
    let raw = if case.long {
        if case.family == 3 {
            r.set_address_register(case.source, ea.wrapping_add(4));
        }
        let lo = read(
            case,
            r,
            access(ea.wrapping_add(2), r, case.family >= 9, false),
            &mut p,
        );
        u32::from(high) << 16 | u32::from(lo)
    } else {
        u32::from(high)
    };
    p.source = raw;
    let source = if case.long {
        raw
    } else {
        i32::from(raw as i16) as u32
    };
    p.sampled = r.address_register(case.destination).unwrap();
    let result = if case.subtract {
        p.sampled.wrapping_sub(source)
    } else {
        p.sampled.wrapping_add(source)
    };
    r.set_address_register(case.destination, p.sampled & 0xffff0000 | result & 65535);
    let f = read(
        case,
        r,
        access(case.end().wrapping_add(2), r, true, true),
        &mut p,
    );
    r.set_address_register(case.destination, result);
    r.pc = case.end();
    p.full = r;
    p.queue = [next, f];
    p
}
