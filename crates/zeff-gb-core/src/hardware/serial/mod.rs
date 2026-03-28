use std::io::{self, Write};
use std::fmt;

use crate::hardware::types::hardware_mode::HardwareMode;
use crate::save_state::{StateReader, StateWriter};
use anyhow::Result;

pub(super) struct Serial {
    sb: u8,
    sc: u8,
    cycles: u64,
    mode: HardwareMode,
    output_log: Vec<u8>,
}

impl fmt::Debug for Serial {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Serial")
            .field("sb", &format_args!("{:#04X}", self.sb))
            .field("sc", &format_args!("{:#04X}", self.sc))
            .field("cycles", &self.cycles)
            .field("mode", &self.mode)
            .field("output_log_len", &self.output_log.len())
            .finish()
    }
}

impl Serial {
    pub(super) fn new() -> Self {
        Self {
            sb: 0,
            sc: 0,
            cycles: 0,
            mode: HardwareMode::DMG,
            output_log: Vec::new(),
        }
    }

    fn transfer_period(&self) -> u64 {
        let cgb_mode = matches!(self.mode, HardwareMode::CGBNormal | HardwareMode::CGBDouble);
        let fast_serial = cgb_mode && (self.sc & 0x02) != 0;
        match (self.mode, fast_serial) {
            (HardwareMode::CGBDouble, false) => 2048,
            (HardwareMode::CGBDouble, true) => 64,
            (_, false) => 4096,
            (_, true) => 128,
        }
    }

    pub(super) fn output_bytes(&self) -> &[u8] {
        &self.output_log
    }

    pub(super) fn sb(&self) -> u8 {
        self.sb
    }

    pub(super) fn sc(&self) -> u8 {
        self.sc
    }

    pub(super) fn write_sb(&mut self, value: u8) {
        self.sb = value;
    }

    pub(super) fn write_sc(&mut self, value: u8) {
        self.sc = value;
    }

    pub(super) fn set_mode(&mut self, mode: HardwareMode) {
        self.mode = mode;
    }

    pub(super) fn reset_cycles(&mut self) {
        self.cycles = 0;
    }

    pub(super) fn step(&mut self, cycles: u64) -> bool {
        if self.sc & 0x81 != 0x81 {
            return false;
        }

        self.cycles += cycles;

        let transfer_period = self.transfer_period();
        if self.cycles >= transfer_period {
            self.cycles -= transfer_period;
            self.output_log.push(self.sb);
            print!("{}", self.sb as char);
            let _ = io::stdout().flush();

            self.sb = 0xFF;
            self.sc &= !0x80;
            return true;
        }

        false
    }

    pub(super) fn write_state(&self, writer: &mut StateWriter) {
        writer.write_u8(self.sb);
        writer.write_u8(self.sc);
        writer.write_u64(self.cycles);
        writer.write_hardware_mode(self.mode);
    }

    pub(super) fn read_state(reader: &mut StateReader<'_>) -> Result<Self> {
        Ok(Self {
            sb: reader.read_u8()?,
            sc: reader.read_u8()?,
            cycles: reader.read_u64()?,
            mode: reader.read_hardware_mode()?,
            output_log: Vec::new(),
        })
    }
}

#[cfg(test)]
mod tests;
