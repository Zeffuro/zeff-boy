use super::moves::address::{Stream, extension_address};
use super::*;
use crate::ea::Size;

#[derive(Clone, Copy)]
enum Destination {
    Data(usize),
    Address(usize),
    Memory,
}

#[derive(Clone, Copy)]
struct Binary {
    family: u16,
    size: Size,
    mode: u16,
    register: usize,
    destination: Destination,
    quick: Option<u32>,
}

fn decode(opcode: u16) -> Option<Binary> {
    let family = opcode >> 12;
    let mode = (opcode >> 3) & 7;
    let register = usize::from(opcode & 7);
    let opmode = (opcode >> 6) & 7;
    let address = matches!(family, 9 | 11 | 13) && matches!(opmode, 3 | 7);
    let quick = family == 5 && opmode & 3 < 3;
    if !matches!(family, 8 | 9 | 11 | 12 | 13) && !quick {
        return None;
    }
    let size = if address {
        if opmode == 3 { Size::Word } else { Size::Long }
    } else {
        match opmode & 3 {
            0 => Size::Byte,
            1 => Size::Word,
            2 => Size::Long,
            _ => return None,
        }
    };
    // Leave register routes and their special aliases to the existing whitelist.
    if mode < 2 || mode == 7 && register > 4 {
        return None;
    }
    let memory_destination = quick || !address && opmode >= 4;
    if memory_destination && mode == 7 && register > 1 {
        return None;
    }
    let destination = if memory_destination {
        Destination::Memory
    } else if address {
        Destination::Address(usize::from((opcode >> 9) & 7))
    } else {
        Destination::Data(usize::from((opcode >> 9) & 7))
    };
    let quick = quick.then(|| {
        let value = u32::from((opcode >> 9) & 7);
        if value == 0 { 8 } else { value }
    });
    Some(Binary {
        family: if quick.is_some() {
            if opcode & 0x0100 == 0 { 13 } else { 9 }
        } else {
            family
        },
        size,
        mode,
        register,
        destination,
        quick,
    })
}

impl PrefetchCpu {
    pub(super) fn binary_instruction<B: ClockedBus>(
        &mut self,
        bus: &mut B,
        pipeline: Pipeline,
    ) -> Result<bool, TimingFault<B::Error>> {
        let Some(operation) = decode(pipeline.words[0]) else {
            return Ok(false);
        };
        let Binary {
            family,
            size,
            mode,
            register,
            destination,
            quick,
        } = operation;
        let mut stream = Stream::new(pipeline);
        let (value, address) = self.binary_operand(bus, &mut stream, operation)?;
        let memory = matches!(destination, Destination::Memory);
        let (old, source, arithmetic_size) = match destination {
            Destination::Memory => (
                value,
                quick.unwrap_or(self.registers.d[usize::from((pipeline.words[0] >> 9) & 7)]),
                size,
            ),
            Destination::Data(register) => (self.registers.d[register], value, size),
            Destination::Address(register) => (
                self.registers.address_register(register).unwrap(),
                size.sign_extend(value),
                Size::Long,
            ),
        };
        let comparison = family == 11 && !memory;
        let split_data = matches!(destination, Destination::Data(_))
            && matches!(family, 8 | 9 | 12 | 13)
            && matches!(size, Size::Word | Size::Long)
            && !(mode == 7 && register == 4);
        if split_data {
            self.reserve_transfer()?;
        }
        let result = match family {
            9 if matches!(destination, Destination::Address(_)) => old.wrapping_sub(source),
            9 => crate::alu::subtract(&mut self.registers, arithmetic_size, old, source, true),
            11 if comparison => {
                crate::alu::subtract(&mut self.registers, arithmetic_size, old, source, false)
            }
            13 if !matches!(destination, Destination::Address(_)) => {
                crate::alu::add(&mut self.registers, arithmetic_size, old, source)
            }
            13 => old.wrapping_add(source),
            8 => old | source,
            11 => old ^ source,
            12 => old & source,
            _ => unreachable!(),
        };
        if matches!(family, 8 | 12) || family == 11 && memory {
            crate::alu::logic(&mut self.registers, size, result);
        }
        let split_address = matches!(destination, Destination::Address(_))
            && !comparison
            && !(mode == 7 && register == 4);
        match destination {
            Destination::Data(register) if !comparison => {
                let mask = if split_data { 0xffff } else { size.mask() };
                self.registers.d[register] = (old & !mask) | (result & mask);
            }
            Destination::Address(register) if !comparison => {
                if split_address {
                    self.reserve_transfer()?;
                    self.registers
                        .set_address_register(register, (old & 0xffff_0000) | (result & 0xffff));
                } else {
                    self.registers.set_address_register(register, result);
                }
            }
            _ => {}
        }
        let words = stream.finish(self, bus)?;
        if memory {
            if size == Size::Long {
                self.write_data(bus, address.wrapping_add(2), false, result as u16)?;
                self.write_data(bus, address, false, (result >> 16) as u16)?;
            } else {
                self.write_data(bus, address, size == Size::Byte, result as u16)?;
            }
        } else if split_data && size == Size::Long {
            let Destination::Data(register) = destination else {
                unreachable!()
            };
            self.registers.d[register] = result;
            self.idle(bus, 2)?;
        } else if split_address {
            if size == Size::Word {
                self.idle(bus, 2)?;
            }
            let Destination::Address(register) = destination else {
                unreachable!()
            };
            self.registers.set_address_register(register, result);
            self.idle(bus, 2)?;
        } else {
            let internal = if comparison {
                if matches!(destination, Destination::Address(_)) || size == Size::Long {
                    2
                } else {
                    0
                }
            } else if matches!(destination, Destination::Address(_)) && size == Size::Word
                || size == Size::Long && mode == 7 && register == 4
            {
                4
            } else if size == Size::Long {
                2
            } else {
                0
            };
            if internal != 0 {
                self.idle(bus, internal)?;
            }
        }
        self.registers.pc = stream.cursor;
        self.pipeline = Some(Pipeline {
            address: stream.cursor,
            words,
        });
        Ok(true)
    }

    fn binary_operand<B: ClockedBus>(
        &mut self,
        bus: &mut B,
        stream: &mut Stream,
        operation: Binary,
    ) -> Result<(u32, u32), TimingFault<B::Error>> {
        let Binary {
            size,
            mode,
            register,
            ..
        } = operation;
        if mode == 7 && register == 4 {
            let high = stream.take(self, bus, false)?;
            let value = if size == Size::Long {
                (u32::from(high) << 16) | u32::from(stream.take(self, bus, false)?)
            } else {
                size.truncate(u32::from(high))
            };
            return Ok((value, 0));
        }
        let base = self.registers.address_register(register).unwrap();
        let increment = if size == Size::Byte && register == 7 {
            2
        } else {
            size.bytes()
        };
        let address = if mode >= 5 {
            extension_address(self, bus, stream, mode, register, false)?
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
        let fc = if mode == 7 && matches!(register, 2 | 3) {
            crate::ea::program_space(&self.registers)
        } else {
            crate::ea::data_space(&self.registers)
        };
        let high = self.read_data(bus, address, size == Size::Byte, fc)?;
        let value = if size == Size::Long {
            if mode == 3 {
                self.registers
                    .set_address_register(register, base.wrapping_add(4));
            }
            (u32::from(high) << 16)
                | u32::from(self.read_data(bus, address.wrapping_add(2), false, fc)?)
        } else {
            u32::from(high)
        };
        Ok((value, address))
    }
}

#[cfg(test)]
mod failure_tests;
#[cfg(test)]
mod tests;
