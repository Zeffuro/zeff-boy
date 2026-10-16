use super::*;
use crate::ea::Size;
use memory_reads::MemoryReadProfile;

pub(super) fn legal(opcode: u16) -> bool {
    let mode = opcode >> 3 & 7;
    opcode & 0xff00 == 0x4a00
        && opcode >> 6 & 3 != 3
        && (mode == 0 || matches!(mode, 2..=6) || mode == 7 && opcode & 7 <= 1)
}

#[derive(Clone, Copy)]
enum TstSource {
    Dn(usize),
    Memory(MemoryReadProfile),
}

#[derive(Clone, Copy)]
pub(super) struct TstProfile {
    size: Size,
    source: TstSource,
}

impl TstProfile {
    pub(super) fn decode(opcode: u16) -> Option<Self> {
        if !legal(opcode) {
            return None;
        }
        let size = match opcode >> 6 & 3 {
            0 => Size::Byte,
            1 => Size::Word,
            2 => Size::Long,
            _ => return None,
        };
        Some(Self {
            size,
            source: if opcode >> 3 & 7 == 0 {
                TstSource::Dn(usize::from(opcode & 7))
            } else {
                TstSource::Memory(MemoryReadProfile::decode(opcode, size)?)
            },
        })
    }
}

impl PrefetchCpu {
    pub(super) fn physical_tst<B: InstructionBus>(
        &mut self,
        bus: &mut B,
        pipeline: Pipeline,
        profile: TstProfile,
    ) -> Result<(), TransferExit<B::Error>> {
        let (value, anchor, retained) = match profile.source {
            TstSource::Dn(register) => (
                self.registers.d[register],
                pipeline.address.wrapping_add(2),
                pipeline.words[1],
            ),
            TstSource::Memory(source) => {
                let operand = self.physical_memory_read(bus, pipeline, source)?;
                (operand.value, operand.anchor, operand.retained)
            }
        };
        self.physical_reserve()?;
        crate::alu::logic(&mut self.registers, profile.size, value);
        let address = anchor.wrapping_add(2);
        let phase =
            self.prefetch_phase(InstructionPhase::OperandRefill, address, address, retained);
        let fetched = self.physical_read(bus, phase)?;
        self.registers.pc = anchor;
        self.pipeline = Some(Pipeline {
            address: anchor,
            words: [retained, fetched],
        });
        Ok(())
    }
}
