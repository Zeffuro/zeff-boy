use super::*;
use crate::bus::{AccessKind, ByteLanes};
use crate::constants::BUS_ADDRESS_MASK;
use crate::ea::Size;

#[derive(Clone, Copy, PartialEq, Eq)]
enum AddressMode {
    Indirect,
    Postincrement,
    Predecrement,
}

impl AddressMode {
    fn decode(mode: u16) -> Option<Self> {
        match mode {
            2 => Some(Self::Indirect),
            3 => Some(Self::Postincrement),
            4 => Some(Self::Predecrement),
            _ => None,
        }
    }
}

#[derive(Clone, Copy)]
pub(super) struct MoveProfile {
    size: Size,
    direction: Direction,
    register: MoveRegister,
    address_register: usize,
    mode: AddressMode,
}

impl MoveProfile {
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
                (mode, 0 | 1) => (
                    Direction::Read,
                    destination,
                    source,
                    AddressMode::decode(mode)?,
                ),
                (0 | 1, mode) if (opcode >> 3) & 7 == 0 || size != Size::Byte => (
                    Direction::Write,
                    source,
                    destination,
                    AddressMode::decode(mode)?,
                ),
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
            mode,
        })
    }

    fn step(self) -> u32 {
        if self.size == Size::Byte && self.address_register == 7 {
            2
        } else {
            self.size.bytes()
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
                function_code: crate::ea::data_space(&cpu.registers),
                kind: AccessKind::Data,
                lanes,
            },
            direction: self.direction,
            internal_pc: pipeline.address.wrapping_add(
                if self.direction == Direction::Read
                    && !(self.mode == AddressMode::Predecrement && self.size != Size::Long)
                {
                    2
                } else {
                    4
                },
            ),
            ird: if self.direction == Direction::Write
                && self.mode == AddressMode::Predecrement
                && !low
            {
                pipeline.words[1]
            } else {
                pipeline.words[0]
            },
        }
    }
}

impl PrefetchCpu {
    pub(super) fn physical_move<B: InstructionBus>(
        &mut self,
        bus: &mut B,
        pipeline: Pipeline,
        profile: MoveProfile,
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
        let address = self
            .registers
            .address_register(profile.address_register)
            .unwrap();
        let value = self.physical_move_load(bus, pipeline, profile, address)?;
        self.physical_reserve()?;
        profile
            .register
            .publish(&mut self.registers, profile.size, value);
        let phase = self.prefetch_phase(
            InstructionPhase::OperandRefill,
            pipeline.address.wrapping_add(4),
            pipeline.address.wrapping_add(4),
            pipeline.words[1],
        );
        let next = self.physical_read(bus, phase)?;
        self.registers.pc = pipeline.address.wrapping_add(2);
        self.pipeline = Some(Pipeline {
            address: self.registers.pc,
            words: [pipeline.words[1], next],
        });
        Ok(())
    }

    fn physical_move_load<B: InstructionBus>(
        &mut self,
        bus: &mut B,
        pipeline: Pipeline,
        profile: MoveProfile,
        address: u32,
    ) -> Result<u32, TransferExit<B::Error>> {
        let first_address = if profile.mode == AddressMode::Predecrement {
            self.entry_idle(bus, 2)
                .map_err(|e| TransferExit::Fault(InstructionFault::Entry(e)))?;
            self.physical_reserve()?;
            let updated = address.wrapping_sub(profile.step());
            self.registers
                .set_address_register(profile.address_register, updated);
            updated
        } else {
            if profile.mode == AddressMode::Postincrement && profile.size != Size::Long {
                self.physical_reserve()?;
                self.registers.set_address_register(
                    profile.address_register,
                    address.wrapping_add(profile.step()),
                );
            }
            address
        };
        let first = profile.operand_phase(self, pipeline, first_address, false);
        let high = self.physical_read(bus, first)?;
        Ok(match profile.size {
            Size::Byte if first_address & 1 == 0 => u32::from(high >> 8),
            Size::Byte => u32::from(high & 0xff),
            Size::Word => u32::from(high),
            Size::Long => {
                if profile.mode == AddressMode::Postincrement {
                    self.physical_reserve()?;
                    self.registers.set_address_register(
                        profile.address_register,
                        address.wrapping_add(profile.step()),
                    );
                }
                let second =
                    profile.operand_phase(self, pipeline, first_address.wrapping_add(2), true);
                let low = self.physical_read(bus, second)?;
                (u32::from(high) << 16) | u32::from(low)
            }
        })
    }
}
