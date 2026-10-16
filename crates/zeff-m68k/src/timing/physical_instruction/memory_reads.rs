use super::*;
use crate::bus::{AccessKind, ByteLanes};
use crate::constants::BUS_ADDRESS_MASK;
use crate::ea::Size;

#[derive(Clone, Copy, PartialEq, Eq)]
enum SourceMode {
    Indirect,
    Postincrement,
    Predecrement,
    Displacement,
    Indexed,
    AbsoluteWord,
    AbsoluteLong,
    PcDisplacement,
    PcIndexed,
}

impl SourceMode {
    fn decode(mode: u16, register: usize) -> Option<Self> {
        Some(match (mode, register) {
            (2, _) => Self::Indirect,
            (3, _) => Self::Postincrement,
            (4, _) => Self::Predecrement,
            (5, _) => Self::Displacement,
            (6, _) => Self::Indexed,
            (7, 0) => Self::AbsoluteWord,
            (7, 1) => Self::AbsoluteLong,
            (7, 2) => Self::PcDisplacement,
            (7, 3) => Self::PcIndexed,
            _ => return None,
        })
    }

    fn extensions(self) -> u32 {
        match self {
            Self::Indirect | Self::Postincrement | Self::Predecrement => 0,
            Self::AbsoluteLong => 2,
            _ => 1,
        }
    }

    fn indexed(self) -> bool {
        matches!(self, Self::Indexed | Self::PcIndexed)
    }

    fn pc_relative(self) -> bool {
        matches!(self, Self::PcDisplacement | Self::PcIndexed)
    }
}

#[derive(Clone, Copy)]
pub(super) struct MemoryReadProfile {
    pub(super) size: Size,
    address_register: usize,
    mode: SourceMode,
}

impl MemoryReadProfile {
    pub(super) fn decode(opcode: u16, size: Size) -> Option<Self> {
        let address_register = usize::from(opcode & 7);
        Some(Self {
            size,
            address_register,
            mode: SourceMode::decode((opcode >> 3) & 7, address_register)?,
        })
    }

    fn step(self) -> u32 {
        if self.size == Size::Byte && self.address_register == 7 {
            2
        } else {
            self.size.bytes()
        }
    }

    fn operand_pc(self, pc: u32) -> u32 {
        pc.wrapping_add(match self.mode {
            SourceMode::AbsoluteLong => 6,
            SourceMode::AbsoluteWord => 4,
            SourceMode::Predecrement if self.size != Size::Long => 4,
            _ => 2,
        })
    }

    fn address(self, cpu: &PrefetchCpu, pipeline: Pipeline, low: u16) -> u32 {
        let extension = pipeline.words[1];
        match self.mode {
            SourceMode::AbsoluteWord => Size::Word.sign_extend(u32::from(extension)),
            SourceMode::AbsoluteLong => (u32::from(extension) << 16) | u32::from(low),
            _ => {
                let base = if self.mode.pc_relative() {
                    pipeline.address.wrapping_add(2)
                } else {
                    cpu.registers
                        .address_register(self.address_register)
                        .unwrap()
                };
                match self.mode {
                    SourceMode::Displacement | SourceMode::PcDisplacement => {
                        base.wrapping_add_signed(i32::from(extension as i16))
                    }
                    SourceMode::Indexed | SourceMode::PcIndexed => {
                        let register = usize::from((extension >> 12) & 7);
                        let index = if extension & 0x8000 == 0 {
                            cpu.registers.d[register]
                        } else {
                            cpu.registers.address_register(register).unwrap()
                        };
                        let index = if extension & 0x0800 == 0 {
                            Size::Word.sign_extend(index)
                        } else {
                            index
                        };
                        base.wrapping_add(index)
                            .wrapping_add_signed(i32::from(extension as u8 as i8))
                    }
                    _ => base,
                }
            }
        }
    }

    fn operand_phase(
        self,
        cpu: &PrefetchCpu,
        pipeline: Pipeline,
        address: u32,
        low: bool,
    ) -> TransferPhase {
        let lanes = if self.size != Size::Byte {
            ByteLanes::Both
        } else if address & 1 == 0 {
            ByteLanes::Upper
        } else {
            ByteLanes::Lower
        };
        TransferPhase {
            phase: if low {
                InstructionPhase::OperandReadLow
            } else {
                InstructionPhase::OperandReadHigh
            },
            access: BusAccess {
                logical_address: address,
                bus_address: address & BUS_ADDRESS_MASK & !1,
                function_code: if self.mode.pc_relative() {
                    crate::ea::program_space(&cpu.registers)
                } else {
                    crate::ea::data_space(&cpu.registers)
                },
                kind: AccessKind::Data,
                lanes,
            },
            direction: Direction::Read,
            internal_pc: self.operand_pc(pipeline.address),
            ird: pipeline.words[0],
        }
    }
}

pub(super) struct MemoryOperand {
    pub(super) value: u32,
    pub(super) address: u32,
    pub(super) anchor: u32,
    pub(super) retained: u16,
}

impl PrefetchCpu {
    pub(super) fn physical_memory_read<B: InstructionBus>(
        &mut self,
        bus: &mut B,
        pipeline: Pipeline,
        profile: MemoryReadProfile,
    ) -> Result<MemoryOperand, TransferExit<B::Error>> {
        let mut address = profile.address(self, pipeline, 0);
        if profile.mode.indexed() || profile.mode == SourceMode::Predecrement {
            self.entry_idle(bus, 2)
                .map_err(|e| TransferExit::Fault(InstructionFault::Entry(e)))?;
        }
        let mut operand = MemoryOperand {
            value: 0,
            address: 0,
            anchor: pipeline.address.wrapping_add(2),
            retained: pipeline.words[1],
        };
        if profile.mode.extensions() != 0 {
            let phase = self.prefetch_phase(
                InstructionPhase::ExtensionFirst,
                pipeline.address.wrapping_add(4),
                pipeline
                    .address
                    .wrapping_add(if profile.mode == SourceMode::AbsoluteWord {
                        4
                    } else {
                        2
                    }),
                pipeline.words[0],
            );
            let first = self.physical_read(bus, phase)?;
            operand.retained = first;
            if profile.mode == SourceMode::AbsoluteLong {
                address = profile.address(self, pipeline, first);
                let next_address = pipeline.address.wrapping_add(6);
                let phase = self.prefetch_phase(
                    InstructionPhase::ExtensionSecond,
                    next_address,
                    next_address,
                    pipeline.words[0],
                );
                operand.retained = self.physical_read(bus, phase)?;
            }
            operand.anchor = operand.anchor.wrapping_add(2 * profile.mode.extensions());
        }
        (operand.value, operand.address) =
            self.physical_memory_source(bus, pipeline, profile, address)?;
        Ok(operand)
    }

    fn physical_memory_source<B: InstructionBus>(
        &mut self,
        bus: &mut B,
        pipeline: Pipeline,
        profile: MemoryReadProfile,
        address: u32,
    ) -> Result<(u32, u32), TransferExit<B::Error>> {
        let size = profile.size;
        let first_address = if profile.mode == SourceMode::Predecrement {
            self.physical_reserve()?;
            let updated = address.wrapping_sub(profile.step());
            self.registers
                .set_address_register(profile.address_register, updated);
            updated
        } else {
            if profile.mode == SourceMode::Postincrement && size != Size::Long {
                self.physical_reserve()?;
                self.registers.set_address_register(
                    profile.address_register,
                    address.wrapping_add(profile.step()),
                );
            }
            address
        };
        let phase = profile.operand_phase(self, pipeline, first_address, false);
        let high = self.physical_read(bus, phase)?;
        let value = match size {
            Size::Byte if first_address & 1 == 0 => u32::from(high >> 8),
            Size::Byte => u32::from(high & 0xff),
            Size::Word => u32::from(high),
            Size::Long => {
                if profile.mode == SourceMode::Postincrement {
                    self.physical_reserve()?;
                    self.registers.set_address_register(
                        profile.address_register,
                        address.wrapping_add(profile.step()),
                    );
                }
                let phase =
                    profile.operand_phase(self, pipeline, first_address.wrapping_add(2), true);
                let low = self.physical_read(bus, phase)?;
                (u32::from(high) << 16) | u32::from(low)
            }
        };
        Ok((value, first_address))
    }
}
