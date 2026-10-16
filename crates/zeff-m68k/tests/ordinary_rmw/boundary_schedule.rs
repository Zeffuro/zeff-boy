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

fn logic(old: u16, result: u32, size: u32) -> u16 {
    let mask = if size == 2 { 0xffff } else { u32::MAX };
    let sign = if size == 2 { 0x8000 } else { 0x8000_0000 };
    (old & !15) | if result & mask == 0 { 4 } else { 0 } | if result & sign != 0 { 8 } else { 0 }
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
    let result = if family == 0x4200 {
        0
    } else {
        value ^ if size == 2 { 0xffff } else { u32::MAX }
    };
    let f = model::transfers(&actions);
    actions.push(Action::Reserve);
    actions.push(Action::Sr(logic(r.status_register(), result, 2)));
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
        actions.push(Action::Sr(logic(r.status_register(), result, 4)));
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
        r.d[reg] = (r.d[reg] & 0xffff_0000) | 0x001f;
    }
    let mut bus = Device::new(11);
    model::handler(&mut bus, 8);
    let (mut actions, _, cursor, mut queue) = source(r, mode, reg, 2, 0x001f, &mut bus);
    queue[0] = 0x46c0 | (mode << 3) | reg as u16;
    if supervisor {
        actions.push(Action::Idle(2));
        actions.push(Action::Sr(0x001f));
        actions.push(Action::Idle(2));
        read(
            &mut actions,
            cursor,
            FunctionCode::UserProgram,
            AccessKind::Prefetch,
        );
        read(
            &mut actions,
            cursor + 2,
            FunctionCode::UserProgram,
            AccessKind::Prefetch,
        );
        actions.push(Action::Commit(cursor, [0x7201, 0x7402]));
    } else {
        actions = model::entry(r, 8, false);
    }
    Schedule {
        r,
        queue,
        bus,
        actions,
        f: 0,
    }
}
