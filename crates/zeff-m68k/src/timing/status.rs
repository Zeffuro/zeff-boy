use super::control::Completion;
use super::moves::address::Stream;
use super::*;
use crate::constants::{CCR_MASK, VECTOR_PRIVILEGE_VIOLATION};
use crate::ea::Size;

impl PrefetchCpu {
    pub(super) fn status_instruction<B: ClockedBus>(
        &mut self,
        bus: &mut B,
        pipeline: Pipeline,
    ) -> Result<Option<Completion>, TimingFault<B::Error>> {
        let opcode = pipeline.words[0];
        let family = opcode & 0xffc0;
        if !matches!(family, 0x40c0 | 0x44c0 | 0x46c0) {
            return Ok(None);
        }
        let mode = (opcode >> 3) & 7;
        let register = usize::from(opcode & 7);
        let from_sr = family == 0x40c0;
        let to_sr = family == 0x46c0;
        let legal = if from_sr {
            crate::instruction::alterable(mode, register as u16)
        } else {
            crate::instruction::data_source(mode, register as u16)
        };
        if !legal {
            return Err(TimingFault::UnsupportedInstruction {
                pc: pipeline.address,
                opcode,
            });
        }
        if to_sr && !self.registers.supervisor() {
            self.short_exception(bus, VECTOR_PRIVILEGE_VIOLATION, pipeline.address)?;
            return Ok(Some(Completion {
                exception: Some(VECTOR_PRIVILEGE_VIOLATION),
                trace: false,
            }));
        }
        let sr = self.registers.status_register();
        let mut stream = Stream::new(pipeline);
        if from_sr {
            if mode == 0 {
                self.registers.d[register] =
                    (self.registers.d[register] & 0xffff_0000) | u32::from(sr);
                let words = stream.finish(self, bus)?;
                self.idle(bus, 2)?;
                self.registers.pc = stream.cursor;
                self.pipeline = Some(Pipeline {
                    address: stream.cursor,
                    words,
                });
            } else {
                let (_, address) =
                    self.read_operand(bus, &mut stream, (Size::Word, mode, register))?;
                let words = stream.finish(self, bus)?;
                self.write_data(bus, address, false, sr)?;
                self.registers.pc = stream.cursor;
                self.pipeline = Some(Pipeline {
                    address: stream.cursor,
                    words,
                });
            }
        } else {
            let (value, _) = self.read_operand(bus, &mut stream, (Size::Word, mode, register))?;
            self.idle(bus, 2)?;
            self.registers.set_status_register(if to_sr {
                value as u16
            } else {
                (sr & !CCR_MASK) | (value as u16 & CCR_MASK)
            });
            self.idle(bus, 2)?;
            self.control_refill(bus, stream.cursor)?;
        }
        Ok(Some(Completion {
            exception: None,
            trace: true,
        }))
    }
}

#[cfg(test)]
mod failure_tests;
#[cfg(test)]
mod tests;
