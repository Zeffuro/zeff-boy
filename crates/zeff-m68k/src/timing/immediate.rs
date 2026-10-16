use super::moves::address::Stream;
use super::*;
use crate::constants::CCR_MASK;
use crate::ea::Size;

impl PrefetchCpu {
    pub(super) fn immediate_instruction<B: ClockedBus>(
        &mut self,
        bus: &mut B,
        pipeline: Pipeline,
    ) -> Result<bool, TimingFault<B::Error>> {
        let opcode = pipeline.words[0];
        if matches!(opcode, 0x003c | 0x023c | 0x0a3c) {
            self.fetch(bus, pipeline.address.wrapping_add(4))?;
            self.idle(bus, 6)?;
            let sr = self.registers.status_register();
            let immediate = pipeline.words[1];
            let result = match opcode {
                0x003c => sr | immediate,
                0x023c => sr & immediate,
                _ => sr ^ immediate,
            };
            self.registers
                .set_status_register((sr & !CCR_MASK) | (result & CCR_MASK));
            self.idle(bus, 2)?;
            // CCR setters repeat the first fetch, just like the original SR setters.
            let pc = pipeline.address.wrapping_add(4);
            let first = self.fetch(bus, pc)?;
            let second = self.fetch(bus, pc.wrapping_add(2))?;
            self.registers.pc = pc;
            self.pipeline = Some(Pipeline {
                address: pc,
                words: [first, second],
            });
            return Ok(true);
        }
        let family = opcode & 0xff00;
        if !matches!(family, 0x0000 | 0x0200 | 0x0400 | 0x0600 | 0x0a00 | 0x0c00) {
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
        if mode == 1 || mode == 7 && register > 1 {
            return Ok(false);
        }
        let mut stream = Stream::new(pipeline);
        let high = stream.take(self, bus, false)?;
        let immediate = if size == Size::Long {
            (u32::from(high) << 16) | u32::from(stream.take(self, bus, false)?)
        } else {
            size.truncate(u32::from(high))
        };
        let (old, address) = self.read_operand(bus, &mut stream, (size, mode, register))?;
        if family == 0x0c00 {
            self.reserve_transfer()?;
        }
        let result = match family {
            0x0000 => old | immediate,
            0x0200 => old & immediate,
            0x0400 | 0x0c00 => {
                crate::alu::subtract(&mut self.registers, size, old, immediate, family != 0x0c00)
            }
            0x0600 => crate::alu::add(&mut self.registers, size, old, immediate),
            _ => old ^ immediate,
        };
        if matches!(family, 0x0000 | 0x0200 | 0x0a00) {
            crate::alu::logic(&mut self.registers, size, result);
        }
        if mode == 0 && family != 0x0c00 {
            self.registers.d[register] =
                (self.registers.d[register] & !size.mask()) | size.truncate(result);
        }
        let words = stream.finish(self, bus)?;
        if mode == 0 {
            if size == Size::Long {
                self.idle(bus, if family == 0x0c00 { 2 } else { 4 })?;
            }
        } else if family != 0x0c00 {
            if size == Size::Long {
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
mod failure_tests;
#[cfg(test)]
mod tests;
