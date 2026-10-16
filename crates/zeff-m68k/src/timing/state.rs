use anyhow::{Result, ensure};
use zeff_emu_common::save_state::{StateReader, StateWriter};

use super::{CpuClocks, InterruptState, Pipeline, PrefetchCpu, physical};
use crate::{Registers, STATUS_MASK};

const BOUNDARY_STATE_VERSION: u8 = 1;
const MAX_BOUNDARY_STATE_BYTES: usize = 95;

impl PrefetchCpu {
    /// Ordinary boundaries only; physical/imported and host-frozen states reject.
    pub fn write_boundary_state(&self, writer: &mut StateWriter) -> Result<()> {
        ensure!(
            !self.frozen
                && self.physical_state == physical::PhysicalEntryState::Idle
                && self.physical_inspection
                    == physical::PhysicalInspection::DiagnosticCompleteValues
                && self.imported_boundary.is_none(),
            "M68000 state requires an ordinary execution boundary"
        );
        self.validate_boundary_state()?;
        writer.write_u8(BOUNDARY_STATE_VERSION);
        writer.write_section(|writer| {
            for value in self.registers.d.into_iter().chain(self.registers.a) {
                writer.write_u32(value);
            }
            writer.write_u32(self.registers.usp);
            writer.write_u32(self.registers.ssp);
            writer.write_u32(self.registers.pc);
            writer.write_u16(self.registers.status_register());
            writer.write_u64(self.clock.0);
            writer.write_bool(self.pipeline.is_some());
            if let Some(pipeline) = self.pipeline {
                writer.write_u32(pipeline.address);
                for word in pipeline.words {
                    writer.write_u16(word);
                }
            }
            writer.write_bool(self.stopped);
            writer.write_bool(self.pending_trace);
            let (level, edge) = self.interrupts.state_parts();
            writer.write_u8(level);
            writer.write_bool(edge);
        });
        Ok(())
    }

    pub fn read_boundary_state(&mut self, reader: &mut StateReader<'_>) -> Result<()> {
        ensure!(
            reader.read_u8()? == BOUNDARY_STATE_VERSION,
            "unsupported M68000 boundary state version"
        );
        let mut reader = StateReader::new(reader.read_slice(MAX_BOUNDARY_STATE_BYTES)?);
        let mut registers = Registers::new();
        for value in registers.d.iter_mut().chain(&mut registers.a) {
            *value = reader.read_u32()?;
        }
        registers.usp = reader.read_u32()?;
        registers.ssp = reader.read_u32()?;
        registers.pc = reader.read_u32()?;
        let sr = reader.read_u16()?;
        ensure!(sr & !STATUS_MASK == 0, "invalid M68000 status register");
        registers.set_status_register(sr);
        let clock = CpuClocks(reader.read_u64()?);
        let pipeline = if reader.read_bool()? {
            Some(Pipeline {
                address: reader.read_u32()?,
                words: [reader.read_u16()?, reader.read_u16()?],
            })
        } else {
            None
        };
        let stopped = reader.read_bool()?;
        let pending_trace = reader.read_bool()?;
        let level = reader.read_u8()?;
        let edge = reader.read_bool()?;
        let interrupts = InterruptState::from_state_parts(level, edge)
            .ok_or_else(|| anyhow::anyhow!("invalid M68000 interrupt level"))?;
        ensure!(reader.is_exhausted(), "trailing M68000 boundary state data");
        let candidate = Self {
            registers,
            clock,
            pipeline,
            stopped,
            pending_trace,
            interrupts,
            ..Self::default()
        };
        candidate.validate_boundary_state()?;
        *self = candidate;
        Ok(())
    }

    fn validate_boundary_state(&self) -> Result<()> {
        ensure!(self.registers.pc & 1 == 0, "unaligned M68000 boundary PC");
        ensure!(
            self.pipeline
                .is_none_or(|pipeline| !self.stopped && pipeline.address == self.registers.pc),
            "inconsistent M68000 boundary prefetch"
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests;
