use super::*;
use crate::hardware::vdp::state::*;
use anyhow::{Result, ensure};

impl VdpWriteFifo {
    pub(in crate::hardware::vdp) fn write_state(&self, w: &mut StateWriter) {
        for entry in self.slots {
            write_optional(w, entry, |w, entry| {
                write_command(w, entry.command);
                w.write_u16(entry.data);
            });
        }
        w.write_u8(self.head as u8);
        w.write_u8(self.count as u8);
        w.write_bool(self.second_byte);
    }

    pub(in crate::hardware::vdp) fn read_state(r: &mut StateReader<'_>) -> Result<Self> {
        let mut slots = [None; FIFO_CAPACITY];
        for entry in &mut slots {
            *entry = read_optional(r, |r| {
                Ok(VdpWrite {
                    command: read_command(r)?,
                    data: r.read_u16()?,
                })
            })?;
        }
        let result = Self {
            slots,
            head: usize::from(r.read_u8()?),
            count: usize::from(r.read_u8()?),
            second_byte: r.read_bool()?,
        };
        result.validate_state()?;
        Ok(result)
    }

    pub(in crate::hardware::vdp) fn validate_state(&self) -> Result<()> {
        ensure!(
            self.head < FIFO_CAPACITY && self.count <= FIFO_CAPACITY,
            "invalid VDP FIFO cursor"
        );
        for entry in self.slots.iter().flatten() {
            validate_command(entry.command)?;
        }
        for offset in 0..self.count {
            let entry = self.slots[(self.head + offset) % FIFO_CAPACITY]
                .ok_or_else(|| anyhow::anyhow!("missing active VDP FIFO slot"))?;
            ensure!(
                matches!(
                    entry.command.code & !COMMAND_CD4,
                    VRAM_WRITE_CODE | CRAM_WRITE_CODE | VSRAM_WRITE_CODE
                ) || entry.command.is_unused_write_target(),
                "invalid active VDP FIFO command"
            );
        }
        ensure!(
            !self.second_byte
                || self
                    .front()
                    .is_some_and(|entry| entry.command.code & !COMMAND_CD4 == VRAM_WRITE_CODE),
            "invalid VDP FIFO byte phase"
        );
        Ok(())
    }
}
