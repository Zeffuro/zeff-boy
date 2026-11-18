use crate::hardware::vdp::state::{StateReader, StateWriter};
use anyhow::{Result, ensure};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct Counter {
    vertical: u8,
    horizontal: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Drive {
    pub mask: u16,
    pub word: u16,
}

impl Counter {
    pub fn evaluate(
        &mut self,
        registers: [u8; 24],
        horizontal: u16,
        vertical: u16,
        interlace_latch: [bool; 2],
        light_pen: bool,
    ) -> Result<()> {
        ensure!(
            registers[12] & 6 == 0 && interlace_latch == [false; 2] && !light_pen,
            "unsupported VDP counter capture profile"
        );
        ensure!(
            horizontal <= 511 && vertical <= 511,
            "invalid VDP counter input"
        );
        // Capture precedes the current primitive's horizontal/vertical clocking.
        let m5 = registers[1] & 4 != 0;
        let horizontal_capture = m5 && registers[0] & 2 == 0;
        if !m5 || horizontal_capture {
            self.vertical = vertical as u8;
        }
        if horizontal_capture {
            self.horizontal = (horizontal >> 1) as u8;
        }
        Ok(())
    }

    pub fn drive(self, upper_read: bool, lower_read: bool) -> Drive {
        let mask = if upper_read { 0xff00 } else { 0 } | if lower_read { 0x00ff } else { 0 };
        Drive {
            mask,
            word: u16::from_be_bytes([self.vertical, self.horizontal]) & mask,
        }
    }

    pub fn write_state(self, writer: &mut StateWriter) {
        writer.write_u8(1);
        writer.write_u8(self.vertical);
        writer.write_u8(self.horizontal);
    }

    pub fn read_state(reader: &mut StateReader<'_>) -> Result<Self> {
        ensure!(
            reader.read_u8()? == 1,
            "unsupported VDP counter capture state"
        );
        Ok(Self {
            vertical: reader.read_u8()?,
            horizontal: reader.read_u8()?,
        })
    }
}
