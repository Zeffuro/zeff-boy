use super::moves::address::{Stream, extension_address};
use super::*;
use crate::constants::RESET_OUTPUT_CLOCKS;
use crate::ea::Size;

impl PrefetchCpu {
    pub(super) fn reset_instruction<B: ClockedBus>(
        &mut self,
        bus: &mut B,
        pipeline: Pipeline,
    ) -> Result<(), TimingFault<B::Error>> {
        self.idle(bus, 4)?;
        if self
            .clock
            .0
            .checked_add(u64::from(RESET_OUTPUT_CLOCKS))
            .is_none()
        {
            self.frozen = true;
            return Err(TimingFault::ClockOverflow);
        }
        match bus.begin_reset_output(self.clock) {
            Ok(BusResponse::Complete(())) => {}
            Ok(BusResponse::Unsupported) => {
                self.frozen = true;
                return Err(TimingFault::ResetOutputUnsupported);
            }
            Err(source) => {
                self.frozen = true;
                return Err(TimingFault::ResetOutput { source });
            }
        }
        let held = self.idle(bus, RESET_OUTPUT_CLOCKS);
        bus.end_reset_output(self.clock);
        held?;
        let next = self.fetch(bus, pipeline.address.wrapping_add(4))?;
        self.registers.pc = pipeline.address.wrapping_add(2);
        self.pipeline = Some(Pipeline {
            address: self.registers.pc,
            words: [pipeline.words[1], next],
        });
        Ok(())
    }

    pub(super) fn tas_instruction<B: ClockedBus>(
        &mut self,
        bus: &mut B,
        pipeline: Pipeline,
    ) -> Result<bool, TimingFault<B::Error>> {
        let opcode = pipeline.words[0];
        if opcode & 0xffc0 != 0x4ac0 {
            return Ok(false);
        }
        let mode = (opcode >> 3) & 7;
        let register = usize::from(opcode & 7);
        if !crate::instruction::alterable(mode, register as u16) {
            return Err(TimingFault::UnsupportedInstruction {
                pc: pipeline.address,
                opcode,
            });
        }
        let mut stream = Stream::new(pipeline);
        if mode == 0 {
            let original = self.registers.d[register];
            crate::alu::logic(&mut self.registers, Size::Byte, original);
            self.registers.d[register] = original | 0x80;
        } else {
            let base = self.registers.address_register(register).unwrap();
            let step = if register == 7 { 2 } else { 1 };
            let address = if mode >= 5 {
                extension_address(self, bus, &mut stream, mode, register, false)?
            } else if mode == 4 {
                self.idle(bus, 2)?;
                let address = base.wrapping_sub(step);
                self.registers.set_address_register(register, address);
                address
            } else {
                base
            };
            if mode == 3 {
                self.registers
                    .set_address_register(register, base.wrapping_add(step));
            }
            let access = self.data_access(address, true)?;
            match bus.begin_rmw(access, self.clock) {
                Ok(BusResponse::Complete(())) => {}
                Ok(BusResponse::Unsupported) => {
                    self.frozen = true;
                    return Err(TimingFault::RmwOwnershipUnsupported { access });
                }
                Err(source) => {
                    self.frozen = true;
                    return Err(TimingFault::RmwOwnership { access, source });
                }
            }
            let held = (|| {
                let original = self.read_data(bus, address, true, access.function_code)?;
                crate::alu::logic(&mut self.registers, Size::Byte, u32::from(original));
                self.idle(bus, 2)?;
                self.write_data(bus, address, true, original | 0x80)
            })();
            bus.end_rmw(self.clock);
            held?;
        }
        let words = stream.finish(self, bus)?;
        self.registers.pc = stream.cursor;
        self.pipeline = Some(Pipeline {
            address: stream.cursor,
            words,
        });
        Ok(true)
    }
}

#[cfg(test)]
mod failure_tests;
#[cfg(test)]
mod spy;
#[cfg(test)]
mod tests;
