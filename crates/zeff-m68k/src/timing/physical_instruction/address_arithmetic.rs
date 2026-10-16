use super::*;
use crate::ea::Size;
use memory_reads::MemoryReadProfile;

pub(super) fn legal(opcode: u16) -> bool {
    matches!(opcode >> 12, 9 | 13)
        && matches!(opcode >> 6 & 7, 3 | 7)
        && !(opcode >> 3 & 7 == 7 && opcode & 7 > 4)
}

#[derive(Clone, Copy)]
pub(super) struct AddressArithmeticProfile {
    source: MemoryReadProfile,
    destination: usize,
    subtract: bool,
}

impl AddressArithmeticProfile {
    pub(super) fn decode(opcode: u16) -> Option<Self> {
        if !legal(opcode) {
            return None;
        }
        let size = if opcode >> 6 & 7 == 3 {
            Size::Word
        } else {
            Size::Long
        };
        Some(Self {
            source: MemoryReadProfile::decode(opcode, size)?,
            destination: usize::from(opcode >> 9 & 7),
            subtract: opcode >> 12 == 9,
        })
    }
}

impl PrefetchCpu {
    pub(super) fn physical_address_arithmetic<B: InstructionBus>(
        &mut self,
        bus: &mut B,
        pipeline: Pipeline,
        profile: AddressArithmeticProfile,
    ) -> Result<(), TransferExit<B::Error>> {
        let operand = self.physical_memory_read(bus, pipeline, profile.source)?;
        self.physical_reserve()?;
        let destination = self
            .registers
            .address_register(profile.destination)
            .unwrap();
        let source = profile.source.size.sign_extend(operand.value);
        let result = if profile.subtract {
            destination.wrapping_sub(source)
        } else {
            destination.wrapping_add(source)
        };
        self.registers.set_address_register(
            profile.destination,
            (destination & 0xffff_0000) | (result & 0xffff),
        );
        let address = operand.anchor.wrapping_add(2);
        let phase = self.prefetch_phase(
            InstructionPhase::OperandRefill,
            address,
            address,
            operand.retained,
        );
        let fetched = self.physical_read(bus, phase)?;
        if profile.source.size == Size::Word {
            self.entry_idle(bus, 2)
                .map_err(|e| TransferExit::Fault(InstructionFault::Entry(e)))?;
        }
        self.registers
            .set_address_register(profile.destination, result);
        self.entry_idle(bus, 2)
            .map_err(|e| TransferExit::Fault(InstructionFault::Entry(e)))?;
        self.registers.pc = operand.anchor;
        self.pipeline = Some(Pipeline {
            address: operand.anchor,
            words: [operand.retained, fetched],
        });
        Ok(())
    }
}
