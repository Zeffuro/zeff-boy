use std::io::{self, Write};

pub(crate) struct Serial {
    pub(crate) sb: u8,
    pub(crate) sc: u8,
    pub(crate) cycles: u64,
}

impl Serial {
    pub(crate) fn new() -> Self {
        Self {
            sb: 0,
            sc: 0,
            cycles: 0,
        }
    }

    pub(crate) fn step(&mut self, cycles: u64) -> bool {
        if self.sc & 0x81 != 0x81 {
            return false;
        }

        self.cycles += cycles;

        if self.cycles >= 1024 {
            self.cycles -= 1024;
            print!("{}", self.sb as char);
            let _ = io::stdout().flush();

            self.sb = 0xFF;
            self.sc &= !0x80;
            return true;
        }

        false
    }
}