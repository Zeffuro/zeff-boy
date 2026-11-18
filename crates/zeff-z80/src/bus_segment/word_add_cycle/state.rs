use super::*;
use crate::state::{Reader, Writer};

impl WordAddTail {
    pub fn write_state(&self, w: &mut Writer) -> Result<(), CycleError> {
        self.validate(&self.cpu, &self.bus)?;
        w.write_u8(1);
        self.cpu.write_continuation(&self.bus, w);
        w.write_u8(self.opcode);
        w.write_u32(self.waits);
        w.write_u8(match self.phase {
            WordAddPhase::Internal4 => 0,
            WordAddPhase::Internal3 => 1,
        });
        Ok(())
    }
    pub fn read_state(r: &mut Reader<'_>) -> anyhow::Result<Self> {
        anyhow::ensure!(r.read_u8()? == 1, "unsupported ADD16 tail state");
        let mut cpu = Cpu::new();
        let mut bus = BusState::default();
        cpu.read_continuation(&mut bus, r)?;
        let tail = Self {
            cpu,
            bus,
            opcode: r.read_u8()?,
            waits: r.read_u32()?,
            phase: match r.read_u8()? {
                0 => WordAddPhase::Internal4,
                1 => WordAddPhase::Internal3,
                _ => anyhow::bail!("invalid ADD16 internal phase"),
            },
        };
        tail.validate(&tail.cpu, &tail.bus)
            .map_err(|e| anyhow::anyhow!("invalid ADD16 tail: {e:?}"))?;
        Ok(tail)
    }
}
