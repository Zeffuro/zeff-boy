use super::*;
use device::Device;
use model::{Action, an, read};

pub(super) struct Schedule {
    pub r: Registers,
    pub queue: [u16; 2],
    pub bus: Device,
    pub actions: Vec<Action>,
    pub f: usize,
}

fn source(
    r: Registers,
    mode: u16,
    reg: usize,
    size: u32,
    value: u32,
    bus: &mut Device,
) -> (Vec<Action>, u32, u32, [u16; 2]) {
    let pc = r.pc;
    let base = if mode == 7 { pc + 2 } else { an(r, reg) };
    let (extensions, address) = match (mode, reg) {
        (0, _) => (vec![], 0),
        (2 | 3, _) => (vec![], base),
        (4, _) => (vec![], base - size),
        (5, _) | (7, 2) => (vec![0x0040], base + 0x40),
        (6, _) | (7, 3) => (vec![0x1010], base + (r.d[1] as i16 as i32 as u32) + 0x10),
        (7, 0) => (vec![0x8030], 0xffff_8030),
        (7, 1) => (vec![0x2345, 0x3010], 0x2345_3010),
        (7, 4) => (vec![value as u16], 0),
        _ => unreachable!(),
    };
    let cursor = pc + 2 + extensions.len() as u32 * 2;
    let mut code = extensions.clone();
    code.extend([0x7201, 0x7402]);
    for (index, word) in code.iter().enumerate() {
        bus.put(pc + 2 + index as u32 * 2, *word);
    }
    let queue = [0, code[0]];
    let program = if r.status_register() & 0x2000 != 0 {
        FunctionCode::SupervisorProgram
    } else {
        FunctionCode::UserProgram
    };
    let data = if r.status_register() & 0x2000 != 0 {
        FunctionCode::SupervisorData
    } else {
        FunctionCode::UserData
    };
    let mut a = Vec::new();
    if mode == 4 || mode == 6 || mode == 7 && reg == 3 {
        a.push(Action::Idle(2));
    }
    if mode == 4 {
        a.push(Action::An(reg, address));
    }
    for index in 0..extensions.len() {
        read(
            &mut a,
            pc + 4 + index as u32 * 2,
            program,
            AccessKind::Prefetch,
        );
    }
    if mode == 3 && size == 2 {
        a.push(Action::An(reg, base + 2));
    }
    if mode != 0 && !(mode == 7 && reg == 4) {
        bus.put(
            address,
            if size == 4 {
                (value >> 16) as u16
            } else {
                value as u16
            },
        );
        read(
            &mut a,
            address,
            if mode == 7 && matches!(reg, 2 | 3) {
                program
            } else {
                data
            },
            AccessKind::Data,
        );
        if size == 4 {
            if mode == 3 {
                a.push(Action::An(reg, base + 4));
            }
            bus.put(address + 2, value as u16);
            read(&mut a, address + 2, data, AccessKind::Data);
        }
    }
    (a, address, cursor, queue)
}

fn negate(old: u16, family: u16, value: u32, size: u32) -> (u32, u16) {
    let modulus = 1u64 << (size * 8);
    let sign = modulus / 2;
    let source = u64::from(value) & (modulus - 1);
    let incoming = u64::from(family == 0x4000 && old & 16 != 0);
    let sum = source + incoming;
    let result = 0u64.wrapping_sub(sum) & (modulus - 1);
    let signed = if source < sign {
        source as i64
    } else {
        source as i64 - modulus as i64
    };
    let mathematical = -signed - incoming as i64;
    let flags = (u16::from(sum != 0) * 17)
        | (u16::from(result == 0 && (family == 0x4400 || old & 4 != 0)) * 4)
        | (u16::from(result >= sign) * 8)
        | (u16::from(mathematical < -(sign as i64) || mathematical >= sign as i64) * 2);
    (result as u32, old & !31 | flags)
}

pub(super) fn ordinary(
    family: u16,
    size: u32,
    mode: u16,
    reg: usize,
    supervisor: bool,
    trace: bool,
) -> Schedule {
    let r = model::registers(supervisor, trace);
    let mut bus = Device::new(11);
    model::handler(&mut bus, 9);
    model::handler(&mut bus, 31);
    let value = 0x8000;
    let (mut actions, address, cursor, mut queue) = source(r, mode, reg, size, value, &mut bus);
    queue[0] = family | if size == 2 { 0x40 } else { 0x80 } | (mode << 3) | reg as u16;
    let (result, full_sr) = negate(r.status_register(), family, value, size);
    let low_sr = negate(r.status_register(), family, value, 2).1;
    let f = model::transfers(&actions);
    actions.push(Action::Reserve);
    actions.push(Action::Sr(low_sr));
    let program = if supervisor {
        FunctionCode::SupervisorProgram
    } else {
        FunctionCode::UserProgram
    };
    let data = if supervisor {
        FunctionCode::SupervisorData
    } else {
        FunctionCode::UserData
    };
    read(&mut actions, cursor + 2, program, AccessKind::Prefetch);
    if size == 4 {
        actions.push(Action::Reserve);
        actions.push(Action::Sr(full_sr));
        actions.push(Action::Write(
            model::access(address + 2, data, AccessKind::Data),
            result as u16,
        ));
    }
    actions.push(Action::Write(
        model::access(address, data, AccessKind::Data),
        if size == 4 {
            (result >> 16) as u16
        } else {
            result as u16
        },
    ));
    actions.push(Action::Commit(cursor, [0x7201, 0x7402]));
    Schedule {
        r,
        queue,
        bus,
        actions,
        f,
    }
}

pub(super) fn status(mode: u16, reg: usize, supervisor: bool) -> Schedule {
    let mut r = model::registers(supervisor, false);
    if mode == 0 {
        r.d[reg] = (r.d[reg] & 0xffff_0000) | 0xa704;
    }
    let mut bus = Device::new(11);
    let (mut actions, _, cursor, mut queue) = source(r, mode, reg, 2, 0xa704, &mut bus);
    queue[0] = 0x44c0 | (mode << 3) | reg as u16;
    actions.push(Action::Idle(2));
    actions.push(Action::Sr(r.status_register() & !31 | 4));
    actions.push(Action::Idle(2));
    let program = if supervisor {
        FunctionCode::SupervisorProgram
    } else {
        FunctionCode::UserProgram
    };
    read(&mut actions, cursor, program, AccessKind::Prefetch);
    read(&mut actions, cursor + 2, program, AccessKind::Prefetch);
    actions.push(Action::Commit(cursor, [0x7201, 0x7402]));
    Schedule {
        r,
        queue,
        bus,
        actions,
        f: 0,
    }
}

pub(super) fn from_sr(mode: u16, reg: usize, supervisor: bool) -> Schedule {
    let r = model::registers(supervisor, false);
    let mut bus = Device::new(11);
    let (mut actions, address, cursor, mut queue) = source(r, mode, reg, 2, 0x8000, &mut bus);
    queue[0] = 0x40c0 | mode << 3 | reg as u16;
    if mode == 0 {
        actions.push(Action::Dn(
            reg,
            r.d[reg] & 0xffff_0000 | u32::from(r.status_register()),
        ));
    }
    let f = model::transfers(&actions);
    let program = if supervisor {
        FunctionCode::SupervisorProgram
    } else {
        FunctionCode::UserProgram
    };
    let data = if supervisor {
        FunctionCode::SupervisorData
    } else {
        FunctionCode::UserData
    };
    read(&mut actions, cursor + 2, program, AccessKind::Prefetch);
    if mode == 0 {
        actions.push(Action::Idle(2));
    } else {
        actions.push(Action::Write(
            model::access(address, data, AccessKind::Data),
            r.status_register(),
        ));
    }
    actions.push(Action::Commit(cursor, [0x7201, 0x7402]));
    Schedule {
        r,
        queue,
        bus,
        actions,
        f,
    }
}
