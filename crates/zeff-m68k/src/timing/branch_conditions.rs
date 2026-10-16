use super::moves::address::Stream;
use super::*;
use crate::ea::Size;

impl PrefetchCpu {
    pub(super) fn branch_condition_instruction<B: ClockedBus>(
        &mut self,
        bus: &mut B,
        pipeline: Pipeline,
    ) -> Result<bool, TimingFault<B::Error>> {
        let opcode = pipeline.words[0];
        let pc = pipeline.address;
        if opcode & 0xff00 == 0x6100 {
            let word = opcode as u8 == 0;
            let displacement = if word {
                i32::from(pipeline.words[1] as i16)
            } else {
                i32::from(opcode as u8 as i8)
            };
            let target = pc.wrapping_add(2).wrapping_add_signed(displacement);
            let return_pc = pc.wrapping_add(if word { 4 } else { 2 });
            self.idle(bus, 2)?;
            self.push_stack(bus, return_pc)?;
            self.control_refill(bus, target)?;
            return Ok(true);
        }
        let condition = crate::alu::condition(self.registers.status_register(), (opcode >> 8) & 15);
        if opcode & 0xf0f8 == 0x50c8 {
            let register = usize::from(opcode & 7);
            let target = pc
                .wrapping_add(2)
                .wrapping_add_signed(i32::from(pipeline.words[1] as i16));
            let fallthrough = pc.wrapping_add(4);
            self.idle(bus, 2)?;
            if condition {
                self.idle(bus, 2)?;
                self.control_refill(bus, fallthrough)?;
            } else {
                let counter = (self.registers.d[register] as u16).wrapping_sub(1);
                let first = self.fetch(bus, target)?;
                let (address, first) = if counter == u16::MAX {
                    (fallthrough, self.fetch(bus, fallthrough)?)
                } else {
                    (target, first)
                };
                self.registers.d[register] =
                    (self.registers.d[register] & 0xffff_0000) | u32::from(counter);
                let second = self.fetch(bus, address.wrapping_add(2))?;
                self.registers.pc = address;
                self.pipeline = Some(Pipeline {
                    address,
                    words: [first, second],
                });
            }
            return Ok(true);
        }
        if opcode & 0xf0c0 != 0x50c0 {
            return Ok(false);
        }
        let mode = (opcode >> 3) & 7;
        let register = usize::from(opcode & 7);
        if !crate::instruction::alterable(mode, register as u16) {
            return Err(TimingFault::UnsupportedInstruction { pc, opcode });
        }
        let value = if condition { 255 } else { 0 };
        let mut stream = Stream::new(pipeline);
        if mode == 0 {
            if !condition {
                self.registers.d[register] &= !255;
            }
            let words = stream.finish(self, bus)?;
            if condition {
                self.registers.d[register] |= 255;
                self.idle(bus, 2)?;
            }
            self.registers.pc = stream.cursor;
            self.pipeline = Some(Pipeline {
                address: stream.cursor,
                words,
            });
        } else {
            let (_, address) = self.read_operand(bus, &mut stream, (Size::Byte, mode, register))?;
            let words = stream.finish(self, bus)?;
            self.write_data(bus, address, true, value)?;
            self.registers.pc = stream.cursor;
            self.pipeline = Some(Pipeline {
                address: stream.cursor,
                words,
            });
        }
        Ok(true)
    }

    pub(super) fn push_stack<B: ClockedBus>(
        &mut self,
        bus: &mut B,
        value: u32,
    ) -> Result<(), TimingFault<B::Error>> {
        let address = self.registers.stack_pointer().wrapping_sub(4);
        self.registers.set_address_register(7, address);
        self.write_active_stack(bus, address, (value >> 16) as u16)?;
        self.write_active_stack(bus, address.wrapping_add(2), value as u16)
    }

    pub(super) fn write_active_stack<B: ClockedBus>(
        &mut self,
        bus: &mut B,
        address: u32,
        value: u16,
    ) -> Result<(), TimingFault<B::Error>> {
        if address & 1 != 0 {
            self.frozen = true;
            return Err(TimingFault::AddressError { address });
        }
        self.write_access(
            bus,
            BusAccess {
                logical_address: address,
                bus_address: address & BUS_ADDRESS_MASK,
                function_code: crate::ea::data_space(&self.registers),
                kind: AccessKind::Stack,
                lanes: ByteLanes::Both,
            },
            value,
        )
    }

    pub(super) fn control_refill<B: ClockedBus>(
        &mut self,
        bus: &mut B,
        address: u32,
    ) -> Result<(), TimingFault<B::Error>> {
        let first = self.fetch(bus, address)?;
        let second = self.fetch(bus, address.wrapping_add(2))?;
        self.registers.pc = address;
        self.pipeline = Some(Pipeline {
            address,
            words: [first, second],
        });
        Ok(())
    }
}

#[cfg(test)]
mod failure_tests;
#[cfg(test)]
mod tests;
