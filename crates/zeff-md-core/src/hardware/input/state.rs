use super::*;
use crate::save_state::STATE_VERSION_WITH_SERIAL_IO;
use anyhow::{Result, ensure};
use zeff_emu_common::save_state::{StateReader, StateWriter};

impl Input {
    pub(crate) fn write_state(&self, writer: &mut StateWriter) {
        for pad in &self.pads {
            pad.write_state(writer);
        }
        writer.write_bytes(&self.data);
        writer.write_bytes(&self.control);
        writer.write_bytes(&self.transmit);
        writer.write_bytes(&self.serial_control);
    }

    pub(crate) fn read_state(
        &mut self,
        reader: &mut StateReader<'_>,
        now: u64,
        version: u32,
    ) -> Result<()> {
        let mut candidate = self.clone();
        for (pad, previous) in candidate.pads.iter_mut().zip(self.pads) {
            *pad = Pad::read_state(reader, now)?;
            ensure!(pad.kind == previous.kind, "MD controller topology mismatch");
        }
        reader.read_exact(&mut candidate.data)?;
        reader.read_exact(&mut candidate.control)?;
        candidate.transmit.fill(IO_SERIAL_TX_RESET);
        candidate.serial_control.fill(0);
        if version >= STATE_VERSION_WITH_SERIAL_IO {
            reader.read_exact(&mut candidate.transmit)?;
            reader.read_exact(&mut candidate.serial_control)?;
            ensure!(
                candidate
                    .serial_control
                    .iter()
                    .all(|value| value & !IO_SERIAL_CONTROL_MASK == 0),
                "invalid MD serial control"
            );
        }
        *self = candidate;
        Ok(())
    }
}

#[cfg(test)]
mod tests;
