use super::*;
use crate::ea::Size;

#[derive(Clone, Copy)]
pub(super) struct RegisterMoveProfile {
    size: Size,
    source: MoveRegister,
    destination: MoveRegister,
}

impl RegisterMoveProfile {
    pub(super) fn decode(opcode: u16) -> Option<Self> {
        let size = match opcode >> 12 {
            1 => Size::Byte,
            2 => Size::Long,
            3 => Size::Word,
            _ => return None,
        };
        let destination_mode = (opcode >> 6) & 7;
        let source_mode = (opcode >> 3) & 7;
        if destination_mode > 1 || size == Size::Byte && (destination_mode == 1 || source_mode == 1)
        {
            return None;
        }
        let index = usize::from(opcode & 7);
        let source = match source_mode {
            0 => MoveRegister::Data(index),
            1 => MoveRegister::Address(index),
            _ => return None,
        };
        Some(Self {
            size,
            source,
            destination: if destination_mode == 1 {
                MoveRegister::Address(usize::from((opcode >> 9) & 7))
            } else {
                MoveRegister::Data(usize::from((opcode >> 9) & 7))
            },
        })
    }
}

impl PrefetchCpu {
    pub(super) fn physical_register_move<B: InstructionBus>(
        &mut self,
        bus: &mut B,
        pipeline: Pipeline,
        profile: RegisterMoveProfile,
    ) -> Result<(), TransferExit<B::Error>> {
        let value = profile.source.read(&self.registers);
        self.physical_reserve()?;
        profile
            .destination
            .publish(&mut self.registers, profile.size, value);
        let phase = self.prefetch_phase(
            InstructionPhase::OperandRefill,
            pipeline.address.wrapping_add(4),
            pipeline.address.wrapping_add(4),
            pipeline.words[1],
        );
        let refill = self.physical_read(bus, phase)?;
        self.registers.pc = pipeline.address.wrapping_add(2);
        self.pipeline = Some(Pipeline {
            address: self.registers.pc,
            words: [pipeline.words[1], refill],
        });
        Ok(())
    }
}
