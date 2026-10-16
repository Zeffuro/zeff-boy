use super::moves::address::{Stream, extension_address};
use super::*;
use crate::ea::Size;

impl PrefetchCpu {
    pub(super) fn unary_instruction<B: ClockedBus>(
        &mut self,
        bus: &mut B,
        pipeline: Pipeline,
    ) -> Result<bool, TimingFault<B::Error>> {
        let opcode = pipeline.words[0];
        let family = opcode & 0xff00;
        if !matches!(family, 0x4000 | 0x4200 | 0x4400 | 0x4600 | 0x4a00) {
            return Ok(false);
        }
        let size = match (opcode >> 6) & 3 {
            0 => Size::Byte,
            1 => Size::Word,
            2 => Size::Long,
            _ => return Ok(false),
        };
        let mode = (opcode >> 3) & 7;
        let register = usize::from(opcode & 7);
        if mode < 2 || mode == 7 && register > 1 {
            return Ok(false);
        }
        let mut stream = Stream::new(pipeline);
        let base = self.registers.address_register(register).unwrap();
        let increment = if size == Size::Byte && register == 7 {
            2
        } else {
            size.bytes()
        };
        let address = if mode >= 5 {
            extension_address(self, bus, &mut stream, mode, register, false)?
        } else if mode == 4 {
            self.idle(bus, 2)?;
            let address = base.wrapping_sub(increment);
            self.registers.set_address_register(register, address);
            address
        } else {
            base
        };
        if mode == 3 && size != Size::Long {
            self.registers
                .set_address_register(register, base.wrapping_add(increment));
        }
        let function_code = crate::ea::data_space(&self.registers);
        // Cache the operand address; postincrement must not redirect the write.
        let high = self.read_data(bus, address, size == Size::Byte, function_code)?;
        let value = if size == Size::Long {
            if mode == 3 {
                self.registers
                    .set_address_register(register, base.wrapping_add(increment));
            }
            (u32::from(high) << 16)
                | u32::from(self.read_data(bus, address.wrapping_add(2), false, function_code)?)
        } else {
            u32::from(high)
        };
        if family == 0x4a00 || size == Size::Byte {
            self.reserve_transfer()?;
        }
        let staged_logic =
            matches!(family, 0x4200 | 0x4600) && matches!(size, Size::Word | Size::Long);
        let staged_negate =
            matches!(family, 0x4000 | 0x4400) && matches!(size, Size::Word | Size::Long);
        let mut full = self.registers;
        let flags = if staged_negate {
            &mut full
        } else {
            &mut self.registers
        };
        let result = match family {
            0x4000 => crate::extend::arithmetic(flags, size, 0, value, true),
            0x4400 => crate::alu::subtract(flags, size, 0, value, true),
            _ => {
                let result = match family {
                    0x4200 => 0,
                    0x4600 => size.truncate(!value),
                    _ => value,
                };
                if !staged_logic {
                    crate::alu::logic(flags, size, result);
                }
                result
            }
        };
        if staged_logic {
            self.reserve_transfer()?;
            crate::alu::logic(&mut self.registers, Size::Word, result);
        }
        if staged_negate {
            self.reserve_transfer()?;
            if family == 0x4000 {
                crate::extend::arithmetic(&mut self.registers, Size::Word, 0, value, true);
            } else {
                crate::alu::subtract(&mut self.registers, Size::Word, 0, value, true);
            }
        }
        let words = stream.finish(self, bus)?;
        if family != 0x4a00 {
            if size == Size::Long {
                if staged_logic {
                    self.reserve_transfer()?;
                    crate::alu::logic(&mut self.registers, Size::Long, result);
                }
                if staged_negate {
                    self.reserve_transfer()?;
                    self.registers.set_status_register(full.status_register());
                }
                // Every long unary RMW writes low first, including ordinary indirect EAs.
                self.write_data(bus, address.wrapping_add(2), false, result as u16)?;
                self.write_data(bus, address, false, (result >> 16) as u16)?;
            } else {
                self.write_data(bus, address, size == Size::Byte, result as u16)?;
            }
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
mod tests;
