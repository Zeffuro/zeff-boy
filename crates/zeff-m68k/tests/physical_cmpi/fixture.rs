use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Size {
    Byte,
    Word,
    Long,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Ea {
    Dn,
    Indirect,
    Post,
    Pre,
    Displacement,
    Index,
    AbsoluteWord,
    AbsoluteLong,
}
pub(super) const EAS: [Ea; 7] = [
    Ea::Indirect,
    Ea::Post,
    Ea::Pre,
    Ea::Displacement,
    Ea::Index,
    Ea::AbsoluteWord,
    Ea::AbsoluteLong,
];
#[derive(Clone, Copy, Debug)]
pub(super) struct Form {
    pub size: Size,
    pub ea: Ea,
}
pub(super) fn forms() -> Vec<Form> {
    [Size::Word, Size::Long]
        .into_iter()
        .flat_map(|size| EAS.into_iter().map(move |ea| Form { size, ea }))
        .collect()
}
impl Form {
    pub fn variable(self) -> bool {
        !matches!(self.ea, Ea::AbsoluteWord | Ea::AbsoluteLong)
    }
    pub fn extensions(self) -> usize {
        match self.ea {
            Ea::Dn | Ea::Indirect | Ea::Post | Ea::Pre => 0,
            Ea::AbsoluteLong => 2,
            _ => 1,
        }
    }
    pub fn immediates(self) -> usize {
        if self.size == Size::Long { 2 } else { 1 }
    }
    pub fn idle(self) -> bool {
        matches!(self.ea, Ea::Pre | Ea::Index)
    }
    pub fn count(self) -> usize {
        self.immediates()
            + self.extensions()
            + 1
            + if self.ea == Ea::Dn {
                0
            } else {
                1 + usize::from(self.size == Size::Long)
            }
    }
    pub fn length(self) -> u32 {
        2 + 2 * (self.immediates() + self.extensions()) as u32
    }
    pub fn stride(self) -> u32 {
        if self.size == Size::Long { 4 } else { 2 }
    }
    pub fn tail(self) -> bool {
        self.ea == Ea::Dn && self.size == Size::Long
    }
    pub fn opcode(self, register: u16) -> u16 {
        0x0c00
            | match self.size {
                Size::Byte => 0,
                Size::Word => 0x40,
                Size::Long => 0x80,
            }
            | match self.ea {
                Ea::Dn => register,
                Ea::Indirect => 0x10 | register,
                Ea::Post => 0x18 | register,
                Ea::Pre => 0x20 | register,
                Ea::Displacement => 0x28 | register,
                Ea::Index => 0x30 | register,
                Ea::AbsoluteWord => 0x38,
                Ea::AbsoluteLong => 0x39,
            }
    }
}
#[derive(Clone, Copy)]
pub(super) struct Case {
    pub form: Form,
    pub r: Registers,
    pub register: u16,
    pub value: u32,
    pub immediate: u32,
    pub extension: u16,
    pub low: u16,
    pub next: u16,
    pub last: u16,
    pub start: u64,
}
impl Case {
    pub fn new(form: Form) -> Self {
        Self {
            form,
            r: registers(false, 31),
            register: 3,
            value: 0x8000_0001,
            immediate: 1,
            extension: match form.ea {
                Ea::Displacement => 0xfff0,
                Ea::Index => 0xe0f0,
                Ea::AbsoluteWord => 0x9000,
                Ea::AbsoluteLong => 0xabcd,
                _ => 0,
            },
            low: 0x6000,
            next: 0x7201,
            last: 0x7480,
            start: 37,
        }
    }
    pub fn opcode(self) -> u16 {
        self.form.opcode(self.register)
    }
    pub fn retained(self) -> u16 {
        if self.form.size == Size::Long {
            (self.immediate >> 16) as u16
        } else {
            self.immediate as u16
        }
    }
    pub fn seed(self) -> (PrefetchCpu, BusDevice) {
        let mut bus = Device::new(self.start);
        bus.word(self.r.pc, self.opcode());
        bus.word(self.r.pc.wrapping_add(2), self.retained());
        if self.form.size == Size::Long {
            bus.word(self.r.pc.wrapping_add(4), self.immediate as u16);
        }
        let b = self.r.pc.wrapping_add(self.form.stride());
        if self.form.extensions() > 0 {
            bus.word(b.wrapping_add(2), self.extension);
        }
        if self.form.extensions() == 2 {
            bus.word(b.wrapping_add(4), self.low);
        }
        bus.word(self.r.pc.wrapping_add(self.form.length()), self.next);
        bus.word(self.r.pc.wrapping_add(self.form.length() + 2), self.last);
        if self.form.ea != Ea::Dn {
            let address = oracle::address(self, &bus.words);
            bus.operand(address, self.form.size, self.value);
        }
        let cpu = PrefetchCpu::from_prefetched(
            self.r,
            [self.opcode(), self.retained()],
            CpuClocks(self.start),
        )
        .unwrap();
        (cpu, bus)
    }
}
pub(super) fn registers(supervisor: bool, ccr: u16) -> Registers {
    let mut r = Registers::new();
    r.d = std::array::from_fn(|i| 0xaabb_cc00 | (i as u32 * 2));
    r.a = std::array::from_fn(|i| 0x1234_6000 + i as u32 * 16);
    r.usp = 0x3456_7000;
    r.ssp = 0x8765_8000;
    r.pc = 0x2345_2000;
    r.set_status_register(if supervisor { SUPERVISOR } else { 0 } | ccr);
    r
}
pub(super) const STATUSES: [u16; 4] = [0, 0x0115, 0x041a, 0x070f];
pub(super) fn pairs(size: Size) -> [(u32, u32); 12] {
    let (sign, mask) = if size == Size::Long {
        (0x8000_0000, u32::MAX)
    } else {
        (0x8000, 65535)
    };
    [
        (0, 0),
        (0, 1),
        (1, 0),
        (1, mask),
        (mask, 1),
        (sign - 1, 1),
        (sign, sign),
        (sign, sign - 1),
        (sign - 1, sign),
        (mask, sign - 1),
        (sign + 1, 0),
        (if size == Size::Long { 0x10000 } else { 0x7ffe }, 0),
    ]
}
pub(super) fn pair(case: &mut Case, pair: (u32, u32)) {
    case.value = pair.0;
    case.immediate = pair.1;
}
pub(super) fn odd(mut case: Case) -> Case {
    match case.form.ea {
        Ea::Dn => panic!("register has no operand address"),
        Ea::Indirect | Ea::Post | Ea::Pre => {
            let n = usize::from(case.register);
            case.r
                .set_address_register(n, case.r.address_register(n).unwrap() | 1);
        }
        Ea::AbsoluteLong => case.low |= 1,
        _ => case.extension ^= 1,
    }
    case
}

pub(super) fn inputs(case: Case) -> bool {
    let (_, bus) = case.seed();
    oracle::address(case, &bus.words) & 1 == 0
        && oracle::value(case, &bus.words)
            == case.value
                & if case.form.size == Size::Long {
                    u32::MAX
                } else {
                    65535
                }
}
