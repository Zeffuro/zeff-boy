use super::*;
use crate::state::{Reader, Writer};

impl DjnzTail {
    pub fn write_state(&self, w: &mut Writer) -> Result<(), CycleError> {
        self.validate(&self.cpu, &self.bus)?;
        w.write_u8(1);
        self.cpu.write_continuation(&self.bus, w);
        w.write_u8(self.opcode());
        w.write_u32(self.m1_waits);
        w.write_bool(self.operand.is_some());
        if let Some((value, waits)) = self.operand {
            w.write_u8(value);
            w.write_u32(waits);
        }
        Ok(())
    }
    pub fn read_state(r: &mut Reader<'_>) -> anyhow::Result<Self> {
        anyhow::ensure!(r.read_u8()? == 1, "unsupported DJNZ tail state");
        let mut cpu = Cpu::new();
        let mut bus = BusState::default();
        cpu.read_continuation(&mut bus, r)?;
        anyhow::ensure!(r.read_u8()? == 0x10, "invalid DJNZ frozen opcode");
        let tail = Self {
            cpu,
            bus,
            m1_waits: r.read_u32()?,
            operand: if r.read_bool()? {
                Some((r.read_u8()?, r.read_u32()?))
            } else {
                None
            },
        };
        tail.validate(&tail.cpu, &tail.bus)
            .map_err(|e| anyhow::anyhow!("invalid DJNZ tail: {e:?}"))?;
        Ok(tail)
    }
}
