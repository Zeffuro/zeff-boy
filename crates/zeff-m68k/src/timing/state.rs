use anyhow::{Result, ensure};
use zeff_emu_common::save_state::{StateReader, StateWriter};

use super::{CpuClocks, InterruptState, Pipeline, PrefetchCpu, physical};
use crate::{Registers, STATUS_MASK};

const BOUNDARY_STATE_VERSION: u8 = 2;
const MAX_BOUNDARY_STATE_BYTES: usize = 97;

impl PrefetchCpu {
    /// Serializes ordinary instruction boundaries only.
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
        let observed = self.interrupts.observed_parts();
        writer.write_u8(if observed.is_some() {
            BOUNDARY_STATE_VERSION
        } else {
            1
        });
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
            if let Some((pending, level)) = observed {
                writer.write_bool(pending);
                writer.write_u8(level);
            }
        });
        Ok(())
    }

    pub fn read_boundary_state(&mut self, reader: &mut StateReader<'_>) -> Result<()> {
        let version = reader.read_u8()?;
        ensure!(
            (1..=BOUNDARY_STATE_VERSION).contains(&version),
            "unsupported M68000 boundary state version"
        );
        let mut reader = StateReader::new(reader.read_slice(if version == 1 {
            MAX_BOUNDARY_STATE_BYTES - 2
        } else {
            MAX_BOUNDARY_STATE_BYTES
        })?);
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
            Some(Pipeline::unknown(
                reader.read_u32()?,
                [reader.read_u16()?, reader.read_u16()?],
            ))
        } else {
            None
        };
        let stopped = reader.read_bool()?;
        let pending_trace = reader.read_bool()?;
        let level = reader.read_u8()?;
        let edge = reader.read_bool()?;
        let mut interrupts = InterruptState::from_state_parts(level, edge)
            .ok_or_else(|| anyhow::anyhow!("invalid M68000 interrupt level"))?;
        if version == 2 {
            let pending = reader.read_bool()?;
            let level = reader.read_u8()?;
            ensure!(
                interrupts.restore_observation(pending, level),
                "invalid M68000 observed interrupt"
            );
        }
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
        if let Some((pending, level)) = self.interrupts.observed_parts() {
            ensure!(
                !self.stopped
                    && !self.pending_trace
                    && self.pipeline.is_some()
                    && (!pending || level > ((self.registers.status_register() >> 8) & 7) as u8),
                "inconsistent M68000 observed interrupt boundary"
            );
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
