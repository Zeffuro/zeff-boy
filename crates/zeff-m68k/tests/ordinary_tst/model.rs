use super::*;
use device::{Device, Event};

pub(super) const RESERVE: u64 = 4 + u32::MAX as u64;

#[derive(Clone, Copy)]
pub(super) enum Action {
    Idle(u32),
    An(usize, u32),
    Read(BusAccess, bool),
}

pub(super) struct Spec {
    pub initial: Registers,
    pub queue: [u16; 2],
    pub device: Device,
    pub actions: Vec<Action>,
    pub value: u32,
    pub next_pc: u32,
    pub source: u32,
}

pub(super) fn forms() -> Vec<(u16, bool)> {
    let mut forms = Vec::new();
    for size in 0..3 {
        for mode in [0, 2, 3, 4, 5, 6, 7] {
            for register in 0..if mode == 7 { 2 } else { 8 } {
                let opcode = 0x4a00 | size << 6 | mode << 3 | register;
                forms.push((opcode, false));
                if size == 0 && mode != 0 {
                    forms.push((opcode, true));
                }
            }
        }
    }
    forms
}

pub(super) fn seed(opcode: u16, odd: bool, sr: u16, value: u32) -> Spec {
    let size = opcode >> 6 & 3;
    let mode = opcode >> 3 & 7;
    let register = usize::from(opcode & 7);
    let mut r = Registers::new();
    r.pc = 0x1234_0100;
    r.set_status_register(sr);
    r.d = std::array::from_fn(|i| 0x1111_0000 + i as u32);
    r.d[1] = 0xcafe_fffe;
    for i in 0..8 {
        r.set_address_register(i, 0x1200_3008 + i as u32 * 16 + u32::from(odd));
    }
    if r.supervisor() {
        r.usp = 0xdead_b000;
    } else {
        r.ssp = 0xdead_a000;
    }
    let base = r.address_register(register).unwrap();
    let increment = if size == 0 && register == 7 {
        2
    } else {
        1 << size
    };
    let (extensions, source) = match mode {
        0 => {
            r.d[register] = value;
            (vec![], r.pc + 4)
        }
        2 | 3 => (vec![], base),
        4 => (vec![], base.wrapping_sub(increment)),
        5 => (vec![0xfffe], base.wrapping_sub(2)),
        6 => (vec![0x1702], base),
        7 if register == 0 => (vec![0x8030 + u16::from(odd)], 0xffff_8030 + u32::from(odd)),
        7 => (
            vec![0xabcd, 0x3010 + u16::from(odd)],
            0xabcd_3010 + u32::from(odd),
        ),
        _ => unreachable!(),
    };
    let mut words = BTreeMap::new();
    words.insert(r.pc & BUS_ADDRESS_MASK, opcode);
    let stream: Vec<_> = extensions.iter().copied().chain([0x7201, 0x7402]).collect();
    for (i, word) in stream.iter().enumerate() {
        words.insert((r.pc + 2 + i as u32 * 2) & BUS_ADDRESS_MASK, *word);
    }
    if mode != 0 {
        let high = match size {
            0 if source & 1 == 0 => ((value as u16 & 0xff) << 8) | 0x5a,
            0 => 0xa500 | (value as u16 & 0xff),
            1 => value as u16,
            _ => (value >> 16) as u16,
        };
        words.insert(source & BUS_ADDRESS_MASK & !1, high);
        if size == 2 {
            words.insert(source.wrapping_add(2) & BUS_ADDRESS_MASK, value as u16);
        }
    }
    let access = |address: u32, program: bool| BusAccess {
        logical_address: address,
        bus_address: address & BUS_ADDRESS_MASK & !1,
        function_code: match (r.supervisor(), program) {
            (false, false) => FunctionCode::UserData,
            (true, false) => FunctionCode::SupervisorData,
            (false, true) => FunctionCode::UserProgram,
            (true, true) => FunctionCode::SupervisorProgram,
        },
        kind: if program {
            AccessKind::Prefetch
        } else {
            AccessKind::Data
        },
        lanes: if program || size != 0 {
            ByteLanes::Both
        } else if address & 1 == 0 {
            ByteLanes::Upper
        } else {
            ByteLanes::Lower
        },
    };
    let mut actions = Vec::new();
    if matches!(mode, 4 | 6) {
        actions.push(Action::Idle(2));
    }
    for i in 0..extensions.len() {
        actions.push(Action::Read(access(r.pc + 4 + i as u32 * 2, true), false));
    }
    if mode == 4 {
        actions.push(Action::An(register, source));
    }
    if mode == 3 && size != 2 {
        actions.push(Action::An(register, base.wrapping_add(increment)));
    }
    if mode != 0 {
        actions.push(Action::Read(access(source, false), false));
        if size == 2 {
            if mode == 3 {
                actions.push(Action::An(register, base.wrapping_add(increment)));
            }
            actions.push(Action::Read(access(source.wrapping_add(2), false), false));
        }
    }
    let next_pc = r.pc + 2 + extensions.len() as u32 * 2;
    actions.push(Action::Read(access(next_pc + 2, true), true));
    let count = actions
        .iter()
        .filter(|a| matches!(a, Action::Read(..)))
        .count();
    Spec {
        initial: r,
        queue: [opcode, stream[0]],
        device: Device {
            words,
            clock: 0,
            events: vec![],
            count: 0,
            wait: 0,
            final_wait: None,
            final_index: count - 1,
            fail: None,
            mutation: None,
        },
        actions,
        value,
        next_pc,
        source,
    }
}

pub(super) fn expected(
    spec: &Spec,
) -> (
    Registers,
    [u16; 2],
    Device,
    Result<TimedStep, TimingFault<usize>>,
) {
    let mut r = spec.initial;
    let mut device = spec.device.clone();
    let start = device.clock;
    let mut retained = spec.queue[1];
    for action in &spec.actions {
        match *action {
            Action::An(index, value) => {
                r.set_address_register(index, value);
            }
            Action::Idle(clocks) => {
                let Some(next) = device.clock.checked_add(u64::from(clocks)) else {
                    return (r, spec.queue, device, Err(TimingFault::ClockOverflow));
                };
                device.events.push(Event::Idle(device.clock, clocks));
                device.clock = next;
            }
            Action::Read(access, final_fetch) => {
                if device.clock.checked_add(RESERVE).is_none() {
                    return (r, spec.queue, device, Err(TimingFault::ClockOverflow));
                }
                if final_fetch {
                    let size = spec.queue[0] >> 6 & 3;
                    let mask = [0xff, 0xffff, u32::MAX][usize::from(size)];
                    let sign = [0x80, 0x8000, 0x8000_0000][usize::from(size)];
                    let value = spec.value & mask;
                    let nz = if value == 0 {
                        4
                    } else if value & sign != 0 {
                        8
                    } else {
                        0
                    };
                    r.set_status_register((r.status_register() & !15) | nz);
                }
                if let Some((index, address, value)) = device.mutation
                    && device.count == index
                {
                    device.words.insert(address & BUS_ADDRESS_MASK & !1, value);
                }
                let value = device.sample(access.bus_address);
                let wait = device.response_wait();
                device
                    .events
                    .push(Event::Read(access, device.clock, value, wait));
                device.clock += 4 + u64::from(wait);
                let index = device.count;
                device.count += 1;
                if device.fail == Some(index) {
                    return (
                        r,
                        spec.queue,
                        device,
                        Err(TimingFault::Bus {
                            access,
                            direction: Direction::Read,
                            source: index,
                        }),
                    );
                }
                if final_fetch {
                    r.pc = spec.next_pc;
                    let elapsed = device.clock - start;
                    return (
                        r,
                        [retained, value],
                        device,
                        Ok(TimedStep {
                            opcode: spec.queue[0],
                            clocks: elapsed,
                            exception: None,
                        }),
                    );
                }
                if access.kind == AccessKind::Prefetch {
                    retained = value;
                }
            }
        }
    }
    unreachable!()
}
