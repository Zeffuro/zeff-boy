use super::*;
use crate::bus::{AccessKind, ByteLanes};
use crate::constants::BUS_ADDRESS_MASK;
use crate::ea::Size;

#[derive(Clone, Copy, PartialEq, Eq)]
enum AddressMode {
    Displacement,
    Indexed,
    AbsoluteWord,
    AbsoluteLong,
    PcDisplacement,
    PcIndexed,
    Immediate,
}

impl AddressMode {
    fn decode(mode: u16, register: usize, direction: Direction) -> Option<Self> {
        match (mode, register) {
            (5, _) => Some(Self::Displacement),
            (6, _) => Some(Self::Indexed),
            (7, 0) => Some(Self::AbsoluteWord),
            (7, 1) => Some(Self::AbsoluteLong),
            (7, 2) if direction == Direction::Read => Some(Self::PcDisplacement),
            (7, 3) if direction == Direction::Read => Some(Self::PcIndexed),
            (7, 4) if direction == Direction::Read => Some(Self::Immediate),
            _ => None,
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
pub(super) struct ExtensionMoveProfile {
    size: Size,
    direction: Direction,
    register: MoveRegister,
    address_register: usize,
    mode: AddressMode,
}

impl ExtensionMoveProfile {
    pub(super) fn decode(opcode: u16) -> Option<Self> {
        let size = match opcode >> 12 {
            1 => Size::Byte,
            2 => Size::Long,
            3 => Size::Word,
            _ => return None,
        };
        let source = usize::from(opcode & 7);
        let destination = usize::from((opcode >> 9) & 7);
        let destination_mode = (opcode >> 6) & 7;
        if destination_mode == 1 && size == Size::Byte {
            return None;
        }
        let (direction, data_register, address_register, mode) =
            match ((opcode >> 3) & 7, destination_mode) {
                (mode, 0 | 1) => (Direction::Read, destination, source, mode),
                (0 | 1, mode) if (opcode >> 3) & 7 == 0 || size != Size::Byte => {
                    (Direction::Write, source, destination, mode)
                }
                _ => return None,
            };
        Some(Self {
            size,
            direction,
            register: if direction == Direction::Read && destination_mode == 1 {
                MoveRegister::Address(destination)
            } else if direction == Direction::Write && (opcode >> 3) & 7 == 1 {
                MoveRegister::Address(source)
            } else {
                MoveRegister::Data(data_register)
            },
            address_register,
            mode: AddressMode::decode(mode, address_register, direction)?,
        })
    }

    fn two_extensions(self) -> bool {
        self.mode == AddressMode::AbsoluteLong
            || (self.mode == AddressMode::Immediate && self.size == Size::Long)
    }

    fn first_extension_pc(self, pc: u32) -> u32 {
        let offset = if self.two_extensions()
            || (self.direction == Direction::Read
                && matches!(
                    self.mode,
                    AddressMode::Displacement
                        | AddressMode::Indexed
                        | AddressMode::PcDisplacement
                        | AddressMode::PcIndexed
                )) {
            2
        } else {
            4
        };
        pc.wrapping_add(offset)
    }

    fn operand_pc(self, pc: u32) -> u32 {
        if self.mode == AddressMode::AbsoluteLong {
            pc.wrapping_add(6)
        } else {
            self.first_extension_pc(pc)
        }
    }

    fn address(self, cpu: &PrefetchCpu, pipeline: Pipeline, low: u16) -> u32 {
        let extension = pipeline.words[1];
        match self.mode {
            AddressMode::AbsoluteWord => Size::Word.sign_extend(u32::from(extension)),
            AddressMode::AbsoluteLong => (u32::from(extension) << 16) | u32::from(low),
            AddressMode::Immediate => unreachable!(),
            _ => {
                let base = if self.mode.pc_relative() {
                    pipeline.address.wrapping_add(2)
                } else {
                    cpu.registers
                        .address_register(self.address_register)
                        .unwrap()
                };
                if self.mode.indexed() {
                    let index = usize::from((extension >> 12) & 7);
                    let value = if extension & 0x8000 == 0 {
                        cpu.registers.d[index]
                    } else {
                        cpu.registers.address_register(index).unwrap()
                    };
                    let value = if extension & 0x0800 == 0 {
                        Size::Word.sign_extend(value)
                    } else {
                        value
                    };
                    base.wrapping_add(value)
                        .wrapping_add_signed(i32::from(extension as u8 as i8))
                } else {
                    base.wrapping_add_signed(i32::from(extension as i16))
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
        let phase = match (self.direction, low) {
            (Direction::Read, false) => InstructionPhase::OperandReadHigh,
            (Direction::Read, true) => InstructionPhase::OperandReadLow,
            (Direction::Write, false) => InstructionPhase::OperandWriteHigh,
            (Direction::Write, true) => InstructionPhase::OperandWriteLow,
        };
        let lanes = if self.size != Size::Byte {
            ByteLanes::Both
        } else if address & 1 == 0 {
            ByteLanes::Upper
        } else {
            ByteLanes::Lower
        };
        TransferPhase {
            phase,
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
            direction: self.direction,
            internal_pc: self.operand_pc(pipeline.address),
            ird: pipeline.words[0],
        }
    }
}

impl PrefetchCpu {
    pub(super) fn physical_extension_move<B: InstructionBus>(
        &mut self,
        bus: &mut B,
        pipeline: Pipeline,
        profile: ExtensionMoveProfile,
    ) -> Result<(), TransferExit<B::Error>> {
        if profile.direction == Direction::Write {
            let value = profile.register.read(&self.registers);
            let destination = stores::StoreProfile::decode_destination(pipeline.words[0]).unwrap();
            return self.physical_store_move(
                bus,
                stores::StoreStream::from_pipeline(pipeline),
                destination,
                value,
            );
        }
        if profile.mode.indexed() {
            self.entry_idle(bus, 2)
                .map_err(|e| TransferExit::Fault(InstructionFault::Entry(e)))?;
        }
        self.physical_reserve()?;
        let phase = self.prefetch_phase(
            InstructionPhase::ExtensionFirst,
            pipeline.address.wrapping_add(4),
            profile.first_extension_pc(pipeline.address),
            pipeline.words[0],
        );
        let first = self.physical_read(bus, phase)?;
        let next_opcode = if profile.two_extensions() {
            self.physical_reserve()?;
            let phase = self.prefetch_phase(
                InstructionPhase::ExtensionSecond,
                pipeline.address.wrapping_add(6),
                pipeline.address.wrapping_add(6),
                pipeline.words[0],
            );
            self.physical_read(bus, phase)?
        } else {
            first
        };
        let value = if profile.mode == AddressMode::Immediate {
            if profile.size == Size::Long {
                (u32::from(pipeline.words[1]) << 16) | u32::from(first)
            } else {
                profile.size.truncate(u32::from(pipeline.words[1]))
            }
        } else {
            let address = profile.address(self, pipeline, first);
            self.physical_extension_load(bus, pipeline, profile, address)?
        };
        self.physical_reserve()?;
        profile
            .register
            .publish(&mut self.registers, profile.size, value);
        let length = if profile.two_extensions() { 6 } else { 4 };
        let refill_address = pipeline.address.wrapping_add(length + 2);
        let phase = self.prefetch_phase(
            InstructionPhase::OperandRefill,
            refill_address,
            refill_address,
            next_opcode,
        );
        let refill = self.physical_read(bus, phase)?;
        self.registers.pc = pipeline.address.wrapping_add(length);
        self.pipeline = Some(Pipeline {
            address: self.registers.pc,
            words: [next_opcode, refill],
        });
        Ok(())
    }

    fn physical_extension_load<B: InstructionBus>(
        &mut self,
        bus: &mut B,
        pipeline: Pipeline,
        profile: ExtensionMoveProfile,
        address: u32,
    ) -> Result<u32, TransferExit<B::Error>> {
        let phase = profile.operand_phase(self, pipeline, address, false);
        let high = self.physical_read(bus, phase)?;
        Ok(match profile.size {
            Size::Byte if address & 1 == 0 => u32::from(high >> 8),
            Size::Byte => u32::from(high & 0xff),
            Size::Word => u32::from(high),
            Size::Long => {
                let phase = profile.operand_phase(self, pipeline, address.wrapping_add(2), true);
                let low = self.physical_read(bus, phase)?;
                (u32::from(high) << 16) | u32::from(low)
            }
        })
    }
}
