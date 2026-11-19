use crate::Region;
use anyhow::{Result, ensure};
use zeff_emu_common::save_state::{StateReader, StateWriter};

const BAUD: [u32; 4] = [4800, 2400, 1200, 300];
const FRAME_BITS: u8 = 10;
pub(super) const CONTROL_MASK: u8 = 0xf8;
pub(super) const TX_FULL: u8 = 1;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct Serial {
    clock: u64,
    phase: u32,
    baud: u8,
    remaining: u8,
    shift: u8,
    holding: Option<u8>,
}

impl Serial {
    pub(super) fn inactive_empty(&self, now: u64, control: u8, region: Region) -> bool {
        if control & 0x38 != 0 || self.validate(now, control, region).is_err() {
            return false;
        }
        let mut current = self.clone();
        current.advance(now, control, region);
        current.remaining == 0 && current.holding.is_none()
    }

    pub(super) fn status(&self, now: u64, control: u8, region: Region) -> u8 {
        let mut current = self.clone();
        current.advance(now, control, region);
        u8::from(current.holding.is_some()) * TX_FULL
    }

    pub(super) fn transmit(&mut self, value: u8, now: u64, control: u8, region: Region) {
        self.advance(now, control, region);
        self.holding = Some(value);
    }

    pub(super) fn control(&mut self, before: u8, after: u8, now: u64, region: Region) {
        self.advance(now, before, region);
        if self.remaining == 0 && before >> 6 != after >> 6 {
            self.baud = after >> 6;
            self.phase = 0;
        }
    }

    fn advance(&mut self, now: u64, control: u8, region: Region) {
        let hz = region.master_hz() as u32;
        let mut elapsed = now - self.clock;
        while self.remaining != 0 || self.holding.is_some() {
            let rate = BAUD[usize::from(self.baud)];
            let until_edge = u64::from((hz - self.phase).div_ceil(rate));
            if elapsed < until_edge {
                self.phase += (elapsed * u64::from(rate)) as u32;
                self.clock = now;
                return;
            }
            elapsed -= until_edge;
            self.phase =
                (u64::from(self.phase) + until_edge * u64::from(rate) - u64::from(hz)) as u32;
            self.remaining = self.remaining.saturating_sub(1);
            if self.remaining == 0 {
                self.shift = 0;
                if self.baud != control >> 6 {
                    self.baud = control >> 6;
                    self.phase = 0;
                }
                if let Some(value) = self.holding.take() {
                    self.shift = value;
                    self.remaining = FRAME_BITS;
                }
            }
        }
        let phase = u64::from(self.phase)
            + (elapsed % u64::from(hz)) * u64::from(BAUD[usize::from(self.baud)]);
        self.phase = (phase % u64::from(hz)) as u32;
        self.clock = now;
    }

    pub(super) fn validate(&self, now: u64, control: u8, region: Region) -> Result<()> {
        ensure!(
            self.clock <= now && u64::from(self.phase) < region.master_hz() && self.baud < 4,
            "invalid MD serial clock"
        );
        ensure!(
            self.remaining <= FRAME_BITS
                && (self.remaining != 0 || (self.shift == 0 && self.baud == control >> 6)),
            "invalid MD serial transmitter"
        );
        Ok(())
    }

    pub(super) fn write_state(&self, writer: &mut StateWriter) {
        writer.write_u64(self.clock);
        writer.write_u32(self.phase);
        writer.write_u8(self.baud);
        writer.write_u8(self.remaining);
        writer.write_u8(self.shift);
        writer.write_bool(self.holding.is_some());
        if let Some(value) = self.holding {
            writer.write_u8(value);
        }
    }

    pub(super) fn read_state(reader: &mut StateReader<'_>) -> Result<Self> {
        Ok(Self {
            clock: reader.read_u64()?,
            phase: reader.read_u32()?,
            baud: reader.read_u8()?,
            remaining: reader.read_u8()?,
            shift: reader.read_u8()?,
            holding: if reader.read_bool()? {
                Some(reader.read_u8()?)
            } else {
                None
            },
        })
    }
}

#[cfg(test)]
mod tests;
