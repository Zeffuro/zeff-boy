use super::*;
use crate::ea::Size;
use memory_reads::MemoryReadProfile;

pub(super) fn legal(opcode: u16) -> bool {
    let opmode = opcode >> 6 & 7;
    let mode = opcode >> 3 & 7;
    opcode & 0xf000 == 0xb000
        && opmode <= 2
        && !(opmode == 0 && mode == 1)
        && !(mode == 7 && opcode & 7 > 4)
}

pub(super) fn legal_address(opcode: u16) -> bool {
    opcode & 0xf000 == 0xb000
        && matches!(opcode >> 6 & 7, 3 | 7)
        && !(opcode >> 3 & 7 == 7 && opcode & 7 > 4)
}

#[derive(Clone, Copy)]
enum Source {
    Data(usize),
    Address(usize),
    Immediate,
    Memory(MemoryReadProfile),
}

#[derive(Clone, Copy)]
enum Destination {
    Data(usize),
    Address(usize),
}

#[derive(Clone, Copy)]
pub(super) struct CmpProfile {
    size: Size,
    source: Source,
    destination: Destination,
}

impl CmpProfile {
    pub(super) fn decode(opcode: u16) -> Option<Self> {
        if !legal(opcode) && !legal_address(opcode) {
            return None;
        }
        let opmode = opcode >> 6 & 7;
        let size = match opmode {
            0 => Size::Byte,
            1 | 3 => Size::Word,
            2 | 7 => Size::Long,
            _ => return None,
        };
        let source_register = usize::from(opcode & 7);
        let source = match (opcode >> 3 & 7, source_register) {
            (0, _) => Source::Data(source_register),
            (1, _) => Source::Address(source_register),
            (7, 4) => Source::Immediate,
            _ => Source::Memory(MemoryReadProfile::decode(opcode, size)?),
        };
        let register = usize::from(opcode >> 9 & 7);
        Some(Self {
            size,
            source,
            destination: if matches!(opmode, 3 | 7) {
                Destination::Address(register)
            } else {
                Destination::Data(register)
            },
        })
    }
}

impl PrefetchCpu {
    fn physical_cmp_immediate<B: InstructionBus>(
        &mut self,
        bus: &mut B,
        pipeline: Pipeline,
        size: Size,
    ) -> Result<(u32, u32, u16), TransferExit<B::Error>> {
        let pc = pipeline.address;
        let opcode = pipeline.words[0];
        let phase = self.prefetch_phase(
            InstructionPhase::ImmediateFirst,
            pc.wrapping_add(4),
            pc.wrapping_add(if size == Size::Long { 2 } else { 4 }),
            opcode,
        );
        let first = self.physical_read(bus, phase)?;
        if size == Size::Long {
            let phase = self.prefetch_phase(
                InstructionPhase::ImmediateSecond,
                pc.wrapping_add(6),
                pc.wrapping_add(6),
                opcode,
            );
            let retained = self.physical_read(bus, phase)?;
            Ok((
                (u32::from(pipeline.words[1]) << 16) | u32::from(first),
                pc.wrapping_add(6),
                retained,
            ))
        } else {
            Ok((u32::from(pipeline.words[1]), pc.wrapping_add(4), first))
        }
    }

    pub(super) fn physical_cmp<B: InstructionBus>(
        &mut self,
        bus: &mut B,
        pipeline: Pipeline,
        profile: CmpProfile,
    ) -> Result<(), TransferExit<B::Error>> {
        let (source, anchor, retained) = match profile.source {
            Source::Data(register) => (
                self.registers.d[register],
                pipeline.address.wrapping_add(2),
                pipeline.words[1],
            ),
            Source::Address(register) => (
                self.registers.address_register(register).unwrap(),
                pipeline.address.wrapping_add(2),
                pipeline.words[1],
            ),
            Source::Immediate => self.physical_cmp_immediate(bus, pipeline, profile.size)?,
            Source::Memory(memory) => {
                let operand = self.physical_memory_read(bus, pipeline, memory)?;
                (operand.value, operand.anchor, operand.retained)
            }
        };
        self.physical_reserve()?;
        let address_destination = matches!(profile.destination, Destination::Address(_));
        let tail = address_destination || profile.size == Size::Long;
        let (size, destination, value) = match profile.destination {
            Destination::Data(register) => (profile.size, self.registers.d[register], source),
            Destination::Address(register) => (
                Size::Long,
                self.registers.address_register(register).unwrap(),
                profile.size.sign_extend(source),
            ),
        };
        crate::alu::subtract(&mut self.registers, size, destination, value, false);
        let address = anchor.wrapping_add(2);
        let phase = self.prefetch_phase(
            InstructionPhase::OperandRefill,
            address,
            address,
            if tail { pipeline.words[0] } else { retained },
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
