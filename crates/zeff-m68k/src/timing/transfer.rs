use super::moves::address::{Stream, extension_address};
use super::*;
use crate::ea::Size;

fn register(registers: &Registers, index: usize) -> u32 {
    if index < 8 {
        registers.d[index]
    } else {
        registers.address_register(index - 8).unwrap()
    }
}

fn set_register(registers: &mut Registers, index: usize, value: u32) {
    if index < 8 {
        registers.d[index] = value;
    } else {
        registers.set_address_register(index - 8, value);
    }
}

impl PrefetchCpu {
    pub(super) fn transfer_instruction<B: ClockedBus>(
        &mut self,
        bus: &mut B,
        pipeline: Pipeline,
    ) -> Result<bool, TimingFault<B::Error>> {
        let opcode = pipeline.words[0];
        if opcode & 0xf138 == 0x0108 {
            self.movep(bus, pipeline)?;
            return Ok(true);
        }
        if opcode & 0xfb80 != 0x4880 || (opcode >> 3) & 7 == 0 {
            return Ok(false);
        }
        let mode = (opcode >> 3) & 7;
        let base_register = usize::from(opcode & 7);
        let load = opcode & 0x0400 != 0;
        let control = crate::instruction::control_address(mode, base_register as u16);
        let valid = if load {
            control || mode == 3
        } else {
            control && !(mode == 7 && base_register >= 2) || mode == 4
        };
        if !valid {
            return Err(TimingFault::UnsupportedInstruction {
                pc: pipeline.address,
                opcode,
            });
        }
        let size = if opcode & 0x0040 == 0 {
            Size::Word
        } else {
            Size::Long
        };
        let original = self.registers;
        let fc = if mode == 7 && matches!(base_register, 2 | 3) {
            crate::ea::program_space(&original)
        } else {
            crate::ea::data_space(&original)
        };
        let mut stream = Stream::new(pipeline);
        let mask = stream.take(self, bus, false)?;
        let mut address = if mode >= 5 {
            extension_address(self, bus, &mut stream, mode, base_register, false)?
        } else {
            original.address_register(base_register).unwrap()
        };
        for bit in 0..16 {
            if mask & (1 << bit) == 0 {
                continue;
            }
            let index = if mode == 4 { 15 - bit } else { bit };
            if mode == 4 {
                address = address.wrapping_sub(size.bytes());
            }
            if load {
                let high = self.read_data(bus, address, false, fc)?;
                let value = if size == Size::Long {
                    (u32::from(high) << 16)
                        | u32::from(self.read_data(bus, address.wrapping_add(2), false, fc)?)
                } else {
                    Size::Word.sign_extend(u32::from(high))
                };
                set_register(&mut self.registers, index, value);
            } else {
                let value = register(&original, index);
                if size == Size::Long && mode == 4 {
                    self.write_data(bus, address.wrapping_add(2), false, value as u16)?;
                    self.write_data(bus, address, false, (value >> 16) as u16)?;
                } else {
                    self.write_data(
                        bus,
                        address,
                        false,
                        if size == Size::Long {
                            (value >> 16) as u16
                        } else {
                            value as u16
                        },
                    )?;
                    if size == Size::Long {
                        self.write_data(bus, address.wrapping_add(2), false, value as u16)?;
                    }
                }
            }
            if mode != 4 {
                address = address.wrapping_add(size.bytes());
            }
        }
        if load {
            self.read_word(bus, address, fc, AccessKind::Discard)?;
        }
        if matches!(mode, 3 | 4) {
            self.registers.set_address_register(base_register, address);
        }
        let words = stream.finish(self, bus)?;
        self.registers.pc = stream.cursor;
        self.pipeline = Some(Pipeline {
            address: stream.cursor,
            words,
        });
        Ok(true)
    }

    fn movep<B: ClockedBus>(
        &mut self,
        bus: &mut B,
        pipeline: Pipeline,
    ) -> Result<(), TimingFault<B::Error>> {
        let opcode = pipeline.words[0];
        let destination = usize::from((opcode >> 9) & 7);
        let long = opcode & 0x0040 != 0;
        let store = opcode & 0x0080 != 0;
        let base = self
            .registers
            .address_register(usize::from(opcode & 7))
            .unwrap();
        let source = self.registers.d[destination];
        let fc = crate::ea::data_space(&self.registers);
        let mut stream = Stream::new(pipeline);
        let displacement = stream.take(self, bus, false)? as i16;
        let address = base.wrapping_add_signed(i32::from(displacement));
        let bytes = if long { 4 } else { 2 };
        let mut value = 0;
        for position in 0..bytes {
            let address = address.wrapping_add(position * 2);
            if store {
                let byte = (source >> ((bytes - position - 1) * 8)) as u8;
                self.write_data(bus, address, true, u16::from(byte))?;
            } else {
                value = value << 8 | u32::from(self.read_data(bus, address, true, fc)?);
            }
        }
        if !store {
            self.registers.d[destination] = if long {
                value
            } else {
                (source & 0xffff_0000) | value
            };
        }
        let words = stream.finish(self, bus)?;
        self.registers.pc = stream.cursor;
        self.pipeline = Some(Pipeline {
            address: stream.cursor,
            words,
        });
        Ok(())
    }
}

#[cfg(test)]
mod failure_tests;
#[cfg(test)]
mod tests;
