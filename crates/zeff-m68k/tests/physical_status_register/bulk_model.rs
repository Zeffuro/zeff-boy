use super::*;
use bulk_device::{Device, Kind};

pub(super) const SOURCES: [u16; 8] = [0, 31, 16, 0x2000, 0x8000, 0xa71f, 0x58e0, 0xffff];
pub(super) const REPORTS: [(Kind, u32); 4] = [
    (Kind::Host, 6),
    (Kind::Boundary, 8),
    (Kind::Unavailable, 5),
    (Kind::Complete, 3),
];
pub(super) const ENTRY_REPORTS: [(Kind, u32); 6] = [
    (Kind::Complete, 0),
    (Kind::Complete, 1),
    (Kind::Complete, 3),
    (Kind::Host, 6),
    (Kind::Boundary, 8),
    (Kind::Unavailable, 5),
];
pub(super) const DETECTORS: [(Kind, u32); 5] = [
    (Kind::Unavailable, 0),
    (Kind::Unavailable, 5),
    (Kind::Host, 6),
    (Kind::Complete, 0),
    (Kind::Boundary, 8),
];

#[derive(Clone, Copy, Debug)]
pub(super) struct Spec {
    pub base: usize,
    pub family: u16,
    pub immediate: bool,
    pub reg: usize,
    pub r: Registers,
    pub queued: u16,
    pub first: u16,
    pub last: u16,
    pub start: u64,
    pub wait: u32,
    pub level: u8,
    pub edge: bool,
}
#[derive(Clone, Copy, Debug)]
pub(super) struct Phase {
    pub access: BusAccess,
    pub direction: Direction,
    pub pc: u32,
    pub ird: u16,
    pub phase: InstructionPhase,
    pub before: Registers,
    pub during: Registers,
    pub value: u16,
}
#[derive(Clone, Copy, Debug)]
pub(super) enum Action {
    Idle(u32),
    Publish(Registers),
    Transfer(Phase),
}
pub(super) struct Plan {
    pub actions: Vec<Action>,
    pub phases: Vec<Phase>,
    pub final_r: Registers,
    pub queue: [u16; 2],
}
pub(super) fn status(mut r: Registers, sr: u16) -> Registers {
    r.set_status_register(sr);
    r
}
pub(super) fn sample(words: &BTreeMap<u32, u16>, address: u32) -> u16 {
    words.get(&(address & 0x00ff_fffe)).copied().unwrap_or(0)
}
pub(super) fn access(a: u32, fc: u8, kind: AccessKind) -> BusAccess {
    BusAccess {
        logical_address: a,
        bus_address: a & 0x00ff_fffe,
        function_code: match fc {
            1 => FunctionCode::UserData,
            2 => FunctionCode::UserProgram,
            5 => FunctionCode::SupervisorData,
            6 => FunctionCode::SupervisorProgram,
            _ => panic!("status FC"),
        },
        kind,
        lanes: ByteLanes::Both,
    }
}
pub(super) fn report(b: &Device, at: usize) -> (Kind, u32) {
    b.report
        .filter(|(n, _, _)| *n == at)
        .map(|(_, k, n)| (k, n))
        .unwrap_or((Kind::Complete, b.elapsed[at]))
}
impl Spec {
    pub fn new(base: usize) -> Self {
        let seed = &first::seeds()[base];
        let opcode = seed.queue[0];
        let immediate = opcode & 63 == 60;
        let anchor = seed.r.pc.wrapping_add(if immediate { 4 } else { 2 });
        Self {
            base,
            family: opcode & 0xffc0,
            immediate,
            reg: usize::from(opcode & 7),
            r: seed.r,
            queued: seed.queue[1],
            first: sample(&seed.words, anchor),
            last: sample(&seed.words, anchor.wrapping_add(2)),
            start: seed.clock,
            wait: 4,
            level: 0,
            edge: false,
        }
    }
    pub fn opcode(self) -> u16 {
        self.family | if self.immediate { 60 } else { self.reg as u16 }
    }
    pub fn queued(self) -> u16 {
        self.queued
    }
    pub fn count(self) -> usize {
        if self.immediate { 3 } else { 2 }
    }
    pub fn anchor(self) -> u32 {
        self.r.pc.wrapping_add(if self.immediate { 4 } else { 2 })
    }
    pub fn source(self) -> u16 {
        if self.immediate {
            self.queued
        } else {
            self.r.d[self.reg] as u16
        }
    }
    pub fn value(&mut self, value: u16) {
        if self.immediate {
            self.queued = value;
        } else {
            self.r.d[self.reg] = self.r.d[self.reg] & 0xffff_0000 | u32::from(value);
        }
    }
    pub fn published(self) -> Registers {
        status(
            self.r,
            if self.family == 0x46c0 {
                self.source() & 0xa71f
            } else {
                self.r.status_register() & 0xffe0 | self.source() & 31
            },
        )
    }
    pub fn seed(self) -> (PrefetchCpu, Device) {
        let seed = &first::seeds()[self.base];
        let mut b = Device::new(self.start);
        b.words = seed.words.clone();
        let code: [u16; 4] =
            std::array::from_fn(|i| sample(&seed.words, seed.r.pc.wrapping_add(i as u32 * 2)));
        for i in 0..4 {
            b.words
                .remove(&(seed.r.pc.wrapping_add(i * 2) & 0x00ff_fffe));
        }
        for (i, word) in code.into_iter().enumerate() {
            b.word(self.r.pc.wrapping_add(i as u32 * 2), word);
        }
        b.word(self.r.pc, self.opcode());
        b.word(self.anchor(), self.first);
        b.word(self.anchor().wrapping_add(2), self.last);
        b.word(8, 0);
        b.word(10, 0x4000);
        b.word(0x4000, 0x4e71);
        b.word(0x4002, 0x7002);
        b.elapsed = [self.wait; 8];
        let mut cpu = PrefetchCpu::from_prefetched(
            self.r,
            [self.opcode(), self.queued()],
            CpuClocks(self.start),
        )
        .unwrap();
        bulk_private::set_interrupts(&mut cpu, self.level, self.edge);
        (cpu, b)
    }
}
pub(super) fn encoded() -> Vec<Spec> {
    (0..27).map(Spec::new).collect()
}
pub(super) fn all_banks() -> Vec<Spec> {
    let mut rows = encoded();
    for mut s in (18..27).map(Spec::new) {
        s.r = status(s.r, s.r.status_register() & !0x2000);
        rows.push(s);
    }
    rows
}
pub(super) fn representatives() -> Vec<Spec> {
    [0, 8, 9, 17, 18, 26].into_iter().map(Spec::new).collect()
}
pub(super) fn triggers() -> Vec<(Spec, usize)> {
    let rows: Vec<_> = encoded()
        .into_iter()
        .flat_map(|s| (0..s.count()).map(move |at| (s, at)))
        .collect();
    assert_eq!(rows.len(), 57);
    rows
}
