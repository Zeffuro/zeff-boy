use super::moves::address::Stream;
use super::*;

#[derive(Clone, Copy)]
enum Operation {
    Lea(usize),
    Pea,
    Jump(bool),
}

impl PrefetchCpu {
    pub(super) fn control_address_instruction<B: ClockedBus>(
        &mut self,
        bus: &mut B,
        pipeline: Pipeline,
    ) -> Result<bool, TimingFault<B::Error>> {
        let opcode = pipeline.words[0];
        let mode = (opcode >> 3) & 7;
        let register = usize::from(opcode & 7);
        let operation = if opcode & 0xf1c0 == 0x41c0 {
            Operation::Lea(usize::from((opcode >> 9) & 7))
        } else if opcode & 0xffc0 == 0x4840 && mode != 0 {
            Operation::Pea
        } else if matches!(opcode & 0xffc0, 0x4e80 | 0x4ec0) {
            Operation::Jump(opcode & 0xffc0 == 0x4e80)
        } else {
            return Ok(false);
        };
        if !crate::instruction::control_address(mode, register as u16) {
            return Err(TimingFault::UnsupportedInstruction {
                pc: pipeline.address,
                opcode,
            });
        }
        if let Operation::Jump(call) = operation {
            let (address, return_pc) = self.jump_address(bus, pipeline, mode, register)?;
            let first = self.fetch(bus, address)?;
            if call {
                self.push_stack(bus, return_pc)?;
            }
            let second = self.fetch(bus, address.wrapping_add(2))?;
            self.registers.pc = address;
            self.pipeline = Some(Pipeline {
                address,
                words: [first, second],
            });
            return Ok(true);
        }
        let mut stream = Stream::new(pipeline);
        let extension = pipeline.words[1];
        let base = if mode == 7 {
            stream.cursor
        } else {
            self.registers.address_register(register).unwrap()
        };
        let address = match (mode, register) {
            (2, _) => base,
            (5, _) | (7, 2) => base.wrapping_add_signed(i32::from(extension as i16)),
            (6, _) | (7, 3) => {
                self.idle(bus, 2)?;
                let address = indexed(&self.registers, base, extension);
                stream.take(self, bus, false)?;
                address
            }
            (7, 0) => i32::from(extension as i16) as u32,
            (7, 1) => {
                let high = stream.take(self, bus, false)?;
                let low = stream.take(self, bus, matches!(operation, Operation::Lea(_)))?;
                (u32::from(high) << 16) | u32::from(low)
            }
            _ => unreachable!(),
        };
        if let Operation::Lea(destination) = operation {
            // Commit the absolute-long address only after both extension inputs.
            self.registers.set_address_register(destination, address);
        }
        if matches!((mode, register), (5, _) | (7, 0 | 2)) {
            stream.take(self, bus, false)?;
        }
        if matches!((mode, register), (6, _) | (7, 3)) {
            self.idle(bus, 2)?;
        }
        let absolute_push = matches!(operation, Operation::Pea) && mode == 7 && register < 2;
        if absolute_push {
            self.push_stack(bus, address)?;
        }
        let words = stream.finish(self, bus)?;
        if matches!(operation, Operation::Pea) && !absolute_push {
            self.push_stack(bus, address)?;
        }
        self.registers.pc = stream.cursor;
        self.pipeline = Some(Pipeline {
            address: stream.cursor,
            words,
        });
        Ok(true)
    }

    fn jump_address<B: ClockedBus>(
        &mut self,
        bus: &mut B,
        pipeline: Pipeline,
        mode: u16,
        register: usize,
    ) -> Result<(u32, u32), TimingFault<B::Error>> {
        let cursor = pipeline.address.wrapping_add(2);
        let extension = pipeline.words[1];
        let base = if mode == 7 {
            cursor
        } else {
            self.registers.address_register(register).unwrap()
        };
        let address = match (mode, register) {
            (2, _) => return Ok((base, cursor)),
            (5, _) | (7, 2) => {
                self.idle(bus, 2)?;
                base.wrapping_add_signed(i32::from(extension as i16))
            }
            (6, _) | (7, 3) => {
                self.idle(bus, 6)?;
                indexed(&self.registers, base, extension)
            }
            (7, 0) => {
                self.idle(bus, 2)?;
                i32::from(extension as i16) as u32
            }
            (7, 1) => {
                let low = self.fetch(bus, cursor.wrapping_add(2))?;
                return Ok((
                    (u32::from(extension) << 16) | u32::from(low),
                    cursor.wrapping_add(4),
                ));
            }
            _ => unreachable!(),
        };
        Ok((address, cursor.wrapping_add(2)))
    }
}

fn indexed(registers: &Registers, base: u32, extension: u16) -> u32 {
    let register = usize::from((extension >> 12) & 7);
    let value = if extension & 0x8000 == 0 {
        registers.d[register]
    } else {
        registers.address_register(register).unwrap()
    };
    let index = if extension & 0x0800 == 0 {
        i32::from(value as i16) as u32
    } else {
        value
    };
    base.wrapping_add(index)
        .wrapping_add_signed(i32::from(extension as u8 as i8))
}

#[cfg(test)]
mod failure_tests;
#[cfg(test)]
mod tests;
