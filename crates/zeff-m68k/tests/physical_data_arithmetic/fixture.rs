use super::*;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Op {
    Add,
    Sub,
    And,
    Or,
}
pub(super) const OPS: [Op; 4] = [Op::Add, Op::Sub, Op::And, Op::Or];
impl Op {
    pub(super) fn top(self) -> u16 {
        match self {
            Self::Add => 0xd000,
            Self::Sub => 0x9000,
            Self::And => 0xc000,
            Self::Or => 0x8000,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Size {
    Byte,
    Word,
    Long,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Ea {
    Indirect,
    Post,
    Pre,
    Displacement,
    Index,
    AbsoluteWord,
    AbsoluteLong,
    PcDisplacement,
    PcIndex,
}

pub(super) const EAS: [Ea; 9] = [
    Ea::Indirect,
    Ea::Post,
    Ea::Pre,
    Ea::Displacement,
    Ea::Index,
    Ea::AbsoluteWord,
    Ea::AbsoluteLong,
    Ea::PcDisplacement,
    Ea::PcIndex,
];

#[derive(Clone, Copy, Debug)]
pub(super) struct Form {
    pub(super) op: Op,
    pub(super) size: Size,
    pub(super) ea: Ea,
}

pub(super) fn forms() -> Vec<Form> {
    OPS.into_iter()
        .flat_map(|op| {
            [Size::Word, Size::Long]
                .into_iter()
                .flat_map(move |size| EAS.into_iter().map(move |ea| Form { op, size, ea }))
        })
        .collect()
}

impl Form {
    pub(super) fn variable(self) -> bool {
        !matches!(
            self.ea,
            Ea::AbsoluteWord | Ea::AbsoluteLong | Ea::PcDisplacement | Ea::PcIndex
        )
    }
    pub(super) fn extensions(self) -> usize {
        match self.ea {
            Ea::Indirect | Ea::Post | Ea::Pre => 0,
            Ea::AbsoluteLong => 2,
            _ => 1,
        }
    }
    pub(super) fn idle(self) -> bool {
        matches!(self.ea, Ea::Pre | Ea::Index | Ea::PcIndex)
    }
    pub(super) fn count(self) -> usize {
        self.extensions() + 2 + usize::from(self.size == Size::Long)
    }
    pub(super) fn length(self) -> u32 {
        2 + 2 * self.extensions() as u32
    }
    pub(super) fn stride(self) -> u32 {
        if self.size == Size::Long { 4 } else { 2 }
    }
    pub(super) fn opcode(self, register: u16) -> u16 {
        let field = match self.ea {
            Ea::Indirect => 0x10 | register,
            Ea::Post => 0x18 | register,
            Ea::Pre => 0x20 | register,
            Ea::Displacement => 0x28 | register,
            Ea::Index => 0x30 | register,
            Ea::AbsoluteWord => 0x38,
            Ea::AbsoluteLong => 0x39,
            Ea::PcDisplacement => 0x3a,
            Ea::PcIndex => 0x3b,
        };
        self.op.top()
            | match self.size {
                Size::Byte => 0,
                Size::Word => 0x40,
                Size::Long => 0x80,
            }
            | field
    }
}

#[derive(Clone, Copy)]
pub(super) struct Case {
    pub(super) form: Form,
    pub(super) r: Registers,
    pub(super) register: u16,
    pub(super) destination: u16,
    pub(super) value: u32,
    pub(super) extension: u16,
    pub(super) low: u16,
    pub(super) next: u16,
    pub(super) last: u16,
    pub(super) start: u64,
}

impl Case {
    pub(super) fn new(form: Form) -> Self {
        Self {
            form,
            r: registers(false, 31),
            register: 3,
            destination: 5,
            value: 0x8000_0001,
            extension: match form.ea {
                Ea::Displacement | Ea::PcDisplacement => 0xfff0,
                Ea::Index | Ea::PcIndex => 0xe0f0,
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
    pub(super) fn opcode(self) -> u16 {
        self.form.opcode(self.register) | self.destination << 9
    }
    pub(super) fn retained(self) -> u16 {
        if self.form.extensions() == 0 {
            self.next
        } else {
            self.extension
        }
    }
    pub(super) fn seed(self) -> (PrefetchCpu, BusDevice) {
        let mut bus = Device::new(self.start);
        bus.word(self.r.pc, self.opcode());
        bus.word(self.r.pc.wrapping_add(2), self.retained());
        if self.form.extensions() == 2 {
            bus.word(self.r.pc.wrapping_add(4), self.low);
        }
        bus.word(self.r.pc.wrapping_add(self.form.length()), self.next);
        bus.word(self.r.pc.wrapping_add(self.form.length() + 2), self.last);
        let address = oracle::address(self, &bus.words);
        bus.operand(address, self.form.size, self.value);
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
    case.r.d[usize::from(case.destination)] = if case.form.size == Size::Long {
        pair.1
    } else {
        0xaabb_0000 | pair.1
    };
}
pub(super) fn odd(mut case: Case) -> Case {
    match case.form.ea {
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
    let address = oracle::address(case, &bus.words);
    address & 1 == 0
        && oracle::value(case, &bus.words)
            == case.value
                & if case.form.size == Size::Long {
                    u32::MAX
                } else {
                    65535
                }
}
