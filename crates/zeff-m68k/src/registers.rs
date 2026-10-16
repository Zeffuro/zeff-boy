use crate::constants::{RESET_STATUS, STATUS_MASK, SUPERVISOR};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Registers {
    pub d: [u32; 8],
    pub a: [u32; 7],
    pub usp: u32,
    pub ssp: u32,
    pub pc: u32,
    sr: u16,
}

impl Registers {
    pub const fn new() -> Self {
        Self {
            d: [0; 8],
            a: [0; 7],
            usp: 0,
            ssp: 0,
            pc: 0,
            sr: RESET_STATUS,
        }
    }

    pub const fn status_register(&self) -> u16 {
        self.sr
    }

    /// Reserved bits and later-family status fields are discarded.
    pub fn set_status_register(&mut self, value: u16) {
        self.sr = value & STATUS_MASK;
    }

    pub const fn supervisor(&self) -> bool {
        self.sr & SUPERVISOR != 0
    }

    pub const fn stack_pointer(&self) -> u32 {
        if self.supervisor() {
            self.ssp
        } else {
            self.usp
        }
    }

    pub fn address_register(&self, index: usize) -> Option<u32> {
        match index {
            0..=6 => Some(self.a[index]),
            7 => Some(self.stack_pointer()),
            _ => None,
        }
    }

    pub fn set_address_register(&mut self, index: usize, value: u32) -> bool {
        match index {
            0..=6 => self.a[index] = value,
            7 if self.supervisor() => self.ssp = value,
            7 => self.usp = value,
            _ => return false,
        }
        true
    }
}

impl Default for Registers {
    fn default() -> Self {
        Self::new()
    }
}
