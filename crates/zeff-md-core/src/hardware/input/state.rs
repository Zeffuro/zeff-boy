use super::*;

use anyhow::{Result, ensure};
use zeff_emu_common::save_state::{StateReader, StateWriter};

impl Input {
    pub(crate) fn write_state(&self, writer: &mut StateWriter, now: u64) -> Result<()> {
        self.validate_uart(now)?;
        for pad in &self.pads {
            pad.write_state(writer);
        }
        writer.write_bytes(&self.data);
        writer.write_bytes(&self.control);
        writer.write_bytes(&self.transmit);
        writer.write_bytes(&self.serial_control);
        self.serial.write_state(writer);
        crate::save_state::write_section(writer, |writer| {
            self.uart_idle.write_state(writer);
            Ok(())
        })?;
        crate::save_state::write_section(writer, |writer| {
            self.uart_tx.write_state(writer);
            Ok(())
        })
    }

    pub(crate) fn read_state(
        &mut self,
        reader: &mut StateReader<'_>,
        now: u64,
        region: Region,
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
        candidate.serial = serial::Serial::default();
        candidate.uart_idle = uart_idle::UartIdle::default();
        candidate.uart_tx = uart_tx::UartTx::default();
        {
            reader.read_exact(&mut candidate.transmit)?;
            reader.read_exact(&mut candidate.serial_control)?;
            ensure!(
                candidate
                    .serial_control
                    .iter()
                    .all(|value| { value & !serial::CONTROL_MASK == 0 }),
                "invalid MD serial control"
            );
        }
        {
            candidate.serial = serial::Serial::read_state(reader)?;
        }
        {
            crate::save_state::read_section(reader, |reader| {
                candidate.uart_idle = uart_idle::UartIdle::read_state(reader)?;
                Ok(())
            })?;
        }
        {
            crate::save_state::read_section(reader, |reader| {
                candidate.uart_tx = uart_tx::UartTx::read_state(reader)?;
                Ok(())
            })?;
        }
        candidate.validate_serial(now, region)?;
        *self = candidate;
        Ok(())
    }

    pub(crate) fn validate_serial(&self, now: u64, region: Region) -> Result<()> {
        self.validate_uart(now)?;
        self.uart_tx.validate(now, Some(region))?;
        ensure!(
            !self.uart_tx.any_used()
                || self
                    .serial
                    .inactive_empty(now, self.serial_control[2], region),
            "MD UART mixed clock owners"
        );
        self.serial
            .validate(now, self.serial_control[IO_PORT_COUNT - 1], region)
    }
}

#[cfg(test)]
mod tests;
