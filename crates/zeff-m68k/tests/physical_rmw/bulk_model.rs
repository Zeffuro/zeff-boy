use super::*;
use bulk_device::Device;
use device::Kind;

#[derive(Clone, Copy, Debug)]
pub(super) struct Spec {
    pub family: u16,
    pub size: u8,
    pub mode: u8,
    pub reg: usize,
    pub r: Registers,
    pub extension: u16,
    pub low: u16,
    pub operand: u32,
    pub first: u16,
    pub last: u16,
    pub start: u64,
    pub wait: u32,
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
    Transfer(Phase),
}
pub(super) struct Plan {
    pub actions: Vec<Action>,
    pub phases: Vec<Phase>,
    pub final_r: Registers,
    pub queue: [u16; 2],
    pub address: u32,
    pub operand: u32,
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
pub(super) fn set_active(r: &mut Registers, n: usize, v: u32) {
    if n < 7 {
        r.a[n] = v;
    } else if r.status_register() & 0x2000 == 0 {
        r.usp = v;
    } else {
        r.ssp = v;
    }
}
pub(super) fn status(mut r: Registers, sr: u16) -> Registers {
    r.set_status_register(sr);
    r
}
pub(super) fn sample(words: &BTreeMap<u32, u16>, a: u32) -> u16 {
    words.get(&(a & 0x00ff_fffe)).copied().unwrap_or(0x4e71)
}
pub(super) fn access(a: u32, fc: u8, k: AccessKind) -> BusAccess {
    fixture::access((a, a & 0x00ff_fffe, fc), k)
}
pub(super) fn report(b: &Device, at: usize) -> (Kind, u32) {
    b.report
        .filter(|(n, _, _)| *n == at)
        .map(|(_, k, t)| (k, t))
        .unwrap_or((Kind::Complete, b.elapsed[at]))
}
pub(super) fn values(size: u8) -> [u32; 8] {
    if size == 2 {
        [0, 1, 0x7fff, 0x8000, 0x8001, 0xffff, 0xfffe, 0x5555]
    } else {
        [
            0,
            1,
            0x8000,
            0x8000_0000,
            0x8000_0001,
            0xffff_ffff,
            0xffff_0000,
            0x5555_5555,
        ]
    }
}
impl Spec {
    pub fn new(family: u16, size: u8, mode: u8, reg: usize, supervisor: bool) -> Self {
        let mut r = Registers::new();
        r.d = std::array::from_fn(|i| 0xa100_0100 + i as u32 * 0x20);
        r.a = std::array::from_fn(|i| 0x10000 + i as u32 * 0x1000);
        r.d[3] = 2;
        r.usp = 0x40000;
        r.ssp = 0x80000;
        r.pc = 0x2000;
        r = status(r, if supervisor { 0x201f } else { 0x001f });
        Self {
            family,
            size,
            mode,
            reg,
            r,
            extension: match mode {
                3 => 0x10,
                4 => 0x3010,
                5 => 0x7000,
                6 => 0x1234,
                _ => 0x4e71,
            },
            low: 0x7000,
            operand: if size == 2 { 0x8001 } else { 0x0000_8000 },
            first: 0x4e71,
            last: 0x7002,
            start: 0,
            wait: 4,
        }
    }
    pub fn opcode(self) -> u16 {
        self.family
            | if self.size == 2 { 0x40 } else { 0x80 }
            | [0x10, 0x18, 0x20, 0x28, 0x30, 0x38, 0x39][self.mode as usize]
            | if self.mode < 5 { self.reg as u16 } else { 0 }
    }
    pub fn queued(self) -> u16 {
        if self.extensions() == 0 {
            self.first
        } else {
            self.extension
        }
    }
    pub fn extensions(self) -> usize {
        if self.mode == 6 {
            2
        } else {
            usize::from(self.mode >= 3)
        }
    }
    pub fn idle(self) -> bool {
        matches!(self.mode, 2 | 4)
    }
    pub fn count(self) -> usize {
        self.extensions() + if self.size == 2 { 3 } else { 5 }
    }
    pub fn canonical(self) -> u32 {
        self.r.pc.wrapping_add(2 + self.extensions() as u32 * 2)
    }
    pub fn operand_pc(self) -> u32 {
        self.r.pc.wrapping_add(match self.mode {
            6 => 6,
            5 => 4,
            2 if self.size == 2 => 4,
            _ => 2,
        })
    }
    pub fn index(self) -> u32 {
        let n = usize::from(self.extension >> 12 & 7);
        let raw = if self.extension & 0x8000 == 0 {
            self.r.d[n]
        } else {
            active(self.r, n)
        };
        if self.extension & 0x0800 == 0 {
            i32::from(raw as i16) as u32
        } else {
            raw
        }
    }
    pub fn address(self, low: u16) -> u32 {
        let b = active(self.r, self.reg);
        match self.mode {
            0 | 1 => b,
            2 => b.wrapping_sub(u32::from(self.size)),
            3 => b.wrapping_add_signed(i32::from(self.extension as i16)),
            4 => b
                .wrapping_add(self.index())
                .wrapping_add_signed(i32::from(self.extension as u8 as i8)),
            5 => i32::from(self.extension as i16) as u32,
            6 => (u32::from(self.extension) << 16) | u32::from(low),
            _ => unreachable!(),
        }
    }
    pub fn target(&mut self, a: u32) {
        let base = match self.mode {
            0 | 1 => a,
            2 => a.wrapping_add(u32::from(self.size)),
            3 => a.wrapping_sub(i32::from(self.extension as i16) as u32),
            4 => a
                .wrapping_sub(self.index())
                .wrapping_sub(i32::from(self.extension as u8 as i8) as u32),
            5 => {
                self.extension = a as u16;
                return;
            }
            6 => {
                self.extension = (a >> 16) as u16;
                self.low = a as u16;
                return;
            }
            _ => unreachable!(),
        };
        set_active(&mut self.r, self.reg, base);
    }
    pub fn source(&self, b: &mut Device, a: u32, v: u32) {
        if self.size == 4 {
            b.word(a, (v >> 16) as u16);
            b.word(a.wrapping_add(2), v as u16);
        } else {
            b.word(a, v as u16);
        }
    }
    pub fn seed(self) -> (PrefetchCpu, Device) {
        let mut b = Device::new(self.start);
        b.elapsed = [self.wait; 8];
        let p = self.r.pc;
        b.word(p, self.opcode());
        b.word(p.wrapping_add(2), self.queued());
        if self.mode == 6 {
            b.word(p.wrapping_add(4), self.low);
        }
        b.word(self.canonical(), self.first);
        b.word(self.canonical().wrapping_add(2), self.last);
        self.source(&mut b, self.address(self.low), self.operand);
        (
            PrefetchCpu::from_prefetched(
                self.r,
                [self.opcode(), self.queued()],
                CpuClocks(self.start),
            )
            .unwrap(),
            b,
        )
    }
}
pub(super) fn encoded() -> Vec<Spec> {
    let mut out = Vec::new();
    for family in [0x4200, 0x4600] {
        for size in [2, 4] {
            for mode in 0..7 {
                for reg in 0..if mode < 5 { 8 } else { 1 } {
                    out.push(Spec::new(family, size, mode, reg, false));
                }
            }
        }
    }
    out
}
pub(super) fn forms() -> Vec<Spec> {
    encoded().into_iter().filter(|s| s.reg == 0).collect()
}
pub(super) fn triggers() -> Vec<(Spec, usize)> {
    let mut out = Vec::new();
    for family in [0x4200, 0x4600] {
        for size in [2, 4] {
            for supervisor in [false, true] {
                let s = Spec::new(family, size, 1, 0, supervisor);
                for at in if size == 2 {
                    &[1, 2][..]
                } else {
                    &[2, 3, 4][..]
                } {
                    out.push((s, *at));
                }
            }
        }
    }
    out
}
pub(super) fn flags(r: Registers, result: u32, size: u8) -> Registers {
    let value = if size == 2 { result & 0xffff } else { result };
    status(
        r,
        r.status_register() & 0xfff0
            | if value == 0 { 4 } else { 0 }
            | if value & if size == 2 { 0x8000 } else { 0x8000_0000 } != 0 {
                8
            } else {
                0
            },
    )
}
