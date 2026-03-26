use std::fmt;

// Status register flag bit positions.
pub const CARRY_FLAG: u8 = 0;
pub const ZERO_FLAG: u8 = 1;
pub const INTERRUPT_FLAG: u8 = 2;
pub const DECIMAL_FLAG: u8 = 3;
pub const BREAK_FLAG: u8 = 4;
pub const UNUSED_FLAG: u8 = 5;
pub const OVERFLOW_FLAG: u8 = 6;
pub const NEGATIVE_FLAG: u8 = 7;

#[derive(Clone, Copy)]
pub struct Registers {
    pub a: u8,
    pub x: u8,
    pub y: u8,
    pub p: u8,
}

impl Registers {
    pub fn power_on() -> Self {
        Self {
            a: 0,
            x: 0,
            y: 0,
            p: 0x24,
        }
    }

    #[inline]
    pub fn get_flag(&self, bit: u8) -> bool {
        (self.p >> bit) & 1 != 0
    }

    #[inline]
    pub fn set_flag(&mut self, bit: u8, value: bool) {
        if value {
            self.p |= 1 << bit;
        } else {
            self.p &= !(1 << bit);
        }
    }
    
    #[inline]
    pub fn set_zn(&mut self, value: u8) {
        self.set_flag(ZERO_FLAG, value == 0);
        self.set_flag(NEGATIVE_FLAG, value & 0x80 != 0);
    }
    
    #[inline]
    pub fn status_for_push(&self, brk: bool) -> u8 {
        let mut v = self.p | (1 << UNUSED_FLAG);
        if brk {
            v |= 1 << BREAK_FLAG;
        } else {
            v &= !(1 << BREAK_FLAG);
        }
        v
    }
}

impl fmt::Debug for Registers {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Registers")
            .field("A", &format_args!("{:#04X}", self.a))
            .field("X", &format_args!("{:#04X}", self.x))
            .field("Y", &format_args!("{:#04X}", self.y))
            .field(
                "P",
                &format_args!(
                    "{:#04X} [{}{}{}{}{}{}]",
                    self.p,
                    if self.get_flag(NEGATIVE_FLAG) { 'N' } else { '-' },
                    if self.get_flag(OVERFLOW_FLAG) { 'V' } else { '-' },
                    if self.get_flag(DECIMAL_FLAG) { 'D' } else { '-' },
                    if self.get_flag(INTERRUPT_FLAG) { 'I' } else { '-' },
                    if self.get_flag(ZERO_FLAG) { 'Z' } else { '-' },
                    if self.get_flag(CARRY_FLAG) { 'C' } else { '-' },
                ),
            )
            .finish()
    }
}

