use super::*;
use crate::bus::{AccessKind, ByteLanes};
use crate::constants::BUS_ADDRESS_MASK;
use crate::ea::Size;
use memory_reads::MemoryReadProfile;

pub(super) fn legal(opcode: u16) -> bool {
    let mode = opcode >> 3 & 7;
    matches!(opcode & 0xff00, 0x4000 | 0x4200 | 0x4400 | 0x4600)
        && opcode >> 6 & 3 < 3
        && (mode == 0 || matches!(mode, 2..=6) || mode == 7 && opcode & 7 <= 1)
}

#[derive(Clone, Copy)]
pub(super) struct RmwProfile {
    source: Option<MemoryReadProfile>,
    size: Size,
    register: usize,
    operation: u16,
}

impl RmwProfile {
    pub(super) fn decode(opcode: u16) -> Option<Self> {
        if !legal(opcode) {
            return None;
        }
        let operation = opcode & 0xff00;
        let register_source = opcode >> 3 & 7 == 0;
        let size = match opcode >> 6 & 3 {
            0 => Size::Byte,
            1 => Size::Word,
            2 => Size::Long,
            _ => return None,
        };
        Some(Self {
            source: if register_source {
                None
            } else {
                Some(MemoryReadProfile::decode(opcode, size)?)
            },
            size,
            register: usize::from(opcode & 7),
            operation,
        })
    }
}

impl PrefetchCpu {
    pub(super) fn physical_rmw<B: InstructionBus>(
        &mut self,
        bus: &mut B,
        pipeline: Pipeline,
        profile: RmwProfile,
    ) -> Result<(), TransferExit<B::Error>> {
        let Some(source) = profile.source else {
            return self.physical_rmw_register(bus, pipeline, profile);
        };
        let size = profile.size;
        let operand = self.physical_memory_read(bus, pipeline, source)?;
        let mut full = self.registers;
        let result = rmw_result(&mut full, profile.operation, size, operand.value);
        self.physical_reserve()?;
        rmw_result(
            &mut self.registers,
            profile.operation,
            if size == Size::Long { Size::Word } else { size },
            operand.value,
        );
        let capture_pc = operand.anchor.wrapping_add(2);
        let phase = self.prefetch_phase(
            InstructionPhase::OperandRefill,
            capture_pc,
            capture_pc,
            pipeline.words[0],
        );
        let fetched = self.physical_read(bus, phase)?;
        if size == Size::Long {
            self.physical_reserve()?;
            self.registers.set_status_register(full.status_register());
            let phase = self.rmw_write_phase(
                InstructionPhase::OperandWriteLow,
                operand.address.wrapping_add(2),
                size,
                capture_pc,
                pipeline.words[0],
            );
            self.physical_write(bus, phase, result as u16)?;
        }
        let phase = self.rmw_write_phase(
            InstructionPhase::OperandWriteHigh,
            operand.address,
            size,
            capture_pc,
            operand.retained,
        );
        let high = if size == Size::Long {
            (result >> 16) as u16
        } else if size == Size::Byte && operand.address & 1 == 0 {
            (result as u16) << 8
        } else {
            result as u16
        };
        self.physical_write(bus, phase, high)?;
        self.registers.pc = operand.anchor;
        self.pipeline = Some(Pipeline {
            address: operand.anchor,
            words: [operand.retained, fetched],
        });
        Ok(())
    }

    fn physical_rmw_register<B: InstructionBus>(
        &mut self,
        bus: &mut B,
        pipeline: Pipeline,
        profile: RmwProfile,
    ) -> Result<(), TransferExit<B::Error>> {
        self.physical_reserve()?;
        let old = self.registers.d[profile.register];
        let result = rmw_result(&mut self.registers, profile.operation, profile.size, old);
        let published_size = if profile.size == Size::Long {
            Size::Word
        } else {
            profile.size
        };
        self.registers.d[profile.register] =
            (old & !published_size.mask()) | published_size.truncate(result);
        let address = pipeline.address.wrapping_add(4);
        let phase = self.prefetch_phase(
            InstructionPhase::OperandRefill,
            address,
            address,
            if profile.size == Size::Long {
                pipeline.words[0]
            } else {
                pipeline.words[1]
            },
        );
        let fetched = self.physical_read(bus, phase)?;
        if profile.size == Size::Long {
            self.registers.d[profile.register] = result;
            self.entry_idle(bus, 2)
                .map_err(|e| TransferExit::Fault(InstructionFault::Entry(e)))?;
        }
        self.registers.pc = pipeline.address.wrapping_add(2);
        self.pipeline = Some(Pipeline {
            address: self.registers.pc,
            words: [pipeline.words[1], fetched],
        });
        Ok(())
    }

    fn rmw_write_phase(
        &self,
        phase: InstructionPhase,
        address: u32,
        size: Size,
        internal_pc: u32,
        ird: u16,
    ) -> TransferPhase {
        TransferPhase {
            phase,
            access: BusAccess {
                logical_address: address,
                bus_address: address & BUS_ADDRESS_MASK & !1,
                function_code: crate::ea::data_space(&self.registers),
                kind: AccessKind::Data,
                lanes: if size != Size::Byte {
                    ByteLanes::Both
                } else if address & 1 == 0 {
                    ByteLanes::Upper
                } else {
                    ByteLanes::Lower
                },
            },
            direction: Direction::Write,
            internal_pc,
            ird,
        }
    }
}

fn rmw_result(registers: &mut crate::Registers, operation: u16, size: Size, value: u32) -> u32 {
    match operation {
        0x4000 => crate::extend::arithmetic(registers, size, 0, value, true),
        0x4400 => crate::alu::subtract(registers, size, 0, value, true),
        _ => {
            let result = if operation == 0x4600 {
                size.truncate(!value)
            } else {
                0
            };
            crate::alu::logic(registers, size, result);
            result
        }
    }
}
