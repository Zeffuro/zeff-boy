use super::*;
use crate::SUPERVISOR;
use crate::constants::{
    VECTOR_ENTRY_BYTES, VECTOR_ILLEGAL_INSTRUCTION, VECTOR_LINE_A, VECTOR_LINE_F,
    VECTOR_PRIVILEGE_VIOLATION, VECTOR_TRAP_BASE,
};

pub(super) struct Completion {
    pub exception: Option<u8>,
    pub trace: bool,
}

impl PrefetchCpu {
    pub(super) fn control<B: ClockedBus>(
        &mut self,
        bus: &mut B,
        pipeline: Pipeline,
    ) -> Result<Option<Completion>, TimingFault<B::Error>> {
        let pc = pipeline.address;
        let opcode = pipeline.words[0];
        let privileged = matches!(
            opcode,
            0x4e70 | 0x4e72 | 0x4e73 | 0x007c | 0x027c | 0x0a7c | 0x46fc
        ) || opcode & 0xfff8 == 0x46c0;
        let trap = if privileged && !self.registers.supervisor() {
            Some((VECTOR_PRIVILEGE_VIOLATION, pc, false))
        } else if opcode & 0xfff0 == 0x4e40 {
            Some((
                VECTOR_TRAP_BASE + (opcode as u8 & 15),
                pc.wrapping_add(2),
                true,
            ))
        } else if matches!(opcode, 0x4afa..=0x4afc) {
            Some((VECTOR_ILLEGAL_INSTRUCTION, pc, false))
        } else if matches!(opcode >> 12, 10 | 15) {
            Some((
                if opcode >> 12 == 10 {
                    VECTOR_LINE_A
                } else {
                    VECTOR_LINE_F
                },
                pc,
                false,
            ))
        } else {
            None
        };
        if let Some((vector, saved_pc, trace)) = trap {
            self.short_exception(bus, vector, saved_pc)?;
            return Ok(Some(Completion {
                exception: Some(vector),
                trace,
            }));
        }
        match opcode {
            0x4e70 => self.reset_instruction(bus, pipeline)?,
            0x4e72 => {
                self.idle(bus, 4)?;
                self.registers.set_status_register(pipeline.words[1]);
                self.registers.pc = pc.wrapping_add(4);
                self.pipeline = None;
                self.stopped = true;
            }
            0x4e73 | 0x4e75 => {
                let rte = opcode == 0x4e73;
                let sp = if rte {
                    self.registers.ssp
                } else {
                    self.registers.stack_pointer()
                };
                let fc = if rte {
                    FunctionCode::SupervisorData
                } else {
                    crate::ea::data_space(&self.registers)
                };
                let sr = if rte {
                    Some(self.read_word(bus, sp, fc, AccessKind::Stack)?)
                } else {
                    None
                };
                let address = sp.wrapping_add(if rte { 2 } else { 0 });
                let high = self.read_word(bus, address, fc, AccessKind::Stack)?;
                let low = self.read_word(bus, address.wrapping_add(2), fc, AccessKind::Stack)?;
                let target = (u32::from(high) << 16) | u32::from(low);
                if let Some(sr) = sr {
                    self.registers.ssp = sp.wrapping_add(6);
                    self.registers.set_status_register(sr);
                } else {
                    self.registers.set_address_register(7, sp.wrapping_add(4));
                }
                self.refill(bus, target)?;
            }
            0x007c | 0x027c | 0x0a7c => {
                self.fetch(bus, pc.wrapping_add(4))?;
                self.idle(bus, 6)?;
                let sr = self.registers.status_register();
                let immediate = pipeline.words[1];
                self.registers.set_status_register(match opcode {
                    0x007c => sr | immediate,
                    0x027c => sr & immediate,
                    _ => sr ^ immediate,
                });
                self.idle(bus, 2)?;
                self.refill(bus, pc.wrapping_add(4))?;
            }
            0x46fc => {
                self.fetch(bus, pc.wrapping_add(4))?;
                self.idle(bus, 2)?;
                self.registers.set_status_register(pipeline.words[1]);
                self.idle(bus, 2)?;
                self.refill(bus, pc.wrapping_add(4))?;
            }
            _ if opcode & 0xfff8 == 0x46c0 => {
                self.idle(bus, 2)?;
                self.registers
                    .set_status_register(self.registers.d[usize::from(opcode & 7)] as u16);
                self.idle(bus, 2)?;
                self.refill(bus, pc.wrapping_add(2))?;
            }
            _ => return Ok(None),
        }
        Ok(Some(Completion {
            exception: None,
            trace: true,
        }))
    }

    fn refill<B: ClockedBus>(
        &mut self,
        bus: &mut B,
        target: u32,
    ) -> Result<(), TimingFault<B::Error>> {
        let first = self.fetch(bus, target)?;
        let second = self.fetch(bus, target.wrapping_add(2))?;
        self.registers.pc = target;
        self.pipeline = Some(Pipeline {
            address: target,
            words: [first, second],
        });
        Ok(())
    }

    pub(super) fn short_exception<B: ClockedBus>(
        &mut self,
        bus: &mut B,
        vector: u8,
        saved_pc: u32,
    ) -> Result<(), TimingFault<B::Error>> {
        self.short_exception_with_idle(bus, vector, saved_pc, 4)
    }

    pub(super) fn short_exception_with_idle<B: ClockedBus>(
        &mut self,
        bus: &mut B,
        vector: u8,
        saved_pc: u32,
        idle: u32,
    ) -> Result<(), TimingFault<B::Error>> {
        let sr = self.registers.status_register();
        let sp = self.registers.ssp;
        if idle != 0 {
            self.idle(bus, idle)?;
        }
        self.write_word(bus, sp.wrapping_sub(2), saved_pc as u16)?;
        self.write_word(bus, sp.wrapping_sub(6), sr)?;
        self.write_word(bus, sp.wrapping_sub(4), (saved_pc >> 16) as u16)?;
        let address = u32::from(vector) * VECTOR_ENTRY_BYTES;
        let high = self.read_word(
            bus,
            address,
            FunctionCode::SupervisorData,
            AccessKind::Vector,
        )?;
        let low = self.read_word(
            bus,
            address + 2,
            FunctionCode::SupervisorData,
            AccessKind::Vector,
        )?;
        let target = (u32::from(high) << 16) | u32::from(low);
        let first = self.read_word(
            bus,
            target,
            FunctionCode::SupervisorProgram,
            AccessKind::Prefetch,
        )?;
        self.idle(bus, 2)?;
        let second = self.read_word(
            bus,
            target.wrapping_add(2),
            FunctionCode::SupervisorProgram,
            AccessKind::Prefetch,
        )?;
        self.registers
            .set_status_register((sr | SUPERVISOR) & !TRACE);
        self.registers.ssp = sp.wrapping_sub(6);
        self.registers.pc = target;
        self.pipeline = Some(Pipeline {
            address: target,
            words: [first, second],
        });
        self.stopped = false;
        Ok(())
    }
}

#[cfg(test)]
mod tests;
