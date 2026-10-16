use super::*;
use crate::bus::{AccessKind, ByteLanes};
use crate::constants::BUS_ADDRESS_MASK;
use crate::ea::Size;
use memory_reads::MemoryReadProfile;

pub(super) fn legal(opcode: u16) -> bool {
    let mode = opcode >> 3 & 7;
    opcode & 0xffc0 == 0x40c0
        && (mode == 0 || matches!(mode, 2..=6) || mode == 7 && opcode & 7 <= 1)
}

#[derive(Clone, Copy)]
pub(super) enum FromSrProfile {
    Data(usize),
    Memory(MemoryReadProfile),
}

impl FromSrProfile {
    pub(super) fn decode(opcode: u16) -> Option<Self> {
        if !legal(opcode) {
            return None;
        }
        Some(if opcode >> 3 & 7 == 0 {
            Self::Data(usize::from(opcode & 7))
        } else {
            Self::Memory(MemoryReadProfile::decode(opcode, Size::Word)?)
        })
    }
}

impl PrefetchCpu {
    pub(super) fn physical_from_sr<B: InstructionBus>(
        &mut self,
        bus: &mut B,
        pipeline: Pipeline,
        profile: FromSrProfile,
    ) -> Result<(), TransferExit<B::Error>> {
        let sr = self.registers.status_register();
        let (anchor, retained, fetched) = match profile {
            FromSrProfile::Data(register) => {
                self.physical_reserve()?;
                self.registers.d[register] =
                    (self.registers.d[register] & 0xffff_0000) | u32::from(sr);
                let address = pipeline.address.wrapping_add(4);
                let phase = self.prefetch_phase(
                    InstructionPhase::OperandRefill,
                    address,
                    address,
                    pipeline.words[0],
                );
                let fetched = self.physical_read(bus, phase)?;
                self.entry_idle(bus, 2)
                    .map_err(|e| TransferExit::Fault(InstructionFault::Entry(e)))?;
                (pipeline.address.wrapping_add(2), pipeline.words[1], fetched)
            }
            FromSrProfile::Memory(profile) => {
                let operand = self.physical_memory_read(bus, pipeline, profile)?;
                let address = operand.anchor.wrapping_add(2);
                let phase = self.prefetch_phase(
                    InstructionPhase::OperandRefill,
                    address,
                    address,
                    pipeline.words[0],
                );
                let fetched = self.physical_read(bus, phase)?;
                let phase = TransferPhase {
                    phase: InstructionPhase::OperandWriteHigh,
                    access: BusAccess {
                        logical_address: operand.address,
                        bus_address: operand.address & BUS_ADDRESS_MASK & !1,
                        function_code: crate::ea::data_space(&self.registers),
                        kind: AccessKind::Data,
                        lanes: ByteLanes::Both,
                    },
                    direction: Direction::Write,
                    internal_pc: address,
                    ird: operand.retained,
                };
                self.physical_write(bus, phase, sr)?;
                (operand.anchor, operand.retained, fetched)
            }
        };
        self.registers.pc = anchor;
        self.pipeline = Some(Pipeline {
            address: anchor,
            words: [retained, fetched],
        });
        Ok(())
    }
}
