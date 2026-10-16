use super::*;
use crate::hardware::vdp::state::*;
use anyhow::{Result, ensure};

impl VdpRegisterPipeline {
    pub(in crate::hardware::vdp) fn write_state(&self, w: &mut StateWriter) {
        w.write_u64(self.epoch.get());
        w.write_u64(self.now.get());
        write_optional(w, self.pending, |w, pending| {
            w.write_u8(pending.index);
            w.write_u8(pending.value);
            w.write_u64(pending.publish_at.get());
        });
        for value in self.registers {
            write_optional(w, value, StateWriter::write_u8);
        }
    }

    pub(in crate::hardware::vdp) fn read_state(r: &mut StateReader<'_>) -> Result<Self> {
        let epoch = MasterTicks::new(r.read_u64()?);
        let now = MasterTicks::new(r.read_u64()?);
        let pending = read_optional(r, |r| {
            Ok(VdpRegisterWrite {
                index: r.read_u8()?,
                value: r.read_u8()?,
                publish_at: MasterTicks::new(r.read_u64()?),
            })
        })?;
        let mut registers = [None; REGISTER_COUNT as usize];
        for value in &mut registers {
            *value = read_optional(r, StateReader::read_u8)?;
        }
        let result = Self {
            epoch,
            now,
            pending,
            registers,
        };
        result.validate_state()?;
        Ok(result)
    }

    pub(in crate::hardware::vdp) fn validate_state(&self) -> Result<()> {
        ensure!(
            self.epoch <= self.now,
            "VDP register epoch is in the future"
        );
        if let Some(write) = self.pending {
            ensure!(write.index < REGISTER_COUNT, "invalid pending VDP register");
            let phase = (self.now.get() - self.epoch.get()) % H32_INTERNAL_CLOCK_TICKS;
            ensure!(
                self.now.get().checked_add(H32_INTERNAL_CLOCK_TICKS - phase)
                    == Some(write.publish_at.get()),
                "invalid VDP register publication deadline"
            );
        }
        Ok(())
    }
}
