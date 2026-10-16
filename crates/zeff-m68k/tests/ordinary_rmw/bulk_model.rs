use super::*;
use bulk_device::Device;

pub(super) const BUDGET: u64 = 4294967299;
pub(super) const WORD_VALUES: [u32; 8] = [0, 1, 0x7fff, 0x8000, 0x8001, 0xffff, 0xfffe, 0x5555];
pub(super) const LONG_VALUES: [u32; 8] = [
    0, 1, 0x8000, 0x80000000, 0x80000001, 0xffffffff, 0xffff0000, 0x55555555,
];
pub(super) const BYTE_VALUES: [u32; 8] = [0, 1, 0x7f, 0x80, 0x81, 0xff, 0xfe, 0x55];

#[derive(Clone, Copy, Debug)]
pub(super) struct Spec {
    pub family: u16,
    pub size: u8,
    pub mode: u16,
    pub reg: usize,
    pub r: Registers,
    pub operand: u32,
    pub extension: [u16; 2],
    pub retained: u16,
    pub fetched: u16,
    pub wait: u32,
    pub target_ea: Option<u32>,
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
        4 => u32::MAX,
        _ => panic!("width"),
    }
}

pub(super) fn values(size: u8) -> [u32; 8] {
    match size {
        1 => BYTE_VALUES,
        2 => WORD_VALUES,
        4 => LONG_VALUES,
        _ => panic!("width"),
    }
}

pub(super) fn sample(words: &BTreeMap<u32, u16>, address: u32) -> u16 {
    words
        .get(&(address & 0x00ff_fffe))
        .copied()
        .unwrap_or(0x4e71)
}

pub(super) fn sized_value(words: &BTreeMap<u32, u16>, address: u32, size: u8) -> u32 {
    let high = sample(words, address);
    match size {
        1 => u32::from(if address & 1 == 0 {
            high >> 8
        } else {
            high & 0xff
        }),
        2 => u32::from(high),
        4 => u32::from(high) << 16 | u32::from(sample(words, address.wrapping_add(2))),
        _ => panic!("width"),
    }
}

pub(super) fn access(r: Registers, address: u32, data: bool, size: u8) -> BusAccess {
    let supervisor = r.status_register() & 0x2000 != 0;
    BusAccess {
        logical_address: address,
        bus_address: address & 0x00ff_fffe,
        function_code: match (supervisor, data) {
            (false, true) => FunctionCode::UserData,
            (false, false) => FunctionCode::UserProgram,
            (true, true) => FunctionCode::SupervisorData,
            (true, false) => FunctionCode::SupervisorProgram,
        },
        kind: if data {
            AccessKind::Data
        } else {
            AccessKind::Prefetch
        },
        lanes: if size != 1 {
            ByteLanes::Both
        } else if address & 1 == 0 {
            ByteLanes::Upper
        } else {
            ByteLanes::Lower
        },
    }
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

pub(super) fn address(s: Spec, extension: [u16; 2]) -> u32 {
    let base = active(s.r, s.reg);
    match s.mode {
        0 => 0,
        2 | 3 => base,
        4 => base.wrapping_sub(s.increment()),
        5 => base.wrapping_add_signed(i32::from(extension[0] as i16)),
        6 => base
            .wrapping_add(index(s.r, extension[0]))
            .wrapping_add_signed(i32::from(extension[0] as u8 as i8)),
        7 if s.reg == 0 => i32::from(extension[0] as i16) as u32,
        7 if s.reg == 1 => u32::from(extension[0]) << 16 | u32::from(extension[1]),
        _ => panic!("mode"),
    }
}

impl Spec {
    pub fn new(family: u16, size: u8, mode: u16, reg: usize, supervisor: bool) -> Self {
        let mut r = fixture::state(
            0x10000,
            if supervisor { 0x251f } else { 0x051f },
            0x40000,
            0x80000,
            0x2000,
        );
        r.a = std::array::from_fn(|i| 0x10000 + i as u32 * 0x1000);
        r.d[3] = 2;
        Self {
            family,
            size,
            mode,
            reg,
            r,
            operand: if family == 0x4600 { 0x8000 } else { 0x80000001 },
            extension: match mode {
                5 => [0x10, 0],
                6 => [0x3010, 0],
                7 if reg == 0 => [0x7000, 0],
                7 => [0x1234, 0x7000],
                _ => [0, 0],
            },
            retained: 0x4e71,
            fetched: 0x7201,
            wait: 0,
            target_ea: None,
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

    pub fn extensions(self) -> usize {
        if self.mode == 7 && self.reg == 1 {
            2
        } else {
            usize::from(self.mode >= 5)
        }
    }

    pub fn increment(self) -> u32 {
        if self.size == 1 && self.reg == 7 {
            2
        } else {
            u32::from(self.size)
        }
    }

    pub fn canonical(self) -> u32 {
        self.r.pc.wrapping_add(2 + 2 * self.extensions() as u32)
    }
    pub fn queue(self) -> [u16; 2] {
        [
            self.opcode(),
            if self.extensions() == 0 {
                self.retained
            } else {
                self.extension[0]
            },
        ]
    }
    pub fn staged(self) -> bool {
        self.mode != 0 && self.size != 1 && matches!(self.family, 0x4000 | 0x4200 | 0x4400 | 0x4600)
    }
    pub fn f_index(self) -> usize {
        if self.mode == 0 {
            0
        } else {
            self.extensions() + if self.size == 4 { 2 } else { 1 }
        }
    }
    pub fn callbacks(self) -> usize {
        if self.mode == 0 {
            1
        } else {
            self.f_index()
                + 1
                + if self.family == 0x4a00 {
                    0
                } else if self.size == 4 {
                    2
                } else {
                    1
                }
        }
    }

    pub fn bank(mut self, supervisor: bool, ccr: u16) -> Self {
        self.r.set_status_register(if supervisor {
            0x2500 | ccr
        } else {
            0x0500 | ccr
        });
        self
    }

    pub fn target(&mut self, target: u32) {
        let value = match self.mode {
            2 | 3 => target,
            4 => target.wrapping_add(self.increment()),
            5 => target.wrapping_sub(i32::from(self.extension[0] as i16) as u32),
            6 => {
                assert!(
                    self.extension[0] & 0x8000 == 0
                        || usize::from(self.extension[0] >> 12 & 7) != self.reg
                );
                target
                    .wrapping_sub(index(self.r, self.extension[0]))
                    .wrapping_sub(i32::from(self.extension[0] as u8 as i8) as u32)
            }
            7 if self.reg == 0 => {
                assert_eq!(i32::from(target as i16) as u32, target);
                self.extension[0] = target as u16;
                self.target_ea = Some(target);
                return;
            }
            7 => {
                self.extension = [(target >> 16) as u16, target as u16];
                self.target_ea = Some(target);
                return;
            }
            _ => panic!("target"),
        };
        set_active(&mut self.r, self.reg, value);
        self.target_ea = Some(target);
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
        if self.mode == 0 {
            self.r.d[self.reg] =
                self.r.d[self.reg] & !mask(self.size) | self.operand & mask(self.size);
        } else {
            self.source_word(&mut bus, address(self, self.extension), self.operand);
        }
        let p = self.r.pc;
        bus.word(p, self.opcode());
        bus.word(p.wrapping_add(2), self.queue()[1]);
        if self.extensions() == 2 {
            bus.word(p.wrapping_add(4), self.extension[1]);
        }
        bus.word(self.canonical(), self.retained);
        bus.word(self.canonical().wrapping_add(2), self.fetched);
        if let Some(target) = self.target_ea {
            let actual = if self.extensions() == 2 {
                [self.queue()[1], sample(&bus.words, p.wrapping_add(4))]
            } else {
                self.extension
            };
            assert_eq!(address(self, actual), target);
        }
        (self, bus)
    }
}

pub(super) fn encoded() -> Vec<Spec> {
    let mut result = Vec::new();
    for family in [0x4200, 0x4600] {
        for size in [2, 4] {
            for mode in 2..=7 {
                for reg in 0..if mode == 7 { 2 } else { 8 } {
                    result.push(Spec::new(family, size, mode, reg, false));
                }
            }
        }
    }
    result
}

pub(super) fn forms() -> Vec<Spec> {
    encoded()
        .into_iter()
        .filter(|s| s.mode == 7 || s.reg == 0)
        .collect()
}
