use super::*;
use device::{Device, Event};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Outcome {
    Complete,
    Host(BusAccess),
    Overflow,
}
pub(super) struct Case {
    pub label: &'static str,
    pub opcode: u16,
    pub r: Registers,
    pub queue: [u16; 2],
    pub bus: Device,
    pub events: Vec<Event>,
    pub final_r: Registers,
    pub final_queue: [u16; 2],
    pub clock: u64,
    pub duration: u64,
    pub outcome: Outcome,
}
pub(super) fn registers(supervisor: bool, active: u32) -> Registers {
    let mut r = Registers::new();
    r.d = std::array::from_fn(|i| 0xa100_0100 + i as u32 * 0x20);
    r.a = std::array::from_fn(|i| 0xb200_0200 + i as u32 * 0x40);
    r.usp = if supervisor { 0x5000 } else { active };
    r.ssp = if supervisor { active } else { 0x5000 };
    r.pc = 0x1000;
    r.set_status_register(if supervisor { 0x2714 } else { 0x0714 });
    r
}
pub(super) fn access(address: u32, supervisor: bool, data: bool) -> BusAccess {
    BusAccess {
        logical_address: address,
        bus_address: address & 0x00ff_fffe,
        function_code: match (supervisor, data) {
            (false, false) => FunctionCode::UserProgram,
            (false, true) => FunctionCode::UserData,
            (true, false) => FunctionCode::SupervisorProgram,
            (true, true) => FunctionCode::SupervisorData,
        },
        kind: if data {
            AccessKind::Data
        } else {
            AccessKind::Prefetch
        },
        lanes: ByteLanes::Both,
    }
}
impl Case {
    pub fn new(label: &'static str, opcode: u16, r: Registers, start: u64, long: bool) -> Self {
        let queue = [opcode, if long { 0 } else { 0x8000 }];
        let mut bus = Device::new(start);
        let words: &[(u32, u16)] = if long {
            &[
                (0x1000, opcode),
                (0x1002, 0),
                (0x1004, 1),
                (0x1006, 0x4e71),
                (0x1008, 0x7201),
                (0x3000, 0x8000),
                (0x3002, 0),
            ]
        } else {
            &[
                (0x1000, opcode),
                (0x1002, 0x8000),
                (0x1004, 0x4e71),
                (0x1006, 0x7201),
                (0x3000, 0x7fff),
            ]
        };
        for &(a, v) in words {
            bus.word(a, v);
        }
        Self {
            label,
            opcode,
            r,
            queue,
            bus,
            events: Vec::new(),
            final_r: r,
            final_queue: queue,
            clock: start,
            duration: 0,
            outcome: Outcome::Overflow,
        }
    }
    pub fn read(&mut self, at: u64, address: u32, sample: (bool, u16, u32)) {
        let (data, value, wait) = sample;
        let a = access(address, self.r.status_register() & 0x2000 != 0, data);
        let index = self
            .events
            .iter()
            .filter(|e| matches!(e, Event::Read(..)))
            .count();
        self.bus.waits[index] = wait;
        assert_eq!(self.bus.words[&a.bus_address], value);
        self.events.push(Event::Read(a, at, value, wait));
    }
    pub fn idle(&mut self, at: u64) {
        self.events.push(Event::Idle(at, 2));
    }
    pub fn finish(&mut self, active: u32, sr: u16, end: u64, outcome: Outcome) {
        if self.r.status_register() & 0x2000 == 0 {
            self.final_r.usp = active;
        } else {
            self.final_r.ssp = active;
        }
        self.final_r.set_status_register(sr);
        self.clock = end;
        self.duration = end - self.bus.clock;
        self.outcome = outcome;
        if outcome == Outcome::Complete {
            self.final_r.pc = if self.queue[1] == 0 { 0x1006 } else { 0x1004 };
            self.final_queue = [0x4e71, 0x7201];
        } else if matches!(outcome, Outcome::Host(_)) {
            self.bus.fail = Some(
                self.events
                    .iter()
                    .filter(|e| matches!(e, Event::Read(..)))
                    .count()
                    - 1,
            );
        }
    }
}
