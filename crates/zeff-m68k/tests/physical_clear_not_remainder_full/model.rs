use super::{InstructionPhase, Registers};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Failure {
    Complete,
    Host,
    Boundary,
    Unavailable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Report {
    pub kind: Failure,
    pub elapsed: u32,
}

impl Default for Report {
    fn default() -> Self {
        Self {
            kind: Failure::Complete,
            elapsed: 4,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Profile {
    pub clear: bool,
    pub bits: u8,
    pub mode: u8,
    pub register: usize,
}

impl Profile {
    pub fn decode(word: u16) -> Option<Self> {
        let page = word & 0xff00;
        let size = word >> 6 & 3;
        let mode = (word >> 3 & 7) as u8;
        let register = usize::from(word & 7);
        let memory = matches!(mode, 2..=6) || mode == 7 && register < 2;
        (matches!(page, 0x4200 | 0x4600) && size < 3 && (mode == 0 || size == 0 && memory)).then(
            || Self {
                clear: page == 0x4200,
                bits: [8, 16, 32][usize::from(size)],
                mode,
                register,
            },
        )
    }
    pub fn memory(self) -> bool {
        self.mode != 0
    }
    pub fn extensions(self) -> usize {
        if self.mode == 7 && self.register == 1 {
            2
        } else {
            usize::from(self.mode >= 5)
        }
    }
    pub fn count(self) -> usize {
        self.extensions() + 1 + usize::from(self.memory()) * 2
    }
    pub fn length(self) -> u32 {
        2 + self.extensions() as u32 * 2
    }
    pub fn idle(self) -> bool {
        matches!(self.mode, 4 | 6)
    }
    pub fn tail(self) -> bool {
        self.mode == 0 && self.bits == 32
    }
    pub fn step(self) -> u32 {
        if self.register == 7 { 2 } else { 1 }
    }
    pub fn phase(self, at: usize) -> InstructionPhase {
        if at >= self.count() {
            return InstructionPhase::SequentialRefill;
        }
        if at < self.extensions() {
            if at == 0 {
                InstructionPhase::ExtensionFirst
            } else {
                InstructionPhase::ExtensionSecond
            }
        } else if self.memory() && at == self.extensions() {
            InstructionPhase::OperandReadHigh
        } else if self.memory() && at == self.count() - 1 {
            InstructionPhase::OperandWriteHigh
        } else {
            InstructionPhase::OperandRefill
        }
    }
    pub fn mask(self) -> u32 {
        if self.bits == 32 {
            u32::MAX
        } else {
            (1u32 << self.bits) - 1
        }
    }
    pub fn result(self, source: u32) -> u32 {
        if self.clear { 0 } else { !source & self.mask() }
    }
}

#[derive(Clone, Copy, Debug)]
pub(super) struct Case {
    pub word: u16,
    pub p: Profile,
    pub r: Registers,
    pub retained: u16,
    pub low: u16,
    pub operand: u16,
    pub next: u16,
    pub last: u16,
    pub start: u64,
    pub handler: u32,
    pub reset_handler: u32,
    pub reset_ssp: u32,
}

pub(super) fn address(c: Case, r: &Registers) -> u32 {
    let p = c.p;
    let base = r.address_register(p.register).unwrap();
    match p.mode {
        2 | 3 => base,
        4 => base.wrapping_sub(p.step()),
        5 => base.wrapping_add_signed(i32::from(c.retained as i16)),
        6 => {
            let reg = usize::from(c.retained >> 12 & 7);
            let raw = if c.retained & 0x8000 == 0 {
                r.d[reg]
            } else {
                r.address_register(reg).unwrap()
            };
            let index = if c.retained & 0x800 == 0 {
                i32::from(raw as i16)
            } else {
                raw as i32
            };
            base.wrapping_add_signed(index)
                .wrapping_add_signed(i32::from(c.retained as i8))
        }
        7 if p.register == 0 => i32::from(c.retained as i16) as u32,
        7 if p.register == 1 => u32::from(c.retained) << 16 | u32::from(c.low),
        _ => panic!("operand address"),
    }
}

pub(super) fn flags(sr: u16, result: u32, bits: u8) -> u16 {
    (sr & !15)
        | if result == 0 { 4 } else { 0 }
        | if result & (1u32 << (bits - 1)) != 0 {
            8
        } else {
            0
        }
}
