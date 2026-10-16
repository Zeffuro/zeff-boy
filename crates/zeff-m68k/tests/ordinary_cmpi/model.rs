use super::*;
use device::Device;

pub(super) const BUDGET: u64 = 4294967299;
#[derive(Clone, Copy, Debug)]
pub(super) struct Spec {
    pub family: u16,
    pub size: u8,
    pub mode: u16,
    pub reg: usize,
    pub r: Registers,
    pub immediate: u32,
    pub operand: u32,
    pub extension: [u16; 2],
    pub wait: u32,
}
pub(super) fn status(mut r: Registers, sr: u16) -> Registers {
    r.set_status_register(sr);
    r
}
pub(super) fn active(r: Registers, reg: usize) -> u32 {
    if reg < 7 {
        r.a[reg]
    } else if r.status_register() & 0x2000 == 0 {
        r.usp
    } else {
        r.ssp
    }
}
pub(super) fn set_active(r: &mut Registers, reg: usize, value: u32) {
    if reg < 7 {
        r.a[reg] = value;
    } else if r.status_register() & 0x2000 == 0 {
        r.usp = value;
    } else {
        r.ssp = value;
    }
}
pub(super) fn mask(size: u8) -> u32 {
    match size {
        1 => 0xff,
        2 => 0xffff,
        4 => 0xffff_ffff,
        _ => panic!("width"),
    }
}
pub(super) fn sample(words: &BTreeMap<u32, u16>, address: u32) -> u16 {
    words
        .get(&(address & 0x00ff_fffe))
        .copied()
        .unwrap_or(0x4e71)
}
pub(super) fn access(r: Registers, address: u32, data: bool, size: u8) -> BusAccess {
    let mut a = fixture::access(address, r.status_register() & 0x2000 != 0, data);
    if size == 1 {
        a.lanes = if address & 1 == 0 {
            ByteLanes::Upper
        } else {
            ByteLanes::Lower
        };
    }
    a
}
pub(super) fn index(r: Registers, extension: u16) -> u32 {
    let reg = usize::from(extension >> 12 & 7);
    let raw = if extension & 0x8000 == 0 {
        r.d[reg]
    } else {
        active(r, reg)
    };
    if extension & 0x0800 == 0 {
        i32::from(raw as i16) as u32
    } else {
        raw
    }
}
pub(super) fn address(s: Spec, extensions: [u16; 2]) -> u32 {
    let base = active(s.r, s.reg);
    match s.mode {
        0 => 0,
        2 | 3 => base,
        4 => base.wrapping_sub(s.increment()),
        5 => base.wrapping_add_signed(i32::from(extensions[0] as i16)),
        6 => base
            .wrapping_add(index(s.r, extensions[0]))
            .wrapping_add_signed(i32::from(extensions[0] as u8 as i8)),
        7 if s.reg == 0 => i32::from(extensions[0] as i16) as u32,
        7 if s.reg == 1 => u32::from(extensions[0]) << 16 | u32::from(extensions[1]),
        _ => panic!("source mode"),
    }
}
impl Spec {
    pub fn new(size: u8, mode: u16, reg: usize, supervisor: bool) -> Self {
        let mut r = fixture::registers(supervisor, 0x40000);
        r.a = std::array::from_fn(|i| 0x10000 + i as u32 * 0x1000);
        r.usp = 0x40000;
        r.ssp = 0x80000;
        r.d[3] = 2;
        r = status(r, if supervisor { 0x2510 } else { 0x0510 });
        Self {
            family: 0x0c00,
            size,
            mode,
            reg,
            r,
            immediate: 1,
            operand: 0x8000,
            extension: match mode {
                5 => [0x10, 0],
                6 => [0x3010, 0],
                7 if reg == 0 => [0x7000, 0],
                7 => [0x1234, 0x7000],
                _ => [0, 0],
            },
            wait: 0,
        }
    }
    pub fn opcode(self) -> u16 {
        self.family
            | match self.size {
                1 => 0,
                2 => 0x40,
                4 => 0x80,
                _ => panic!("width"),
            }
            | self.mode << 3
            | self.reg as u16
    }
    pub fn queue(self) -> [u16; 2] {
        [
            self.opcode(),
            if self.size == 4 {
                (self.immediate >> 16) as u16
            } else {
                self.immediate as u16
            },
        ]
    }
    pub fn extensions(self) -> usize {
        if self.mode == 7 && self.reg == 1 {
            2
        } else {
            usize::from(self.mode >= 5)
        }
    }
    pub fn immediate_words(self) -> usize {
        if self.size == 4 { 2 } else { 1 }
    }
    pub fn increment(self) -> u32 {
        if self.size == 1 && self.reg == 7 {
            2
        } else {
            u32::from(self.size)
        }
    }
    pub fn canonical(self) -> u32 {
        self.r
            .pc
            .wrapping_add(2 + 2 * (self.immediate_words() + self.extensions()) as u32)
    }
    pub fn selected(self) -> bool {
        self.family == 0x0c00 && self.mode != 0 && self.size != 1
    }
    pub fn target(&mut self, target: u32) {
        let increment = self.increment();
        let indexed = index(self.r, self.extension[0]);
        match self.mode {
            2 | 3 => set_active(&mut self.r, self.reg, target),
            4 => set_active(&mut self.r, self.reg, target.wrapping_add(increment)),
            5 => set_active(
                &mut self.r,
                self.reg,
                target.wrapping_sub(i32::from(self.extension[0] as i16) as u32),
            ),
            6 => set_active(
                &mut self.r,
                self.reg,
                target
                    .wrapping_sub(indexed)
                    .wrapping_sub(i32::from(self.extension[0] as u8 as i8) as u32),
            ),
            7 if self.reg == 0 => self.extension[0] = target as u16,
            7 => self.extension = [(target >> 16) as u16, target as u16],
            _ => panic!("source target"),
        }
    }
    pub fn source_word(self, bus: &mut Device, address: u32, value: u32) {
        if self.size == 4 {
            bus.word(address, (value >> 16) as u16);
            bus.word(address.wrapping_add(2), value as u16);
        } else if self.size == 2 {
            bus.word(address, value as u16);
        } else {
            bus.word(
                address,
                if address & 1 == 0 {
                    (value as u16 & 0xff) << 8 | 0x5a
                } else {
                    0xa500 | value as u16 & 0xff
                },
            );
        }
    }
    pub fn seed(mut self) -> (Self, Device) {
        let mut bus = Device::new(0);
        bus.waits = [self.wait; 12];
        let p = self.r.pc;
        bus.word(p, self.opcode());
        bus.word(p.wrapping_add(2), self.queue()[1]);
        let mut cursor = p.wrapping_add(2);
        if self.size == 4 {
            cursor = cursor.wrapping_add(2);
            bus.word(cursor, self.immediate as u16);
        }
        for n in 0..self.extensions() {
            cursor = cursor.wrapping_add(2);
            bus.word(cursor, self.extension[n]);
        }
        cursor = cursor.wrapping_add(2);
        bus.word(cursor, 0x4e71);
        bus.word(cursor.wrapping_add(2), 0x7201);
        if self.mode == 0 {
            self.r.d[self.reg] =
                (self.r.d[self.reg] & !mask(self.size)) | (self.operand & mask(self.size));
        } else {
            self.source_word(&mut bus, address(self, self.extension), self.operand);
        }
        (self, bus)
    }
}
pub(super) fn encoded() -> Vec<Spec> {
    let mut list = Vec::new();
    for size in [2, 4] {
        for mode in 2..=7 {
            for reg in 0..if mode == 7 { 2 } else { 8 } {
                list.push(Spec::new(size, mode, reg, false));
            }
        }
    }
    list
}
pub(super) fn forms() -> Vec<Spec> {
    let mut list = Vec::new();
    for size in [2, 4] {
        for mode in 2..=7 {
            for reg in 0..if mode == 7 { 2 } else { 1 } {
                list.push(Spec::new(size, mode, reg, false));
            }
        }
    }
    list
}
pub(super) fn bank(mut s: Spec, supervisor: bool, ccr: u16) -> Spec {
    s.r = status(s.r, if supervisor { 0x2500 } else { 0x0500 } | ccr);
    s
}
pub(super) fn pairs(size: u8) -> [(u32, u32); 10] {
    let sign = if size == 4 {
        0x8000_0000
    } else if size == 2 {
        0x8000
    } else {
        0x80
    };
    [
        (0, 0),
        (0, 1),
        (1, 0),
        (sign - 1, sign),
        (sign, sign - 1),
        (sign, sign),
        (mask(size), sign - 1),
        (sign + 1, sign),
        (0, sign),
        (if size == 4 { 0x10000 } else { sign - 1 }, 0),
    ]
}
