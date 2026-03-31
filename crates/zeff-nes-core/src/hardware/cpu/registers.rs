use std::fmt;

bitflags::bitflags! {
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct StatusFlags: u8 {
        const CARRY     = 1 << 0;
        const ZERO      = 1 << 1;
        const INTERRUPT = 1 << 2;
        const DECIMAL   = 1 << 3;
        const BREAK     = 1 << 4;
        const UNUSED    = 1 << 5;
        const OVERFLOW  = 1 << 6;
        const NEGATIVE  = 1 << 7;
    }
}

#[derive(Clone, Copy)]
pub struct Registers {
    pub a: u8,
    pub x: u8,
    pub y: u8,
    pub p: StatusFlags,
}

impl Registers {
    pub fn power_on() -> Self {
        Self {
            a: 0,
            x: 0,
            y: 0,
            p: StatusFlags::from_bits_truncate(0x24),
        }
    }

    #[inline]
    pub fn get_flag(&self, flag: StatusFlags) -> bool {
        self.p.contains(flag)
    }

    #[inline]
    pub fn set_flag(&mut self, flag: StatusFlags, value: bool) {
        self.p.set(flag, value);
    }

    #[inline]
    pub fn set_zn(&mut self, value: u8) {
        self.set_flag(StatusFlags::ZERO, value == 0);
        self.set_flag(StatusFlags::NEGATIVE, value & 0x80 != 0);
    }

    #[inline]
    pub fn status_for_push(&self, brk: bool) -> u8 {
        let mut v = self.p | StatusFlags::UNUSED;
        v.set(StatusFlags::BREAK, brk);
        v.bits()
    }
}

impl fmt::Display for Registers {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "A:{:02X} X:{:02X} Y:{:02X} P:{:02X} [{}{}{}{}{}{}]",
            self.a,
            self.x,
            self.y,
            self.p.bits(),
            if self.get_flag(StatusFlags::NEGATIVE) {
                'N'
            } else {
                '-'
            },
            if self.get_flag(StatusFlags::OVERFLOW) {
                'V'
            } else {
                '-'
            },
            if self.get_flag(StatusFlags::DECIMAL) {
                'D'
            } else {
                '-'
            },
            if self.get_flag(StatusFlags::INTERRUPT) {
                'I'
            } else {
                '-'
            },
            if self.get_flag(StatusFlags::ZERO) {
                'Z'
            } else {
                '-'
            },
            if self.get_flag(StatusFlags::CARRY) {
                'C'
            } else {
                '-'
            },
        )
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
                    self.p.bits(),
                    if self.get_flag(StatusFlags::NEGATIVE) {
                        'N'
                    } else {
                        '-'
                    },
                    if self.get_flag(StatusFlags::OVERFLOW) {
                        'V'
                    } else {
                        '-'
                    },
                    if self.get_flag(StatusFlags::DECIMAL) {
                        'D'
                    } else {
                        '-'
                    },
                    if self.get_flag(StatusFlags::INTERRUPT) {
                        'I'
                    } else {
                        '-'
                    },
                    if self.get_flag(StatusFlags::ZERO) {
                        'Z'
                    } else {
                        '-'
                    },
                    if self.get_flag(StatusFlags::CARRY) {
                        'C'
                    } else {
                        '-'
                    },
                ),
            )
            .finish()
    }
}
