use super::*;
use device::{Device, Kind};
pub(super) const VALUES: [[u32; 8]; 3] = [
    [0, 1, 127, 128, 255, 85, 170, 64],
    [0, 1, 32767, 32768, 65535, 21845, 43690, 16384],
    [
        0,
        1,
        65536,
        32768,
        2147483648,
        u32::MAX,
        1431655765,
        2863311530,
    ],
];
pub(super) const REPORTS: [(Kind, u32); 3] =
    [(Kind::Host, 6), (Kind::Boundary, 8), (Kind::Unavailable, 5)];
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
pub(super) fn status(mut r: Registers, sr: u16) -> Registers {
    r.set_status_register(sr);
    r
}
pub(super) fn address(r: &Registers, n: usize) -> u32 {
    if n < 7 {
        r.a[n]
    } else if r.supervisor() {
        r.ssp
    } else {
        r.usp
    }
}
pub(super) fn set_address(r: &mut Registers, n: usize, v: u32) {
    if n < 7 {
        r.a[n] = v
    } else if r.supervisor() {
        r.ssp = v
    } else {
        r.usp = v
    }
}
pub(super) fn access(a: u32, fc: u8, kind: AccessKind) -> BusAccess {
    BusAccess {
        logical_address: a,
        bus_address: a & BUS_ADDRESS_MASK & !1,
        kind,
        lanes: ByteLanes::Both,
        function_code: match fc {
            1 => FunctionCode::UserData,
            2 => FunctionCode::UserProgram,
            5 => FunctionCode::SupervisorData,
            6 => FunctionCode::SupervisorProgram,
            _ => panic!("FC"),
        },
    }
}
pub(super) fn report(b: &Device, at: usize) -> (Kind, u32) {
    b.report
        .filter(|(i, _, _)| *i == at)
        .map(|(_, k, n)| (k, n))
        .unwrap_or((Kind::Complete, b.elapsed[at]))
}
pub(super) fn words() -> [u16; 66] {
    std::array::from_fn(|i| {
        if i < 24 {
            0x4a00 | ((i / 8) as u16) << 6 | (i % 8) as u16
        } else if i < 64 {
            0x4a00 | (((i - 24) / 8 + 2) as u16) << 3 | ((i - 24) % 8) as u16
        } else {
            0x4a38 + (i - 64) as u16
        }
    })
}
pub(super) fn encoded() -> impl Iterator<Item = Spec> {
    [false, true]
        .into_iter()
        .flat_map(|bank| words().into_iter().map(move |w| Spec::new(w, bank)))
}
pub(super) fn triggers() -> impl Iterator<Item = (Spec, usize)> {
    encoded().flat_map(|s| (0..s.count()).map(move |at| (s, at)))
}
#[derive(Clone, Copy, Debug)]
pub(super) struct Spec {
    pub opcode: u16,
    pub bank: bool,
    pub ccr: u16,
    pub value: u32,
    pub lane: u32,
    pub pc: u32,
    pub desired: u32,
    pub ssp: u32,
    pub preserve_sp: bool,
    pub ext: Option<u16>,
    pub wait: u32,
    pub nword: u16,
    pub fword: u16,
    pub mask: u16,
    pub upper: u32,
    pub intended: Option<u32>,
    pub start: u64,
}
impl Spec {
    pub fn new(opcode: u16, bank: bool) -> Self {
        let size = (opcode >> 6 & 3) as usize;
        Self {
            opcode,
            bank,
            ccr: 31,
            value: 0,
            lane: 0,
            pc: 0x2000,
            desired: 0x1000,
            ssp: 0xa000,
            preserve_sp: false,
            ext: None,
            wait: 4,
            nword: 0x7001,
            fword: 0x4e71,
            mask: 5,
            upper: [0xa5b6c7, 0xa5b6, 0][size],
            intended: None,
            start: 0,
        }
    }
    pub fn mode(self) -> u16 {
        self.opcode >> 3 & 7
    }
    pub fn reg(self) -> usize {
        (self.opcode & 7) as usize
    }
    pub fn size(self) -> usize {
        (self.opcode >> 6 & 3) as usize
    }
    pub fn bits(self) -> u32 {
        [8, 16, 32][self.size()]
    }
    pub fn extensions(self) -> usize {
        if self.mode() < 5 {
            0
        } else if self.opcode == 0x4a39 {
            2
        } else {
            1
        }
    }
    pub fn count(self) -> usize {
        self.extensions() + usize::from(self.mode() != 0) + 1
    }
    pub fn anchor(self) -> u32 {
        self.pc.wrapping_add(2 + 2 * self.extensions() as u32)
    }
    pub fn idle(self) -> u64 {
        if matches!(self.mode(), 4 | 6) { 2 } else { 0 }
    }
    pub fn vi(&mut self, vi: usize) {
        self.value = VALUES[self.size()][vi];
        self.lane = (vi & 1) as u32;
        self.desired = 0x1000 | self.lane;
    }
    pub fn seed(self) -> (Registers, [u16; 2], Device, [u16; 2], u32) {
        let mut r = Registers::new();
        r.d = std::array::from_fn(|i| 0x11111111 * (i as u32 + 1));
        r.a = std::array::from_fn(|i| 0x3000 + i as u32 * 0x1000);
        r.usp = 0xb000;
        r.ssp = self.ssp;
        r.pc = self.pc;
        r = status(
            r,
            (if self.bank { 0x2000 } else { 0 }) | self.mask << 8 | self.ccr,
        );
        let mode = self.mode();
        let reg = self.reg();
        let desired = self.desired;
        if mode == 0 {
            r.d[reg] = (if self.bits() == 32 {
                0
            } else {
                self.upper << self.bits()
            }) | self.value;
        }
        if mode == 6 {
            r.d[3] = 2;
        }
        let ext = match mode {
            5 => [16, 0],
            6 => [self.ext.unwrap_or(0x3010), 0],
            7 if reg == 0 => [desired as u16, 0],
            7 => [(desired >> 16) as u16, desired as u16],
            _ => [0, 0],
        };
        if (2..=6).contains(&mode)
            && !(self.preserve_sp && self.bank && reg == 7)
            && (mode != 6 || self.ext.is_none())
        {
            let adjustment = match mode {
                4 => 0u32.wrapping_sub(if reg == 7 { 2 } else { 1 }),
                5 => (ext[0] as i16 as i32) as u32,
                6 => index(&r, ext[0]).wrapping_add((ext[0] as i8 as i32) as u32),
                _ => 0,
            };
            set_address(&mut r, reg, desired.wrapping_sub(adjustment));
        }
        let ea = ea(self, &r, ext, ext[1]);
        let mut b = Device::new(self.start);
        b.words = Ram::default();
        b.word(self.pc, self.opcode);
        for (i, &word) in ext.iter().take(self.extensions()).enumerate() {
            b.word(self.pc.wrapping_add(2 + 2 * i as u32), word);
        }
        b.word(self.anchor(), self.nword);
        b.word(self.anchor().wrapping_add(2), self.fword);
        for (a, v) in [(8, 0), (10, 0x4000), (0x4000, 0x4e71), (0x4002, 0x7002)] {
            b.word(a, v);
        }
        if mode != 0 {
            b.word(
                ea,
                if ea & 1 != 0 {
                    0x5a00 | self.value as u16
                } else {
                    (self.value as u16) << 8 | 0x5a
                },
            );
        }
        if let Some(at) = self.intended {
            for (a, v) in [
                (8, (at >> 16) as u16),
                (10, at as u16),
                (at, 0x4e71),
                (at.wrapping_add(2), 0x7002),
            ] {
                b.word(a, v);
            }
        }
        b.elapsed = [self.wait; 8];
        b.entry_wait = self.wait;
        (
            r,
            [
                self.opcode,
                if self.extensions() == 0 {
                    self.nword
                } else {
                    ext[0]
                },
            ],
            b,
            ext,
            ea,
        )
    }
}
pub(super) fn index(r: &Registers, x: u16) -> u32 {
    let v = if x & 0x8000 != 0 {
        address(r, (x >> 12 & 7) as usize)
    } else {
        r.d[(x >> 12 & 7) as usize]
    };
    if x & 0x800 != 0 {
        v
    } else {
        v as i16 as i32 as u32
    }
}
pub(super) fn ea(s: Spec, r: &Registers, x: [u16; 2], low: u16) -> u32 {
    let base = address(r, s.reg());
    match s.mode() {
        4 => base.wrapping_sub(if s.reg() == 7 { 2 } else { 1 }),
        5 => base.wrapping_add(x[0] as i16 as i32 as u32),
        6 => base
            .wrapping_add(index(r, x[0]))
            .wrapping_add(x[0] as i8 as i32 as u32),
        7 if s.reg() == 0 => x[0] as i16 as i32 as u32,
        7 => u32::from(x[0]) << 16 | u32::from(low),
        _ => base,
    }
}
