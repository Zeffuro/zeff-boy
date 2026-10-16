use super::*;
use crate::ea::Size;

pub(super) mod address;
use address::{Stream, extension_address};

fn step(size: Size, register: usize) -> u32 {
    if size == Size::Byte && register == 7 {
        2
    } else {
        size.bytes()
    }
}

impl PrefetchCpu {
    pub(super) fn move_instruction<B: ClockedBus>(
        &mut self,
        bus: &mut B,
        pipeline: Pipeline,
    ) -> Result<bool, TimingFault<B::Error>> {
        let opcode = pipeline.words[0];
        let size = match opcode >> 12 {
            1 => Size::Byte,
            2 => Size::Long,
            3 => Size::Word,
            _ => return Ok(false),
        };
        let source_mode = (opcode >> 3) & 7;
        let destination_mode = (opcode >> 6) & 7;
        let source = usize::from(opcode & 7);
        let destination = usize::from((opcode >> 9) & 7);
        if source_mode == 7 && source > 4
            || destination_mode == 7 && destination > 1
            || size == Size::Byte && (source_mode == 1 || destination_mode == 1)
        {
            return Ok(false);
        }
        let mut stream = Stream::new(pipeline);
        let value = self.move_source(bus, &mut stream, source_mode, source, size)?;
        let memory_source = source_mode >= 2 && !(source_mode == 7 && source == 4);
        // Resolve the destination after source address updates, including aliased An.
        let words = if destination_mode == 4 {
            let words = stream.finish(self, bus)?;
            self.move_destination(
                bus,
                &mut stream,
                (destination_mode, destination),
                (size, value),
                memory_source,
            )?;
            words
        } else {
            self.move_destination(
                bus,
                &mut stream,
                (destination_mode, destination),
                (size, value),
                memory_source,
            )?;
            stream.finish(self, bus)?
        };
        if destination_mode != 1 {
            crate::alu::logic(&mut self.registers, size, value);
        }
        self.registers.pc = stream.cursor;
        self.pipeline = Some(Pipeline {
            address: self.registers.pc,
            words,
        });
        Ok(true)
    }

    fn move_source<B: ClockedBus>(
        &mut self,
        bus: &mut B,
        stream: &mut Stream,
        mode: u16,
        register: usize,
        size: Size,
    ) -> Result<u32, TimingFault<B::Error>> {
        if mode == 0 {
            return Ok(size.truncate(self.registers.d[register]));
        }
        if mode == 7 && register == 4 {
            let first = stream.take(self, bus, false)?;
            return Ok(if size == Size::Long {
                (u32::from(first) << 16) | u32::from(stream.take(self, bus, false)?)
            } else {
                size.truncate(u32::from(first))
            });
        }
        let base = self.registers.address_register(register).unwrap();
        if mode == 1 {
            return Ok(size.truncate(base));
        }
        let address = if mode >= 5 {
            extension_address(self, bus, stream, mode, register, false)?
        } else if mode == 4 {
            self.idle(bus, 2)?;
            let address = base.wrapping_sub(step(size, register));
            self.registers.set_address_register(register, address);
            address
        } else {
            base
        };
        if mode == 3 && size != Size::Long {
            self.registers
                .set_address_register(register, base.wrapping_add(step(size, register)));
        }
        let fc = if mode == 7 && matches!(register, 2 | 3) {
            crate::ea::program_space(&self.registers)
        } else {
            crate::ea::data_space(&self.registers)
        };
        let first = self.read_data(bus, address, size == Size::Byte, fc)?;
        if size != Size::Long {
            return Ok(u32::from(first));
        }
        if mode == 3 {
            self.registers
                .set_address_register(register, base.wrapping_add(4));
        }
        let low = self.read_data(bus, address.wrapping_add(2), false, fc)?;
        Ok((u32::from(first) << 16) | u32::from(low))
    }

    fn move_destination<B: ClockedBus>(
        &mut self,
        bus: &mut B,
        stream: &mut Stream,
        destination: (u16, usize),
        operand: (Size, u32),
        memory_source: bool,
    ) -> Result<(), TimingFault<B::Error>> {
        let (mode, register) = destination;
        let (size, value) = operand;
        if mode == 0 {
            self.registers.d[register] =
                (self.registers.d[register] & !size.mask()) | size.truncate(value);
            return Ok(());
        }
        if mode == 1 {
            self.registers
                .set_address_register(register, size.sign_extend(value));
            return Ok(());
        }
        let base = self.registers.address_register(register).unwrap();
        let address = if mode >= 5 {
            // Memory-source absolute-long MOVE writes before refilling the next opcode.
            extension_address(self, bus, stream, mode, register, memory_source)?
        } else if mode == 4 {
            base.wrapping_sub(step(size, register))
        } else {
            base
        };
        if mode == 4 && size == Size::Long {
            self.write_data(bus, base.wrapping_sub(2), false, value as u16)?;
            self.registers.set_address_register(register, address);
            self.write_data(bus, address, false, (value >> 16) as u16)?;
        } else {
            if mode == 4 {
                self.registers.set_address_register(register, address);
            }
            let first = if size == Size::Long {
                (value >> 16) as u16
            } else {
                value as u16
            };
            self.write_data(bus, address, size == Size::Byte, first)?;
            if size == Size::Long {
                self.write_data(bus, address.wrapping_add(2), false, value as u16)?;
            }
        }
        if mode == 3 {
            self.registers
                .set_address_register(register, base.wrapping_add(step(size, register)));
        }
        Ok(())
    }

    pub(super) fn data_access<E>(
        &mut self,
        address: u32,
        byte: bool,
    ) -> Result<BusAccess, TimingFault<E>> {
        if !byte && address & 1 != 0 {
            self.frozen = true;
            return Err(TimingFault::AddressError { address });
        }
        Ok(BusAccess {
            logical_address: address,
            bus_address: address & BUS_ADDRESS_MASK & !1,
            function_code: crate::ea::data_space(&self.registers),
            kind: AccessKind::Data,
            lanes: if !byte {
                ByteLanes::Both
            } else if address & 1 == 0 {
                ByteLanes::Upper
            } else {
                ByteLanes::Lower
            },
        })
    }

    pub(super) fn read_data<B: ClockedBus>(
        &mut self,
        bus: &mut B,
        address: u32,
        byte: bool,
        function_code: FunctionCode,
    ) -> Result<u16, TimingFault<B::Error>> {
        let mut access = self.data_access(address, byte)?;
        access.function_code = function_code;
        let value = self.read_access(bus, access)?;
        Ok(if !byte {
            value
        } else if address & 1 == 0 {
            value >> 8
        } else {
            value & 0xff
        })
    }

    pub(super) fn write_data<B: ClockedBus>(
        &mut self,
        bus: &mut B,
        address: u32,
        byte: bool,
        value: u16,
    ) -> Result<(), TimingFault<B::Error>> {
        let access = self.data_access(address, byte)?;
        let value = if !byte {
            value
        } else if address & 1 == 0 {
            (value & 0xff) << 8
        } else {
            value & 0xff
        };
        self.write_access(bus, access, value)
    }
}

#[cfg(test)]
mod extension_tests;
#[cfg(test)]
mod tests;
