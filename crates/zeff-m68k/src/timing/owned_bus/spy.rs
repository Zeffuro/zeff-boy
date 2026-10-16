use super::*;
use crate::timing::test_bus::{Bus, cpu};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Owner {
    None,
    Reset,
    Rmw,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Begin {
    Complete,
    Unsupported,
    Error,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Event {
    BeginReset(u64),
    EndReset(u64),
    BeginRmw(BusAccess, u64),
    EndRmw(u64),
    Cleanup(Owner, u64),
    Read(BusAccess, u64, Owner),
    Write(BusAccess, u16, u64, Owner),
    Idle(u64, u32, Owner),
}

pub(super) struct Spy {
    pub bus: Bus,
    pub events: Vec<Event>,
    pub owner: Owner,
    pub reset: Begin,
    pub rmw: Begin,
    pub ignore_write: bool,
    pub pulse_writes: Vec<(u32, u16)>,
    access: Option<BusAccess>,
}

impl Default for Spy {
    fn default() -> Self {
        Self {
            bus: Bus::default(),
            events: vec![],
            owner: Owner::None,
            reset: Begin::Complete,
            rmw: Begin::Complete,
            ignore_write: false,
            pulse_writes: vec![],
            access: None,
        }
    }
}

impl ClockedBus for Spy {
    type Error = usize;
    fn read_cycle(&mut self, access: BusAccess, start: CpuClocks) -> CycleResult<u16, usize> {
        assert_ne!(self.owner, Owner::Reset);
        if self.owner == Owner::Rmw {
            assert_eq!(Some(access), self.access);
        }
        if access.kind == AccessKind::Prefetch {
            assert_eq!(self.owner, Owner::None);
        }
        self.events.push(Event::Read(access, start.0, self.owner));
        self.bus.read_cycle(access, start)
    }
    fn write_cycle(
        &mut self,
        access: BusAccess,
        value: u16,
        start: CpuClocks,
    ) -> CycleResult<(), usize> {
        assert_ne!(self.owner, Owner::Reset);
        if access.kind == AccessKind::Data {
            assert_eq!(self.owner, Owner::Rmw);
            assert_eq!(Some(access), self.access);
        }
        self.events
            .push(Event::Write(access, value, start.0, self.owner));
        let old = self.bus.words.get(&access.bus_address).copied();
        let result = self.bus.write_cycle(access, value, start);
        if self.owner == Owner::Rmw && self.ignore_write && result.result.is_ok() {
            if let Some(old) = old {
                self.bus.words.insert(access.bus_address, old);
            } else {
                self.bus.words.remove(&access.bus_address);
            }
        }
        result
    }
    fn idle(&mut self, start: CpuClocks, clocks: u32) {
        if self.owner == Owner::Rmw {
            assert_eq!(clocks, 2);
        }
        if self.owner == Owner::Reset {
            assert_eq!(clocks, 124);
        }
        self.events.push(Event::Idle(start.0, clocks, self.owner));
        self.bus.idle(start, clocks);
        if self.owner == Owner::Reset {
            for &(address, value) in &self.pulse_writes {
                self.bus.words.insert(address, value);
            }
        }
    }
    fn begin_reset_output(&mut self, start: CpuClocks) -> Result<BusResponse<()>, usize> {
        assert_eq!(self.owner, Owner::None);
        assert_eq!(self.bus.clock, start.0);
        self.events.push(Event::BeginReset(start.0));
        match self.reset {
            Begin::Complete => {
                self.owner = Owner::Reset;
                Ok(BusResponse::Complete(()))
            }
            Begin::Unsupported => Ok(BusResponse::Unsupported),
            Begin::Error => {
                self.owner = Owner::Reset;
                self.events.push(Event::Cleanup(self.owner, start.0));
                self.owner = Owner::None;
                Err(900)
            }
        }
    }
    fn end_reset_output(&mut self, end: CpuClocks) {
        assert_eq!(self.owner, Owner::Reset);
        assert_eq!(self.bus.clock, end.0);
        self.events.push(Event::EndReset(end.0));
        self.owner = Owner::None;
    }
    fn begin_rmw(&mut self, access: BusAccess, start: CpuClocks) -> Result<BusResponse<()>, usize> {
        assert_eq!(self.owner, Owner::None);
        assert_eq!(self.bus.clock, start.0);
        self.events.push(Event::BeginRmw(access, start.0));
        match self.rmw {
            Begin::Complete => {
                self.owner = Owner::Rmw;
                self.access = Some(access);
                Ok(BusResponse::Complete(()))
            }
            Begin::Unsupported => Ok(BusResponse::Unsupported),
            Begin::Error => {
                self.owner = Owner::Rmw;
                self.events.push(Event::Cleanup(self.owner, start.0));
                self.owner = Owner::None;
                Err(901)
            }
        }
    }
    fn end_rmw(&mut self, end: CpuClocks) {
        assert_eq!(self.owner, Owner::Rmw);
        assert_eq!(self.bus.clock, end.0);
        self.events.push(Event::EndRmw(end.0));
        self.owner = Owner::None;
        self.access = None;
    }
}

pub(super) fn shapes() -> Vec<(u16, usize)> {
    (2..=7)
        .flat_map(|mode| (0..if mode == 7 { 2 } else { 8 }).map(move |register| (mode, register)))
        .collect()
}

pub(super) fn seed(
    shape: (u16, usize),
    sr: u16,
    old: u8,
    parity: u32,
    index: u16,
) -> (PrefetchCpu, Spy, u32, u32) {
    let (mode, register) = shape;
    let mut cpu = cpu(sr);
    for register in 0..8 {
        cpu.registers.d[register] = 0x1234_0010 + register as u32 * 2;
    }
    for register in 0..7 {
        cpu.registers
            .set_address_register(register, 0x2345_3008 + register as u32 * 0x100 + parity);
    }
    if register == 7 {
        cpu.registers
            .set_address_register(7, cpu.registers.stack_pointer() + parity);
    }
    let pc = cpu.registers.pc;
    let base = cpu.registers.address_register(register).unwrap();
    let step = if register == 7 { 2 } else { 1 };
    let (extensions, address) = match (mode, register) {
        (2 | 3, _) => (vec![], base),
        (4, _) => (vec![], base - step),
        (5, _) => (vec![0xfffe], base - 2),
        (6, _) => {
            let index_register = usize::from((index >> 12) & 7);
            let raw = if index & 0x8000 == 0 {
                cpu.registers.d[index_register]
            } else {
                cpu.registers.address_register(index_register).unwrap()
            };
            let offset = if index & 0x0800 == 0 {
                i32::from(raw as i16) as u32
            } else {
                raw
            };
            (
                vec![index],
                base.wrapping_add(offset)
                    .wrapping_add_signed(i32::from(index as u8 as i8)),
            )
        }
        (7, 0) => (vec![0x8030 | parity as u16], 0xffff_8030 | parity),
        (7, 1) => (vec![0xabcd, 0x3010 | parity as u16], 0xabcd_3010 | parity),
        _ => unreachable!(),
    };
    let cursor = pc + 2 + extensions.len() as u32 * 2;
    let words: Vec<_> = extensions.into_iter().chain([0x7201, 0x7402]).collect();
    cpu.pipeline.as_mut().unwrap().words = [0x4ac0 | mode << 3 | register as u16, words[0]];
    let mut spy = Spy::default();
    spy.bus.handler(4);
    spy.bus.handler(8);
    spy.bus.handler(9);
    for (offset, &word) in words.iter().enumerate() {
        spy.bus
            .words
            .insert((pc + 2 + offset as u32 * 2) & BUS_ADDRESS_MASK, word);
    }
    spy.bus.words.insert(
        address & BUS_ADDRESS_MASK & !1,
        if address & 1 == 0 {
            u16::from(old) << 8 | 0x55
        } else {
            0x5500 | u16::from(old)
        },
    );
    (cpu, spy, address, cursor)
}

pub(super) fn reset_seed(sr: u16) -> (PrefetchCpu, Spy) {
    let (mut cpu, mut spy, _, _) = seed((2, 0), sr, 0, 0, 0x1010);
    cpu.pipeline.as_mut().unwrap().words = [0x4e70, 0x7201];
    spy.bus.words.insert(0x340104, 0x7402);
    (cpu, spy)
}

pub(super) fn flags(sr: u16, old: u8) -> u16 {
    (sr & !0x0f) | if old == 0 { 4 } else { 0 } | if old & 0x80 != 0 { 8 } else { 0 }
}
