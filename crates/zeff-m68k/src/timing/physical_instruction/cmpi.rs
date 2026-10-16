use super::*;
use crate::ea::Size;
use memory_reads::MemoryReadProfile;

pub(super) fn legal(opcode: u16) -> bool {
    let mode = opcode >> 3 & 7;
    opcode & 0xff00 == 0x0c00
        && opcode >> 6 & 3 < 3
        && (mode == 0 || matches!(mode, 2..=6) || mode == 7 && opcode & 7 <= 1)
}

#[derive(Clone, Copy)]
enum CmpiSource {
    Dn(usize),
    Memory(MemoryReadProfile),
}

#[derive(Clone, Copy)]
pub(super) struct CmpiProfile {
    size: Size,
    source: CmpiSource,
}

impl CmpiProfile {
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
                CmpiSource::Dn(usize::from(opcode & 7))
            } else {
                CmpiSource::Memory(MemoryReadProfile::decode(opcode, size)?)
            },
        })
    }
}

impl PrefetchCpu {
    pub(super) fn physical_cmpi<B: InstructionBus>(
        &mut self,
        bus: &mut B,
        pipeline: Pipeline,
        profile: CmpiProfile,
    ) -> Result<(), TransferExit<B::Error>> {
        let size = profile.size;
        let pc = pipeline.address;
        let opcode = pipeline.words[0];
        let phase = self.prefetch_phase(
            InstructionPhase::ImmediateFirst,
            pc.wrapping_add(4),
            pc.wrapping_add(if size == Size::Long { 2 } else { 4 }),
            opcode,
        );
        let first = self.physical_read(bus, phase)?;
        let (immediate, next) = if size == Size::Long {
            let phase = self.prefetch_phase(
                InstructionPhase::ImmediateSecond,
                pc.wrapping_add(6),
                pc.wrapping_add(6),
                opcode,
            );
            let next = self.physical_read(bus, phase)?;
            (
                (u32::from(pipeline.words[1]) << 16) | u32::from(first),
                next,
            )
        } else {
            (u32::from(pipeline.words[1]), first)
        };
        let operand_pipeline = Pipeline {
            address: pc.wrapping_add(if size == Size::Long { 4 } else { 2 }),
            words: [opcode, next],
        };
        let (value, anchor, retained) = match profile.source {
            CmpiSource::Dn(register) => (
                self.registers.d[register],
                operand_pipeline.address.wrapping_add(2),
                next,
            ),
            CmpiSource::Memory(source) => {
                let operand = self.physical_memory_read(bus, operand_pipeline, source)?;
                (operand.value, operand.anchor, operand.retained)
            }
        };
        let tail = size == Size::Long && matches!(profile.source, CmpiSource::Dn(_));
        self.physical_reserve()?;
        crate::alu::subtract(&mut self.registers, size, value, immediate, false);
        let address = anchor.wrapping_add(2);
        let phase = self.prefetch_phase(
            InstructionPhase::OperandRefill,
            address,
            address,
            if tail { opcode } else { retained },
        );
        let fetched = self.physical_read(bus, phase)?;
        if tail {
            self.entry_idle(bus, 2)
                .map_err(|e| TransferExit::Fault(InstructionFault::Entry(e)))?;
        }
        self.registers.pc = anchor;
        self.pipeline = Some(Pipeline {
            address: anchor,
            words: [retained, fetched],
        });
        Ok(())
    }
}
