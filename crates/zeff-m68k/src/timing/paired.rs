use super::moves::address::Stream;
use super::*;
use crate::ea::Size;

#[derive(Clone, Copy)]
enum Pair {
    Extend(Size, bool),
    Decimal(bool),
    Compare(Size),
}

fn decode(opcode: u16) -> Option<Pair> {
    if matches!(opcode & 0xf1f8, 0xc108 | 0x8108) {
        return Some(Pair::Decimal(opcode & 0xf000 == 0x8000));
    }
    let size = crate::instruction::size(opcode)?;
    match opcode & 0xf138 {
        0xd108 | 0x9108 => Some(Pair::Extend(size, opcode & 0xf000 == 0x9000)),
        0xb108 => Some(Pair::Compare(size)),
        _ => None,
    }
}

impl PrefetchCpu {
    pub(super) fn paired_instruction<B: ClockedBus>(
        &mut self,
        bus: &mut B,
        pipeline: Pipeline,
    ) -> Result<bool, TimingFault<B::Error>> {
        let opcode = pipeline.words[0];
        let mut stream = Stream::new(pipeline);
        let words = if opcode & 0xffc0 == 0x4800 {
            let mode = (opcode >> 3) & 7;
            let register = usize::from(opcode & 7);
            if mode < 2 || mode == 7 && register > 1 {
                return Ok(false);
            }
            let (value, address) =
                self.read_operand(bus, &mut stream, (Size::Byte, mode, register))?;
            let result = crate::bcd::arithmetic(&mut self.registers, 0, value as u8, true);
            let words = stream.finish(self, bus)?;
            self.write_data(bus, address, true, u16::from(result))?;
            words
        } else {
            let Some(pair) = decode(opcode) else {
                return Ok(false);
            };
            let source = usize::from(opcode & 7);
            let destination = usize::from((opcode >> 9) & 7);
            let size = match pair {
                Pair::Extend(size, _) | Pair::Compare(size) => size,
                Pair::Decimal(_) => Size::Byte,
            };
            let (value, old, address) = if matches!(pair, Pair::Compare(_)) {
                let value = self.compare_operand(bus, source, size, true)?;
                let old = self.compare_operand(bus, destination, size, false)?;
                (value, old, 0)
            } else {
                self.idle(bus, 2)?;
                let (value, _) = self.decrement_operand(bus, source, size)?;
                let (old, address) = self.decrement_operand(bus, destination, size)?;
                (value, old, address)
            };
            let result = match pair {
                Pair::Extend(_, subtract) => {
                    crate::extend::arithmetic(&mut self.registers, size, old, value, subtract)
                }
                Pair::Decimal(subtract) => u32::from(crate::bcd::arithmetic(
                    &mut self.registers,
                    old as u8,
                    value as u8,
                    subtract,
                )),
                Pair::Compare(_) => {
                    crate::alu::subtract(&mut self.registers, size, old, value, false)
                }
            };
            if size == Size::Long && matches!(pair, Pair::Extend(..)) {
                // Long ADDX/SUBX writes its low word before fetching the tail.
                self.write_data(bus, address.wrapping_add(2), false, result as u16)?;
                let words = stream.finish(self, bus)?;
                self.write_data(bus, address, false, (result >> 16) as u16)?;
                words
            } else {
                let words = stream.finish(self, bus)?;
                if !matches!(pair, Pair::Compare(_)) {
                    self.write_data(bus, address, size == Size::Byte, result as u16)?;
                }
                words
            }
        };
        self.registers.pc = stream.cursor;
        self.pipeline = Some(Pipeline {
            address: stream.cursor,
            words,
        });
        Ok(true)
    }

    fn decrement_operand<B: ClockedBus>(
        &mut self,
        bus: &mut B,
        register: usize,
        size: Size,
    ) -> Result<(u32, u32), TimingFault<B::Error>> {
        let base = self.registers.address_register(register).unwrap();
        let step = if size == Size::Byte && register == 7 {
            2
        } else {
            size.bytes()
        };
        let address = base.wrapping_sub(step);
        let fc = crate::ea::data_space(&self.registers);
        if size == Size::Long {
            let low = self.read_data(bus, base.wrapping_sub(2), false, fc)?;
            self.registers.set_address_register(register, address);
            let high = self.read_data(bus, address, false, fc)?;
            Ok(((u32::from(high) << 16) | u32::from(low), address))
        } else {
            self.registers.set_address_register(register, address);
            Ok((
                u32::from(self.read_data(bus, address, size == Size::Byte, fc)?),
                address,
            ))
        }
    }

    fn compare_operand<B: ClockedBus>(
        &mut self,
        bus: &mut B,
        register: usize,
        size: Size,
        source: bool,
    ) -> Result<u32, TimingFault<B::Error>> {
        let address = self.registers.address_register(register).unwrap();
        let step = if size == Size::Byte && register == 7 {
            2
        } else {
            size.bytes()
        };
        let fc = crate::ea::data_space(&self.registers);
        if source {
            self.registers.set_address_register(
                register,
                address.wrapping_add(if size == Size::Long { 2 } else { step }),
            );
        }
        let high = self.read_data(bus, address, size == Size::Byte, fc)?;
        let value = if size == Size::Long {
            if source {
                self.registers
                    .set_address_register(register, address.wrapping_add(4));
            }
            (u32::from(high) << 16)
                | u32::from(self.read_data(bus, address.wrapping_add(2), false, fc)?)
        } else {
            u32::from(high)
        };
        if !source {
            self.registers
                .set_address_register(register, address.wrapping_add(step));
        }
        Ok(value)
    }
}

#[cfg(test)]
mod failure_tests;
#[cfg(test)]
mod tests;
