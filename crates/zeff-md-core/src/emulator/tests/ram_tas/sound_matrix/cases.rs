#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Kind {
    Immediate,
    PairLow,
    PairHigh,
    Ldi,
    BankM1,
    BankPsg,
    BankOpn2,
    BankRegister,
    LocalRead,
    LocalWrite,
    LocalPsg,
    LocalOpn2,
}

#[derive(Clone, Debug)]
pub(super) struct Case {
    pub group: &'static str,
    pub kind: Kind,
    pub residue: u64,
    pub edge: i64,
    pub lane: u32,
    pub disjoint: bool,
    pub epoch: Option<bool>,
    pub trace: bool,
}

impl Case {
    pub fn new(
        group: &'static str,
        kind: Kind,
        residue: u64,
        edge: i64,
        lane: u32,
        disjoint: bool,
    ) -> Self {
        Self {
            group,
            kind,
            residue,
            edge,
            lane,
            disjoint,
            epoch: None,
            trace: false,
        }
    }

    pub fn nops(&self) -> usize {
        (40..55)
            .find(|n| (112 + 28 * n) % 15 == self.residue)
            .unwrap() as usize
    }

    pub fn nominal_a(&self) -> u64 {
        112 + self.nops() as u64 * 28
    }

    pub fn nominal_s(&self) -> u64 {
        (self.nominal_a() as i64 + self.edge).div_euclid(15) as u64 * 15
            + if (self.nominal_a() as i64 + self.edge).rem_euclid(15) == 0 {
                0
            } else {
                15
            }
    }

    pub fn target(&self) -> u16 {
        0x800e + self.lane as u16 + if self.disjoint { 0x100 } else { 0 }
    }

    pub fn bank(&self) -> u16 {
        match self.kind {
            Kind::BankM1 => 0,
            Kind::BankPsg => 0x180,
            Kind::BankOpn2 => 0x140,
            Kind::BankRegister => 0x80,
            _ => 0x1ff,
        }
    }

    pub fn prefix_t(&self) -> u64 {
        37 + match self.kind {
            Kind::Immediate => 7,
            Kind::PairLow => 10,
            Kind::PairHigh => 13,
            Kind::Ldi => 11,
            Kind::BankM1 => 10,
            _ => 4,
        }
    }

    pub fn origin(&self) -> u64 {
        self.nominal_s() - self.prefix_t() * 15
    }

    pub fn program(&self) -> Vec<u8> {
        let target = self.target();
        let hl = match self.kind {
            Kind::PairLow | Kind::PairHigh => 0x55a6,
            Kind::Ldi | Kind::LocalRead | Kind::LocalWrite => 0x1000,
            Kind::BankPsg => 0xff11,
            Kind::BankOpn2 => 0xc000,
            Kind::BankRegister => 0x8000,
            Kind::LocalPsg => 0x7f11,
            Kind::LocalOpn2 => 0x4000,
            _ => target,
        };
        let mut program = vec![
            0x01,
            0x01,
            0x00,
            0x11,
            target as u8,
            (target >> 8) as u8,
            0x21,
            hl as u8,
            (hl >> 8) as u8,
            0x3e,
            0xa6,
        ];
        match self.kind {
            Kind::Immediate => program.extend([0x36, 0xa6]),
            Kind::PairLow | Kind::PairHigh => {
                let address = target - u16::from(self.kind == Kind::PairHigh);
                program.extend([0x22, address as u8, (address >> 8) as u8]);
            }
            Kind::Ldi => program.extend([0xed, 0xa0]),
            Kind::BankM1 => program.extend([0xc3, 0x08, 0x80]),
            Kind::LocalRead => program.push(0x7e),
            _ => program.push(0x77),
        }
        program.extend([0x76, 0x00]);
        program
    }

    pub fn expected_target(&self) -> (u32, bool, bool) {
        match self.kind {
            Kind::BankM1 => (4, false, true),
            Kind::Immediate | Kind::PairLow | Kind::PairHigh => (3, true, true),
            Kind::LocalRead | Kind::LocalWrite | Kind::LocalPsg | Kind::LocalOpn2 => {
                (3, false, false)
            }
            _ => (3, true, true),
        }
    }
}
