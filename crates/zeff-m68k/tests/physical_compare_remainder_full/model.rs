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
    pub cmpa: bool,
    pub bits: u8,
    pub mode: u8,
    pub register: usize,
    pub destination: usize,
}
impl Profile {
    pub fn decode(word: u16) -> Option<Self> {
        if word >> 12 != 11 {
            return None;
        }
        let op = word >> 6 & 7;
        let cmpa = matches!(op, 3 | 7);
        let bits = match op {
            0 => 8,
            1 | 3 => 16,
            2 | 7 => 32,
            _ => return None,
        };
        let mode = (word >> 3 & 7) as u8;
        let register = usize::from(word & 7);
        let admitted = match mode {
            0 => true,
            1 => bits != 8,
            2..=6 => bits == 8 && !cmpa,
            7 => register == 4 || bits == 8 && !cmpa && register <= 3,
            _ => false,
        };
        admitted.then_some(Self {
            cmpa,
            bits,
            mode,
            register,
            destination: usize::from(word >> 9 & 7),
        })
    }
    pub fn memory(self) -> bool {
        matches!(self.mode, 2..=6) || self.mode == 7 && self.register <= 3
    }
    pub fn immediate(self) -> bool {
        self.mode == 7 && self.register == 4
    }
    pub fn extensions(self) -> usize {
        if self.immediate() {
            return if self.bits == 32 { 2 } else { 1 };
        }
        if self.mode == 7 && self.register == 1 {
            return 2;
        }
        usize::from(matches!(self.mode, 5..=7))
    }
    pub fn count(self) -> usize {
        self.extensions() + usize::from(self.memory()) + 1
    }
    pub fn length(self) -> u32 {
        2 + self.extensions() as u32 * 2
    }
    pub fn idle(self) -> bool {
        self.memory() && (matches!(self.mode, 4 | 6) || self.mode == 7 && self.register == 3)
    }
    pub fn tail(self) -> bool {
        self.cmpa || self.bits == 32
    }
    pub fn step(self) -> u32 {
        if self.register == 7 { 2 } else { 1 }
    }
    pub fn index(self) -> bool {
        self.memory() && (self.mode == 6 || self.mode == 7 && self.register == 3)
    }
    pub fn displacement(self) -> bool {
        self.memory() && (self.mode == 5 || self.mode == 7 && self.register == 2)
    }
    pub fn phase(self, at: usize) -> InstructionPhase {
        if at < self.extensions() {
            match (self.immediate(), at) {
                (true, 0) => InstructionPhase::ImmediateFirst,
                (true, _) => InstructionPhase::ImmediateSecond,
                (false, 0) => InstructionPhase::ExtensionFirst,
                (false, _) => InstructionPhase::ExtensionSecond,
            }
        } else if self.memory() && at == self.extensions() {
            InstructionPhase::OperandReadHigh
        } else {
            InstructionPhase::OperandRefill
        }
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
    pub lane: u32,
    pub next: u16,
    pub last: u16,
    pub start: u64,
}
pub(super) fn address(c: Case, r: &Registers) -> u32 {
    let p = c.p;
    let base = if p.mode == 7 && matches!(p.register, 2 | 3) {
        r.pc.wrapping_add(2)
    } else {
        r.address_register(p.register).unwrap()
    };
    match p.mode {
        2 | 3 => base,
        4 => base.wrapping_sub(p.step()),
        5 => base.wrapping_add_signed(i32::from(c.retained as i16)),
        6 => indexed(base, c.retained, r),
        7 => match p.register {
            0 => i32::from(c.retained as i16) as u32,
            1 => u32::from(c.retained) << 16 | u32::from(c.low),
            2 => base.wrapping_add_signed(i32::from(c.retained as i16)),
            3 => indexed(base, c.retained, r),
            _ => panic!("source address"),
        },
        _ => panic!("register source address"),
    }
}
fn indexed(base: u32, extension: u16, r: &Registers) -> u32 {
    let register = usize::from(extension >> 12 & 7);
    let raw = if extension & 0x8000 != 0 {
        r.address_register(register).unwrap()
    } else {
        r.d[register]
    };
    let index = if extension & 0x800 != 0 {
        raw as i32
    } else {
        i32::from(raw as i16)
    };
    base.wrapping_add_signed(index)
        .wrapping_add_signed(i32::from(extension as i8))
}
pub(super) fn flags(sr: u16, source: u32, destination: u32, bits: u8) -> u16 {
    let mask = if bits == 32 {
        u32::MAX
    } else {
        (1u32 << bits) - 1
    };
    let sign = 1u32 << (bits - 1);
    let source = source & mask;
    let destination = destination & mask;
    let result = destination.wrapping_sub(source) & mask;
    (sr & !15)
        | u16::from(source > destination)
        | if (source ^ destination) & (destination ^ result) & sign != 0 {
            2
        } else {
            0
        }
        | if result == 0 { 4 } else { 0 }
        | if result & sign != 0 { 8 } else { 0 }
}
