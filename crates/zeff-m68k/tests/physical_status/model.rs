use super::*;
use device::Kind;

pub(super) const MODES: [u8; 9] = [0, 1, 2, 3, 4, 5, 6, 7, 8];
#[derive(Clone, Copy, Debug)]
pub(super) struct Spec {
    pub context: u8,
    pub mode: u8,
    pub reg: usize,
    pub r: Registers,
    pub extension: u16,
    pub low: u16,
    pub word: u16,
    pub first: u16,
    pub last: u16,
    pub start: u64,
    pub wait: u32,
}
#[derive(Clone, Copy, Debug)]
pub(super) struct Phase {
    pub access: BusAccess,
    pub pc: u32,
    pub ird: u16,
    pub phase: InstructionPhase,
    pub before: Registers,
    pub during: Registers,
    pub value: u16,
}
#[derive(Clone, Copy, Debug)]
pub(super) enum Action {
    Idle(Registers),
    Read(Phase),
}
pub(super) struct Plan {
    pub actions: Vec<Action>,
    pub phases: Vec<Phase>,
    pub final_r: Registers,
    pub queue: [u16; 2],
}
pub(super) fn active(r: Registers, n: usize) -> u32 {
    if n < 7 {
        r.a[n]
    } else if r.status_register() & 0x2000 == 0 {
        r.usp
    } else {
        r.ssp
    }
}
pub(super) fn set_active(r: &mut Registers, n: usize, value: u32) {
    if n < 7 {
        r.a[n] = value;
    } else if r.status_register() & 0x2000 == 0 {
        r.usp = value;
    } else {
        r.ssp = value;
    }
}
pub(super) fn sample(words: &BTreeMap<u32, u16>, address: u32) -> u16 {
    words.get(&(address & 0xfffffe)).copied().unwrap_or(0x4e71)
}
pub(super) fn known(word: u16) -> bool {
    if word == 0x4e71 {
        return true;
    }
    let family = word & 0xffc0;
    let mode = word >> 3 & 7;
    let reg = word & 7;
    matches!(family, 0x44c0 | 0x46c0)
        && (mode == 0 || (2..=6).contains(&mode) || mode == 7 && reg <= 4)
}
impl Spec {
    pub fn new(context: u8, mode: u8, reg: usize) -> Self {
        let sr = if context == 0 { 0x071b } else { 0x271b };
        let mut r = registers(sr);
        r.pc = 0x1000;
        r.a = std::array::from_fn(|i| 0x10000 + i as u32 * 0x1000);
        r.usp = 0x40000;
        r.ssp = 0x80000;
        r.d[3] = 2;
        Self {
            context,
            mode,
            reg,
            r,
            extension: match mode {
                3 | 7 => 0x0010,
                4 | 8 => 0x3010,
                5 => 0x7000,
                6 => 0x1234,
                _ => 0x4afc,
            },
            low: 0x7000,
            word: 0x001f,
            first: 0x4e71,
            last: 0x7201,
            start: 0,
            wait: 0,
        }
    }
    pub fn opcode(self) -> u16 {
        let ea = [0x10, 0x18, 0x20, 0x28, 0x30, 0x38, 0x39, 0x3a, 0x3b][self.mode as usize];
        (if self.context == 2 { 0x46c0 } else { 0x44c0 })
            | ea
            | if self.mode < 5 { self.reg as u16 } else { 0 }
    }
    pub fn extensions(self) -> usize {
        if self.mode == 6 {
            2
        } else {
            usize::from(self.mode >= 3)
        }
    }
    pub fn idle(self) -> bool {
        matches!(self.mode, 2 | 4 | 8)
    }
    pub fn count(self) -> usize {
        self.extensions() + 3
    }
    pub fn canonical(self) -> u32 {
        self.r.pc.wrapping_add(2 + self.extensions() as u32 * 2)
    }
    pub fn index(self) -> u32 {
        let selector = usize::from(self.extension >> 12 & 7);
        let raw = if self.extension & 0x8000 == 0 {
            self.r.d[selector]
        } else {
            active(self.r, selector)
        };
        if self.extension & 0x0800 == 0 {
            i32::from(raw as i16) as u32
        } else {
            raw
        }
    }
    pub fn address(self, low: u16) -> u32 {
        let base = if self.mode >= 7 {
            self.r.pc.wrapping_add(2)
        } else {
            active(self.r, self.reg)
        };
        match self.mode {
            0 | 1 => base,
            2 => base.wrapping_sub(2),
            3 | 7 => base.wrapping_add_signed(i32::from(self.extension as i16)),
            4 | 8 => base
                .wrapping_add(self.index())
                .wrapping_add_signed(i32::from(self.extension as u8 as i8)),
            5 => i32::from(self.extension as i16) as u32,
            6 => u32::from(self.extension) << 16 | u32::from(low),
            _ => unreachable!(),
        }
    }
    pub fn target(&mut self, address: u32) {
        match self.mode {
            0 | 1 => set_active(&mut self.r, self.reg, address),
            2 => set_active(&mut self.r, self.reg, address.wrapping_add(2)),
            3 => set_active(
                &mut self.r,
                self.reg,
                address.wrapping_sub(i32::from(self.extension as i16) as u32),
            ),
            4 => {
                let base = address
                    .wrapping_sub(self.index())
                    .wrapping_sub(i32::from(self.extension as u8 as i8) as u32);
                set_active(&mut self.r, self.reg, base);
            }
            5 => self.extension = address as u16,
            6 => {
                self.extension = (address >> 16) as u16;
                self.low = address as u16;
            }
            7 => {
                self.r.pc = address.wrapping_sub(0x20);
                self.extension = 0x001e;
            }
            8 => {
                self.r.pc = address.wrapping_sub(0x20);
                self.extension = 0x301c;
                self.r.d[3] = 2;
            }
            _ => unreachable!(),
        }
    }
    pub fn odd(&mut self) {
        match self.mode {
            0..=4 => {
                let base = active(self.r, self.reg);
                set_active(&mut self.r, self.reg, base | 1);
            }
            5 | 7 | 8 => self.extension |= 1,
            6 => self.low |= 1,
            _ => unreachable!(),
        }
    }
    pub fn seed(self) -> (PrefetchCpu, Device) {
        let mut bus = Device::new(self.start);
        bus.elapsed = [4 + self.wait; 5];
        let p = self.r.pc;
        bus.word(p, self.opcode());
        bus.word(p.wrapping_add(2), self.extension);
        if self.extensions() > 0 {
            bus.word(p.wrapping_add(4), self.low);
        }
        bus.word(self.canonical(), self.first);
        bus.word(self.canonical().wrapping_add(2), self.last);
        bus.word(self.address(self.low), self.word);
        let cpu = PrefetchCpu::from_prefetched(
            self.r,
            [self.opcode(), self.extension],
            CpuClocks(self.start),
        )
        .unwrap();
        (cpu, bus)
    }
}
pub(super) fn encoded() -> Vec<Spec> {
    let mut result = Vec::new();
    for context in 0..3 {
        for mode in MODES {
            for reg in 0..if mode < 5 { 8 } else { 1 } {
                result.push(Spec::new(context, mode, reg));
            }
        }
    }
    result
}
pub(super) fn forms() -> Vec<Spec> {
    (0..3)
        .flat_map(|c| MODES.map(|m| Spec::new(c, m, 0)))
        .collect()
}
pub(super) fn publications() -> Vec<Spec> {
    let mut result = Vec::new();
    for mut s in forms() {
        if s.context < 2 {
            result.push(s);
        } else {
            for word in [0x001f, 0x201f, 0x801f, 0xa71f] {
                s.word = word;
                result.push(s);
            }
        }
    }
    result
}
pub(super) fn report(bus: &Device, at: usize) -> (Kind, u32) {
    if let Some((_, kind, elapsed)) = bus.report.filter(|(n, _, _)| *n == at) {
        (kind, elapsed)
    } else {
        let kind = match bus.failure.filter(|(n, _)| *n == at) {
            Some((_, true)) => Kind::Boundary,
            Some((_, false)) => Kind::Host,
            None => Kind::Complete,
        };
        (kind, bus.elapsed[at])
    }
}
fn phase(
    s: Spec,
    address: u32,
    kind: AccessKind,
    capture: (u32, u16, InstructionPhase),
    states: (Registers, Registers),
    words: &BTreeMap<u32, u16>,
) -> Phase {
    let (pc, ird, tag) = capture;
    let (before, during) = states;
    let program = kind == AccessKind::Prefetch || s.mode >= 7;
    let supervisor = during.status_register() & 0x2000 != 0;
    Phase {
        access: access(
            address,
            match (program, supervisor) {
                (false, false) => 1,
                (true, false) => 2,
                (false, true) => 5,
                (true, true) => 6,
            },
            kind,
        ),
        pc,
        ird,
        phase: tag,
        before,
        during,
        value: sample(words, address),
    }
}
pub(super) fn plan(s: Spec, bus: &Device) -> Plan {
    let mut actions = Vec::new();
    let mut phases = Vec::new();
    let mut words = bus.words.clone();
    let mut low = s.low;
    let mut r = s.r;
    let p = s.r.pc;
    if s.idle() {
        actions.push(Action::Idle(r));
    }
    for n in 0..s.extensions() {
        if let Some((at, address, value)) = bus.mutation
            && at == phases.len()
        {
            words.insert(address & 0xfffffe, value);
        }
        let address = p.wrapping_add(4 + n as u32 * 2);
        let pc = p.wrapping_add(if n == 1 {
            6
        } else if s.mode == 5 {
            4
        } else {
            2
        });
        let t = phase(
            s,
            address,
            AccessKind::Prefetch,
            (
                pc,
                s.opcode(),
                if n == 0 {
                    InstructionPhase::ExtensionFirst
                } else {
                    InstructionPhase::ExtensionSecond
                },
            ),
            (r, r),
            &words,
        );
        if s.mode == 6 && n == 0 {
            low = t.value;
        }
        phases.push(t);
        actions.push(Action::Read(t));
    }
    let address = s.address(low);
    if let Some((at, a, value)) = bus.mutation
        && at == phases.len()
    {
        words.insert(a & 0xfffffe, value);
    }
    let before = r;
    if s.mode == 1 {
        set_active(&mut r, s.reg, address.wrapping_add(2));
    }
    if s.mode == 2 {
        set_active(&mut r, s.reg, address);
    }
    let pc = p.wrapping_add([2, 2, 4, 2, 2, 4, 6, 2, 2][s.mode as usize]);
    let t = phase(
        s,
        address,
        AccessKind::Data,
        (pc, s.opcode(), InstructionPhase::OperandReadHigh),
        (before, r),
        &words,
    );
    let new_sr = if s.context == 2 {
        t.value & 0xa71f
    } else {
        (s.r.status_register() & 0xffe0) | (t.value & 31)
    };
    phases.push(t);
    actions.push(Action::Read(t));
    r = status(r, new_sr);
    actions.push(Action::Idle(r));
    actions.push(Action::Idle(r));
    let mut first = 0;
    for n in 0..2 {
        if let Some((at, a, value)) = bus.mutation
            && at == phases.len()
        {
            words.insert(a & 0xfffffe, value);
        }
        let address = s.canonical().wrapping_add(n * 2);
        let t = phase(
            s,
            address,
            AccessKind::Prefetch,
            (
                address,
                if n == 0 { s.opcode() } else { first },
                if n == 0 {
                    InstructionPhase::StatusFirst
                } else {
                    InstructionPhase::StatusSecond
                },
            ),
            (r, r),
            &words,
        );
        if n == 0 {
            first = t.value;
        }
        phases.push(t);
        actions.push(Action::Read(t));
    }
    r.pc = s.canonical();
    Plan {
        actions,
        final_r: r,
        queue: [first, phases.last().unwrap().value],
        phases,
    }
}
