use super::control::Completion;
use super::moves::address::Stream;
use super::*;
use crate::constants::TRANSFER_CLOCKS;
use crate::ea::Size;

impl PrefetchCpu {
    pub(super) fn math_instruction<B: ClockedBus>(
        &mut self,
        bus: &mut B,
        pipeline: Pipeline,
    ) -> Result<Option<Completion>, TimingFault<B::Error>> {
        let opcode = pipeline.words[0];
        let family = opcode & 0xf1c0;
        if !matches!(family, 0xc0c0 | 0xc1c0 | 0x80c0 | 0x81c0 | 0x4180) {
            return Ok(None);
        }
        let mode = (opcode >> 3) & 7;
        let register = usize::from(opcode & 7);
        if !crate::instruction::data_source(mode, register as u16) {
            return Err(TimingFault::UnsupportedInstruction {
                pc: pipeline.address,
                opcode,
            });
        }
        let divide = matches!(family, 0x80c0 | 0x81c0);
        if mode == 0 && family != 0x4180 && !(divide && self.registers.d[register] as u16 == 0) {
            return Ok(None);
        }
        let mut stream = Stream::new(pipeline);
        let (source, _) = self.read_operand(bus, &mut stream, (Size::Word, mode, register))?;
        let result = crate::multiply::calculate(
            &mut self.registers,
            family,
            usize::from((opcode >> 9) & 7),
            source as u16,
        );
        if let Some(vector) = result.exception {
            self.short_exception_with_idle(bus, vector, stream.cursor, result.cycles - 30)?;
        } else {
            let early_fetch = matches!(family, 0xc0c0 | 0xc1c0);
            if !early_fetch {
                self.idle(bus, result.cycles - TRANSFER_CLOCKS)?;
            }
            let words = stream.finish(self, bus)?;
            if early_fetch {
                self.idle(bus, result.cycles - TRANSFER_CLOCKS)?;
            }
            self.registers.pc = stream.cursor;
            self.pipeline = Some(Pipeline {
                address: stream.cursor,
                words,
            });
        }
        Ok(Some(Completion {
            exception: result.exception,
            trace: true,
        }))
    }
}

#[cfg(test)]
mod failure_tests;
#[cfg(test)]
mod tests;
