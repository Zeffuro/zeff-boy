use super::*;
use crate::state::{Reader, Writer};

impl JrTail {
    pub fn write_state(&self, writer: &mut Writer) -> Result<(), CycleError> {
        self.validate(&self.cpu, &self.bus)?;
        writer.write_u8(1);
        self.cpu.write_continuation(&self.bus, writer);
        writer.write_u16(self.address);
        writer.write_u8(self.displacement);
        writer.write_u32(self.waits);
        Ok(())
    }

    pub fn read_state(reader: &mut Reader<'_>) -> anyhow::Result<Self> {
        anyhow::ensure!(reader.read_u8()? == 1, "unsupported JR tail state");
        let mut cpu = Cpu::new();
        let mut bus = BusState::default();
        cpu.read_continuation(&mut bus, reader)?;
        let tail = Self {
            cpu,
            bus,
            address: reader.read_u16()?,
            displacement: reader.read_u8()?,
            waits: reader.read_u32()?,
        };
        tail.validate(&tail.cpu, &tail.bus)
            .map_err(|e| anyhow::anyhow!("invalid JR tail: {e:?}"))?;
        Ok(tail)
    }
}
