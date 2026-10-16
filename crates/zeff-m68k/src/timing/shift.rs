use super::moves::address::Stream;
use super::*;
use crate::ea::Size;

impl PrefetchCpu {
    pub(super) fn memory_shift_instruction<B: ClockedBus>(
        &mut self,
        bus: &mut B,
        pipeline: Pipeline,
    ) -> Result<bool, TimingFault<B::Error>> {
        let opcode = pipeline.words[0];
        if opcode & 0xf8c0 != 0xe0c0 {
            return Ok(false);
        }
        let mode = (opcode >> 3) & 7;
        let register = usize::from(opcode & 7);
        if mode < 2 || mode == 7 && register > 1 {
            return Ok(false);
        }
        let mut stream = Stream::new(pipeline);
        let (value, address) = self.read_operand(bus, &mut stream, (Size::Word, mode, register))?;
        let result = crate::shift::shift(
            &mut self.registers,
            Size::Word,
            value,
            1,
            (opcode >> 9) & 3,
            opcode & 0x0100 != 0,
        );
        let words = stream.finish(self, bus)?;
        self.write_data(bus, address, false, result as u16)?;
        self.registers.pc = stream.cursor;
        self.pipeline = Some(Pipeline {
            address: stream.cursor,
            words,
        });
        Ok(true)
    }
}

#[cfg(test)]
mod tests;
