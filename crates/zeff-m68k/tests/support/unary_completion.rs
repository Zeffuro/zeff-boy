use std::collections::BTreeMap;
use zeff_m68k::Registers;
use zeff_m68k::bus::{AccessKind, BUS_ADDRESS_MASK, BusAccess, ByteLanes, FunctionCode};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    Read(BusAccess, u16, u64),
    Write(BusAccess, u16, u64),
    Idle(u64, u32),
}

pub struct Plan {
    pub registers: Registers,
    pub queue: [u16; 2],
    pub clock: u64,
    pub words: BTreeMap<u32, u16>,
    pub events: Vec<Event>,
}

pub fn admitted(word: u16) -> bool {
    let (size, mode, reg) = (word >> 6 & 3, word >> 3 & 7, word & 7);
    matches!(word & 0xff00, 0x4000 | 0x4400)
        && size < 3
        && (size == 0 || mode == 0)
        && (mode == 0 || matches!(mode, 2..=6) || mode == 7 && reg <= 1)
}

struct Device {
    clock: u64,
    bank: bool,
    words: BTreeMap<u32, u16>,
    events: Vec<Event>,
}

impl Device {
    fn access(&self, address: u32, program: bool) -> BusAccess {
        BusAccess {
            logical_address: address,
            bus_address: address & BUS_ADDRESS_MASK & !1,
            function_code: match (self.bank, program) {
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
            lanes: if program {
                ByteLanes::Both
            } else if address & 1 == 0 {
                ByteLanes::Upper
            } else {
                ByteLanes::Lower
            },
        }
    }

    fn read(&mut self, address: u32, program: bool) -> u16 {
        let access = self.access(address, program);
        let value = self
            .words
            .get(&access.bus_address)
            .copied()
            .unwrap_or(0x4e71);
        self.events.push(Event::Read(access, value, self.clock));
        self.clock += 4;
        value
    }

    fn write(&mut self, address: u32, value: u16) {
        let access = self.access(address, false);
        let old = self
            .words
            .get(&access.bus_address)
            .copied()
            .unwrap_or(0x4e71);
        let (payload, word) = if address & 1 == 0 {
            (value << 8, old & 0xff | value << 8)
        } else {
            (value, old & 0xff00 | value)
        };
        self.events.push(Event::Write(access, payload, self.clock));
        self.words.insert(access.bus_address, word);
        self.clock += 4;
    }

    fn idle(&mut self) {
        self.events.push(Event::Idle(self.clock, 2));
        self.clock += 2;
    }
}

pub fn plan(
    word: u16,
    original: Registers,
    queued: u16,
    start: u64,
    words: BTreeMap<u32, u16>,
) -> Plan {
    assert!(admitted(word));
    let (size, mode, reg) = (word >> 6 & 3, word >> 3 & 7, usize::from(word & 7));
    let bits = [8, 16, 32][size as usize];
    let mask = ((1u64 << bits) - 1) as u32;
    let mut r = original;
    let mut b = Device {
        clock: start,
        bank: original.supervisor(),
        words,
        events: Vec::new(),
    };
    let extensions = if mode == 7 && reg == 1 {
        2
    } else {
        u32::from(mode >= 5)
    };
    if matches!(mode, 4 | 6) {
        b.idle();
    }
    let mut retained = queued;
    let mut low = 0;
    for index in 0..extensions {
        retained = b.read(original.pc.wrapping_add(4 + index * 2), true);
        if index == 0 {
            low = retained;
        }
    }
    let mut ea = 0;
    let source = if mode == 0 {
        original.d[reg] & mask
    } else {
        let base = original.address_register(reg).unwrap();
        let step = if reg == 7 { 2 } else { 1 };
        ea = match (mode, reg) {
            (2 | 3, _) => base,
            (4, _) => base.wrapping_sub(step),
            (5, _) => base.wrapping_add(queued as i16 as i32 as u32),
            (6, _) => {
                let n = usize::from(queued >> 12 & 7);
                let index = if queued & 0x8000 == 0 {
                    original.d[n]
                } else {
                    original.address_register(n).unwrap()
                };
                let index = if queued & 0x800 == 0 {
                    index as i16 as i32 as u32
                } else {
                    index
                };
                base.wrapping_add(index)
                    .wrapping_add(queued as i8 as i32 as u32)
            }
            (7, 0) => queued as i16 as i32 as u32,
            (7, 1) => u32::from(queued) << 16 | u32::from(low),
            _ => unreachable!(),
        };
        if mode == 3 {
            r.set_address_register(reg, ea.wrapping_add(step));
        }
        if mode == 4 {
            r.set_address_register(reg, ea);
        }
        let sampled = b.read(ea, false);
        u32::from(if ea & 1 == 0 {
            sampled >> 8
        } else {
            sampled & 255
        })
    };
    let modulus = 1u64 << bits;
    let value = u64::from(source);
    let incoming_x = u64::from(word & 0xff00 == 0x4000 && original.status_register() & 16 != 0);
    let result = (modulus.wrapping_sub(value + incoming_x) & u64::from(mask)) as u32;
    let borrow = u16::from(value + incoming_x != 0);
    let overflow = u16::from(value == modulus / 2 && incoming_x == 0);
    let zero = result == 0 && (word & 0xff00 == 0x4400 || original.status_register() & 4 != 0);
    r.set_status_register(
        (original.status_register() & !31)
            | borrow << 4
            | u16::from(result & (1 << (bits - 1)) != 0) << 3
            | u16::from(zero) << 2
            | overflow << 1
            | borrow,
    );
    let anchor = original.pc.wrapping_add(2 + extensions * 2);
    let fetched = b.read(anchor.wrapping_add(2), true);
    if mode == 0 {
        r.d[reg] = original.d[reg] & !mask | result;
        if size == 2 {
            b.idle();
        }
    } else {
        b.write(ea, result as u16);
    }
    r.pc = anchor;
    Plan {
        registers: r,
        queue: [retained, fetched],
        clock: b.clock,
        words: b.words,
        events: b.events,
    }
}
