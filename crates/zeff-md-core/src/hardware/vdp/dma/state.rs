use super::*;
use crate::hardware::vdp::state::*;
use anyhow::{Result, ensure};

impl VdpDmaCopy {
    pub(in crate::hardware::vdp) fn write_state(&self, w: &mut StateWriter) {
        w.write_u16(self.registers.length);
        w.write_u16(self.registers.source);
        w.write_u8(self.registers.source_high);
        write_command(w, self.command);
        w.write_u8(self.increment);
        w.write_u8(self.data);
    }

    pub(in crate::hardware::vdp) fn read_state(r: &mut StateReader<'_>) -> Result<Self> {
        Ok(Self {
            registers: VdpDmaRegisters {
                length: r.read_u16()?,
                source: r.read_u16()?,
                source_high: r.read_u8()?,
            },
            command: read_command(r)?,
            increment: r.read_u8()?,
            data: r.read_u8()?,
        })
    }

    pub(in crate::hardware::vdp) fn validate_state(
        &self,
        registers: VdpDmaRegisters,
        command: ControlCommand,
        increment: u8,
    ) -> Result<()> {
        ensure!(
            self.registers == registers && self.command == command && self.increment == increment,
            "stale VDP DMA prefetch snapshot"
        );
        Ok(())
    }
}
