use super::control::Completion;
use super::*;
use crate::OVERFLOW;
use crate::constants::{CCR_MASK, VECTOR_PRIVILEGE_VIOLATION, VECTOR_TRAPV};

impl PrefetchCpu {
    pub(super) fn frame_instruction<B: ClockedBus>(
        &mut self,
        bus: &mut B,
        pipeline: Pipeline,
    ) -> Result<Option<Completion>, TimingFault<B::Error>> {
        let opcode = pipeline.words[0];
        let pc = pipeline.address;
        let register = usize::from(opcode & 7);
        if opcode & 0xfff0 == 0x4e60 && !self.registers.supervisor() {
            self.short_exception(bus, VECTOR_PRIVILEGE_VIOLATION, pc)?;
            return Ok(Some(Completion {
                exception: Some(VECTOR_PRIVILEGE_VIOLATION),
                trace: false,
            }));
        }
        if opcode == 0x4e76 && self.registers.status_register() & OVERFLOW != 0 {
            // TRAPV samples supervisor-program space before stacking.
            self.read_word(
                bus,
                pc.wrapping_add(4),
                FunctionCode::SupervisorProgram,
                AccessKind::Prefetch,
            )?;
            self.short_exception_with_idle(bus, VECTOR_TRAPV, pc.wrapping_add(2), 0)?;
            return Ok(Some(Completion {
                exception: Some(VECTOR_TRAPV),
                trace: true,
            }));
        }
        match opcode {
            _ if opcode & 0xfff8 == 0x4e50 => {
                let old = self.registers.address_register(register).unwrap();
                let frame = self.registers.stack_pointer().wrapping_sub(4);
                let displacement = i32::from(pipeline.words[1] as i16);
                let first = self.fetch(bus, pc.wrapping_add(4))?;
                self.registers.set_address_register(register, frame);
                self.write_active_stack(bus, frame, (old >> 16) as u16)?;
                self.write_active_stack(bus, frame.wrapping_add(2), old as u16)?;
                self.registers
                    .set_address_register(7, frame.wrapping_add_signed(displacement));
                let second = self.fetch(bus, pc.wrapping_add(6))?;
                self.registers.pc = pc.wrapping_add(4);
                self.pipeline = Some(Pipeline {
                    address: self.registers.pc,
                    words: [first, second],
                });
            }
            _ if opcode & 0xfff8 == 0x4e58 => {
                let frame = self.registers.address_register(register).unwrap();
                let fc = crate::ea::data_space(&self.registers);
                let high = self.read_word(bus, frame, fc, AccessKind::Stack)?;
                let low = self.read_word(bus, frame.wrapping_add(2), fc, AccessKind::Stack)?;
                self.registers
                    .set_address_register(7, frame.wrapping_add(4));
                self.registers
                    .set_address_register(register, (u32::from(high) << 16) | u32::from(low));
                self.finish_frame(bus, pipeline)?;
            }
            0x4e77 => {
                let sp = self.registers.stack_pointer();
                let fc = crate::ea::data_space(&self.registers);
                let ccr = self.read_word(bus, sp, fc, AccessKind::Stack)?;
                let high = self.read_word(bus, sp.wrapping_add(2), fc, AccessKind::Stack)?;
                self.registers.set_address_register(7, sp.wrapping_add(6));
                self.registers.set_status_register(
                    (self.registers.status_register() & !CCR_MASK) | (ccr & CCR_MASK),
                );
                let low = self.read_word(bus, sp.wrapping_add(4), fc, AccessKind::Stack)?;
                self.control_refill(bus, (u32::from(high) << 16) | u32::from(low))?;
            }
            _ if opcode & 0xfff0 == 0x4e60 => {
                if opcode & 8 == 0 {
                    self.registers.usp = self.registers.address_register(register).unwrap();
                } else {
                    self.registers
                        .set_address_register(register, self.registers.usp);
                }
                self.finish_frame(bus, pipeline)?;
            }
            0x4e76 => self.finish_frame(bus, pipeline)?,
            _ => return Ok(None),
        }
        Ok(Some(Completion {
            exception: None,
            trace: true,
        }))
    }

    fn finish_frame<B: ClockedBus>(
        &mut self,
        bus: &mut B,
        pipeline: Pipeline,
    ) -> Result<(), TimingFault<B::Error>> {
        let next = self.fetch(bus, pipeline.address.wrapping_add(4))?;
        self.registers.pc = pipeline.address.wrapping_add(2);
        self.pipeline = Some(Pipeline {
            address: self.registers.pc,
            words: [pipeline.words[1], next],
        });
        Ok(())
    }
}

#[cfg(test)]
mod failure_tests;
#[cfg(test)]
mod tests;
