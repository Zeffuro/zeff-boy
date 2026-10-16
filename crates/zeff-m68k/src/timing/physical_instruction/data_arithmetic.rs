use super::*;
use crate::ea::Size;
use memory_reads::MemoryReadProfile;

pub(super) fn legal(opcode: u16) -> bool {
    let family = opcode >> 12;
    let size = opcode >> 6 & 7;
    let mode = opcode >> 3 & 7;
    matches!(family, 8 | 9 | 12 | 13)
        && size <= 2
        && !(mode == 7 && opcode & 7 > 4)
        && !(mode == 1 && (size == 0 || matches!(family, 8 | 12)))
}

#[derive(Clone, Copy)]
pub(super) struct DataArithmeticProfile {
    source: MemoryReadProfile,
    destination: usize,
    family: u16,
}

impl DataArithmeticProfile {
    pub(super) fn decode(opcode: u16) -> Option<Self> {
        if !legal(opcode) {
            return None;
        }
        let size = match opcode >> 6 & 7 {
            1 => Size::Word,
            2 => Size::Long,
            _ => return None,
        };
        Some(Self {
            source: MemoryReadProfile::decode(opcode, size)?,
            destination: usize::from(opcode >> 9 & 7),
            family: opcode >> 12,
        })
    }
}

impl PrefetchCpu {
    pub(super) fn physical_data_arithmetic<B: InstructionBus>(
        &mut self,
        bus: &mut B,
        pipeline: Pipeline,
        profile: DataArithmeticProfile,
    ) -> Result<(), TransferExit<B::Error>> {
        let operand = self.physical_memory_read(bus, pipeline, profile.source)?;
        self.physical_reserve()?;
        let old = self.registers.d[profile.destination];
        let size = profile.source.size;
        let result = match profile.family {
            9 => crate::alu::subtract(&mut self.registers, size, old, operand.value, true),
            13 => crate::alu::add(&mut self.registers, size, old, operand.value),
            8 | 12 => {
                let value = if profile.family == 8 {
                    old | operand.value
                } else {
                    old & operand.value
                };
                crate::alu::logic(&mut self.registers, size, value);
                size.truncate(value)
            }
            _ => unreachable!(),
        };
        self.registers.d[profile.destination] = (old & 0xffff_0000) | (result & 0xffff);
        let address = operand.anchor.wrapping_add(2);
        let phase = self.prefetch_phase(
            InstructionPhase::OperandRefill,
            address,
            address,
            if size == Size::Long {
                pipeline.words[0]
            } else {
                operand.retained
            },
        );
        let fetched = self.physical_read(bus, phase)?;
        if size == Size::Long {
            self.registers.d[profile.destination] = result;
            self.entry_idle(bus, 2)
                .map_err(|e| TransferExit::Fault(InstructionFault::Entry(e)))?;
        }
        self.registers.pc = operand.anchor;
        self.pipeline = Some(Pipeline {
            address: operand.anchor,
            words: [operand.retained, fetched],
        });
        Ok(())
    }
}
