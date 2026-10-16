use super::moves::address::Stream;
use super::*;
use crate::ea::Size;

impl PrefetchCpu {
    pub(super) fn bit_instruction<B: ClockedBus>(
        &mut self,
        bus: &mut B,
        pipeline: Pipeline,
    ) -> Result<bool, TimingFault<B::Error>> {
        let opcode = pipeline.words[0];
        let immediate = opcode & 0xff00 == 0x0800;
        if !immediate && opcode & 0xf100 != 0x0100 {
            return Ok(false);
        }
        let mode = (opcode >> 3) & 7;
        let register = usize::from(opcode & 7);
        if mode == 1 || !immediate && mode == 0 {
            return Ok(false);
        }
        let operation = (opcode >> 6) & 3;
        let valid = if operation == 0 {
            mode <= 6 || mode == 7 && register <= if immediate { 3 } else { 4 }
        } else {
            mode <= 6 || mode == 7 && register <= 1
        };
        if !valid {
            return Ok(false);
        }
        let mut stream = Stream::new(pipeline);
        let bit = if immediate {
            u32::from(stream.take(self, bus, false)?)
        } else {
            self.registers.d[usize::from((opcode >> 9) & 7)]
        };
        let size = if mode == 0 { Size::Long } else { Size::Byte };
        let (value, address) = self.read_operand(bus, &mut stream, (size, mode, register))?;
        let mask = 1 << (bit & if mode == 0 { 31 } else { 7 });
        let sr = (self.registers.status_register() & !crate::ZERO)
            | if value & mask == 0 { crate::ZERO } else { 0 };
        self.registers.set_status_register(sr);
        let result = match operation {
            1 => value ^ mask,
            2 => value & !mask,
            3 => value | mask,
            _ => value,
        };
        if mode == 0 && operation != 0 {
            self.registers.d[register] = result;
        }
        let words = stream.finish(self, bus)?;
        if mode == 0 || mode == 7 && register == 4 {
            let clocks = match operation {
                0 => 2,
                2 => 4 + u32::from(bit & 16 != 0) * 2,
                _ => 2 + u32::from(bit & 16 != 0) * 2,
            };
            self.idle(bus, clocks)?;
        } else if operation != 0 {
            self.write_data(bus, address, true, result as u16)?;
        }
        self.registers.pc = stream.cursor;
        self.pipeline = Some(Pipeline {
            address: stream.cursor,
            words,
        });
        Ok(true)
    }
}

#[cfg(test)]
mod failure_tests;
#[cfg(test)]
mod tests;
