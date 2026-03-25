use std::io::{self, Write};
use std::fmt;

use crate::hardware::types::hardware_mode::HardwareMode;
use crate::save_state::{StateReader, StateWriter, decode_hardware_mode};
use anyhow::Result;

pub(crate) struct Serial {
    pub(crate) sb: u8,
    pub(crate) sc: u8,
    pub(crate) cycles: u64,
    pub(crate) mode: HardwareMode,
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
    pub(crate) fn new() -> Self {
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

    pub(crate) fn output_bytes(&self) -> &[u8] {
        &self.output_log
    }

    pub(crate) fn step(&mut self, cycles: u64) -> bool {
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

    pub(crate) fn write_state(&self, writer: &mut StateWriter) {
        writer.write_u8(self.sb);
        writer.write_u8(self.sc);
        writer.write_u64(self.cycles);
        writer.write_u8(encode_hardware_mode(self.mode));
    }

    pub(crate) fn read_state(reader: &mut StateReader<'_>) -> Result<Self> {
        Ok(Self {
            sb: reader.read_u8()?,
            sc: reader.read_u8()?,
            cycles: reader.read_u64()?,
            mode: decode_hardware_mode(reader.read_u8()?)?,
            output_log: Vec::new(),
        })
    }
}

fn encode_hardware_mode(mode: HardwareMode) -> u8 {
    match mode {
        HardwareMode::DMG => 0,
        HardwareMode::SGB1 => 1,
        HardwareMode::SGB2 => 2,
        HardwareMode::CGBNormal => 3,
        HardwareMode::CGBDouble => 4,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transfer_period_matches_mode_and_fast_bit() {
        let mut serial = Serial::new();

        serial.mode = HardwareMode::DMG;
        serial.sc = 0x00;
        assert_eq!(serial.transfer_period(), 4096);

        serial.mode = HardwareMode::CGBNormal;
        serial.sc = 0x00;
        assert_eq!(serial.transfer_period(), 4096);
        serial.sc = 0x02;
        assert_eq!(serial.transfer_period(), 128);

        serial.mode = HardwareMode::CGBDouble;
        serial.sc = 0x00;
        assert_eq!(serial.transfer_period(), 2048);
        serial.sc = 0x02;
        assert_eq!(serial.transfer_period(), 64);
    }

    #[test]
    fn step_completes_transfer_only_after_selected_period() {
        let mut serial = Serial::new();
        serial.mode = HardwareMode::CGBNormal;
        serial.sc = 0x83; // start + internal clock + fast

        assert!(!serial.step(127));
        assert!(serial.step(1));
        assert_eq!(serial.sb, 0xFF);
        assert_eq!(serial.sc & 0x80, 0);
    }
}
