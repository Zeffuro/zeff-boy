use super::*;
use crate::constants::CCR_MASK;
use crate::ea::Size;
use memory_reads::MemoryReadProfile;

pub(super) fn legal(opcode: u16) -> bool {
    let mode = opcode >> 3 & 7;
    matches!(opcode & 0xffc0, 0x44c0 | 0x46c0)
        && (mode == 0 || matches!(mode, 2..=6) || mode == 7 && opcode & 7 <= 4)
}

#[derive(Clone, Copy)]
enum StatusSource {
    Dn(usize),
    Immediate,
    Memory(MemoryReadProfile),
}

#[derive(Clone, Copy)]
pub(super) struct StatusProfile {
    source: StatusSource,
    to_sr: bool,
}

impl StatusProfile {
    pub(super) fn decode(opcode: u16, supervisor: bool) -> Option<Self> {
        let to_sr = opcode & 0xffc0 == 0x46c0;
        if !legal(opcode) || to_sr && !supervisor {
            return None;
        }
        Some(Self {
            source: match (opcode >> 3 & 7, opcode & 7) {
                (0, register) => StatusSource::Dn(usize::from(register)),
                (7, 4) => StatusSource::Immediate,
                _ => StatusSource::Memory(MemoryReadProfile::decode(opcode, Size::Word)?),
            },
            to_sr,
        })
    }
}

impl PrefetchCpu {
    pub(super) fn physical_status<B: InstructionBus>(
        &mut self,
        bus: &mut B,
        pipeline: Pipeline,
        profile: StatusProfile,
    ) -> Result<(), TransferExit<B::Error>> {
        let (value, anchor) = match profile.source {
            StatusSource::Dn(register) => (
                self.registers.d[register] & 0xffff,
                pipeline.address.wrapping_add(2),
            ),
            StatusSource::Immediate => {
                let value = u32::from(pipeline.words[1]);
                let address = pipeline.address.wrapping_add(4);
                let phase = self.prefetch_phase(
                    InstructionPhase::ImmediateFirst,
                    address,
                    address,
                    pipeline.words[0],
                );
                self.physical_read(bus, phase)?;
                (value, address)
            }
            StatusSource::Memory(source) => {
                let operand = self.physical_memory_read(bus, pipeline, source)?;
                (operand.value, operand.anchor)
            }
        };
        self.entry_idle(bus, 2)
            .map_err(|e| TransferExit::Fault(InstructionFault::Entry(e)))?;
        let old_sr = self.registers.status_register();
        self.registers.set_status_register(if profile.to_sr {
            value as u16
        } else {
            (old_sr & !CCR_MASK) | (value as u16 & CCR_MASK)
        });
        self.entry_idle(bus, 2)
            .map_err(|e| TransferExit::Fault(InstructionFault::Entry(e)))?;
        let phase = self.prefetch_phase(
            InstructionPhase::StatusFirst,
            anchor,
            anchor,
            pipeline.words[0],
        );
        let first = self.physical_read(bus, phase)?;
        let address = anchor.wrapping_add(2);
        let phase = self.prefetch_phase(InstructionPhase::StatusSecond, address, address, first);
        let second = self.physical_read(bus, phase)?;
        self.registers.pc = anchor;
        self.pipeline = Some(Pipeline {
            address: anchor,
            words: [first, second],
        });
        Ok(())
    }
}
