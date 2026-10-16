use super::*;
use crate::bus::{AccessKind, ByteLanes};
use crate::constants::{BUS_ADDRESS_MASK, NEGATIVE, ZERO};
use crate::ea::Size;

#[derive(Clone, Copy, PartialEq, Eq)]
enum StoreMode {
    Indirect,
    Postincrement,
    Predecrement,
    Displacement,
    Indexed,
    AbsoluteWord,
    AbsoluteLong,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum StoreOrigin {
    Register,
    Memory,
}

#[derive(Clone, Copy)]
pub(super) struct StoreProfile {
    pub size: Size,
    address_register: usize,
    mode: StoreMode,
}

#[derive(Clone, Copy)]
pub(super) struct StoreStream {
    pub anchor: u32,
    pub opcode: u16,
    pub retained: u16,
}

impl StoreStream {
    pub(super) fn from_pipeline(pipeline: Pipeline) -> Self {
        Self {
            anchor: pipeline.address.wrapping_add(2),
            opcode: pipeline.words[0],
            retained: pipeline.words[1],
        }
    }
}

impl StoreProfile {
    pub(super) fn decode_destination(opcode: u16) -> Option<Self> {
        let size = match opcode >> 12 {
            1 => Size::Byte,
            2 => Size::Long,
            3 => Size::Word,
            _ => return None,
        };
        let address_register = usize::from((opcode >> 9) & 7);
        let mode = match ((opcode >> 6) & 7, address_register) {
            (2, _) => StoreMode::Indirect,
            (3, _) => StoreMode::Postincrement,
            (4, _) => StoreMode::Predecrement,
            (5, _) => StoreMode::Displacement,
            (6, _) => StoreMode::Indexed,
            (7, 0) => StoreMode::AbsoluteWord,
            (7, 1) => StoreMode::AbsoluteLong,
            _ => return None,
        };
        Some(Self {
            size,
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

    fn simple(self) -> bool {
        matches!(
            self.mode,
            StoreMode::Indirect | StoreMode::Postincrement | StoreMode::Predecrement
        )
    }

    fn absolute(self) -> bool {
        matches!(self.mode, StoreMode::AbsoluteWord | StoreMode::AbsoluteLong)
    }

    fn operand_pc(self, stream: StoreStream) -> u32 {
        stream
            .anchor
            .wrapping_add(if self.mode == StoreMode::AbsoluteLong {
                4
            } else {
                2
            })
    }

    fn address(self, cpu: &PrefetchCpu, stream: StoreStream, low: u16) -> u32 {
        let extension = stream.retained;
        match self.mode {
            StoreMode::AbsoluteWord => Size::Word.sign_extend(u32::from(extension)),
            StoreMode::AbsoluteLong => (u32::from(extension) << 16) | u32::from(low),
            _ => {
                let base = cpu
                    .registers
                    .address_register(self.address_register)
                    .unwrap();
                if self.mode == StoreMode::Indexed {
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
        stream: StoreStream,
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
                InstructionPhase::OperandWriteLow
            } else {
                InstructionPhase::OperandWriteHigh
            },
            access: BusAccess {
                logical_address: address,
                bus_address: address & BUS_ADDRESS_MASK & !1,
                function_code: crate::ea::data_space(&cpu.registers),
                kind: AccessKind::Data,
                lanes,
            },
            direction: Direction::Write,
            internal_pc: self.operand_pc(stream),
            ird: if self.mode == StoreMode::Predecrement && !low {
                stream.retained
            } else {
                stream.opcode
            },
        }
    }

    fn high_word(self, value: u32, address: u32) -> u16 {
        match self.size {
            Size::Byte if address & 1 == 0 => (value as u16 & 0xff) << 8,
            Size::Byte => value as u16 & 0xff,
            Size::Word => value as u16,
            Size::Long => (value >> 16) as u16,
        }
    }
}

impl PrefetchCpu {
    pub(super) fn physical_store_move<B: InstructionBus>(
        &mut self,
        bus: &mut B,
        stream: StoreStream,
        profile: StoreProfile,
        value: u32,
    ) -> Result<(), TransferExit<B::Error>> {
        if profile.simple() {
            return self.physical_simple_store_move(
                bus,
                stream,
                profile,
                value,
                StoreOrigin::Register,
            );
        }

        if profile.mode == StoreMode::Indexed {
            self.entry_idle(bus, 2)
                .map_err(|e| TransferExit::Fault(InstructionFault::Entry(e)))?;
        }
        self.physical_reserve()?;
        if profile.mode == StoreMode::AbsoluteWord {
            self.extension_store_flags(profile.size, value);
        }
        let phase = self.prefetch_phase(
            InstructionPhase::ExtensionFirst,
            stream.anchor.wrapping_add(2),
            if profile.mode == StoreMode::AbsoluteLong {
                stream.anchor
            } else {
                stream.anchor.wrapping_add(2)
            },
            stream.opcode,
        );
        let first = self.physical_read(bus, phase)?;
        let next_opcode = if profile.mode == StoreMode::AbsoluteLong {
            self.physical_reserve()?;
            self.extension_store_flags(profile.size, value);
            let address = stream.anchor.wrapping_add(4);
            let phase = self.prefetch_phase(
                InstructionPhase::ExtensionSecond,
                address,
                address,
                stream.opcode,
            );
            self.physical_read(bus, phase)?
        } else {
            first
        };
        let address = profile.address(self, stream, first);
        self.physical_extension_store(bus, stream, profile, address, value)?;
        self.physical_reserve()?;
        crate::alu::logic(&mut self.registers, profile.size, value);
        let end = profile.operand_pc(stream);
        let refill_address = end.wrapping_add(2);
        let phase = self.prefetch_phase(
            InstructionPhase::OperandRefill,
            refill_address,
            refill_address,
            next_opcode,
        );
        let refill = self.physical_read(bus, phase)?;
        self.finish_store(end, next_opcode, refill);
        Ok(())
    }

    pub(super) fn physical_memory_store_move<B: InstructionBus>(
        &mut self,
        bus: &mut B,
        stream: StoreStream,
        profile: StoreProfile,
        value: u32,
    ) -> Result<(), TransferExit<B::Error>> {
        if profile.simple() {
            self.physical_simple_store_move(bus, stream, profile, value, StoreOrigin::Memory)
        } else {
            self.physical_memory_extension_store(bus, stream, profile, value)
        }
    }

    fn physical_memory_extension_store<B: InstructionBus>(
        &mut self,
        bus: &mut B,
        stream: StoreStream,
        profile: StoreProfile,
        value: u32,
    ) -> Result<(), TransferExit<B::Error>> {
        if profile.mode == StoreMode::Indexed {
            self.entry_idle(bus, 2)
                .map_err(|e| TransferExit::Fault(InstructionFault::Entry(e)))?;
        }
        self.physical_reserve()?;
        if profile.size != Size::Long || profile.mode != StoreMode::AbsoluteLong {
            self.extension_store_flags(profile.size, value);
        }
        let operand_pc = stream.anchor.wrapping_add(2);
        let phase = self.prefetch_phase(
            InstructionPhase::ExtensionFirst,
            operand_pc,
            operand_pc,
            stream.opcode,
        );
        let first = self.physical_read(bus, phase)?;
        let address = profile.address(self, stream, first);
        self.physical_reserve()?;
        if profile.mode == StoreMode::AbsoluteLong {
            self.extension_store_flags(profile.size, value);
        } else {
            crate::alu::logic(&mut self.registers, profile.size, value);
        }
        let mut phase = profile.operand_phase(self, stream, address, false);
        phase.internal_pc = operand_pc;
        self.physical_write(bus, phase, profile.high_word(value, address))?;
        if profile.size == Size::Long {
            self.physical_reserve()?;
            crate::alu::logic(&mut self.registers, Size::Long, value);
            let mut phase = profile.operand_phase(self, stream, address.wrapping_add(2), true);
            phase.internal_pc = operand_pc;
            self.physical_write(bus, phase, value as u16)?;
        }
        let end = profile.operand_pc(stream);
        let next_opcode = if profile.mode == StoreMode::AbsoluteLong {
            // Memory-source absolute-long stores fetch N only after their writes.
            let phase =
                self.prefetch_phase(InstructionPhase::ExtensionSecond, end, end, stream.opcode);
            self.physical_read(bus, phase)?
        } else {
            first
        };
        let refill_address = end.wrapping_add(2);
        let phase = self.prefetch_phase(
            InstructionPhase::OperandRefill,
            refill_address,
            refill_address,
            next_opcode,
        );
        let refill = self.physical_read(bus, phase)?;
        self.finish_store(end, next_opcode, refill);
        Ok(())
    }

    fn physical_simple_store_move<B: InstructionBus>(
        &mut self,
        bus: &mut B,
        stream: StoreStream,
        profile: StoreProfile,
        value: u32,
        origin: StoreOrigin,
    ) -> Result<(), TransferExit<B::Error>> {
        let address = self
            .registers
            .address_register(profile.address_register)
            .unwrap();
        let refill = if profile.mode == StoreMode::Predecrement {
            self.physical_predecrement_store(bus, stream, profile, address, value)?
        } else {
            self.physical_simple_store(bus, stream, profile, value, origin)?;
            self.physical_reserve()?;
            if profile.mode == StoreMode::Postincrement {
                self.registers.set_address_register(
                    profile.address_register,
                    address.wrapping_add(profile.step()),
                );
            }
            crate::alu::logic(&mut self.registers, profile.size, value);
            let address = stream.anchor.wrapping_add(2);
            let phase = self.prefetch_phase(
                InstructionPhase::OperandRefill,
                address,
                address,
                stream.retained,
            );
            self.physical_read(bus, phase)?
        };
        self.finish_store(stream.anchor, stream.retained, refill);
        Ok(())
    }

    fn finish_store(&mut self, pc: u32, opcode: u16, refill: u16) {
        self.registers.pc = pc;
        self.pipeline = Some(Pipeline {
            address: pc,
            words: [opcode, refill],
        });
    }

    fn physical_simple_store<B: InstructionBus>(
        &mut self,
        bus: &mut B,
        stream: StoreStream,
        profile: StoreProfile,
        value: u32,
        origin: StoreOrigin,
    ) -> Result<(), TransferExit<B::Error>> {
        let address = self
            .registers
            .address_register(profile.address_register)
            .unwrap();
        if profile.size != Size::Long || origin == StoreOrigin::Memory {
            self.physical_reserve()?;
            self.extension_store_flags(profile.size, value);
        }
        let first = profile.operand_phase(self, stream, address, false);
        self.physical_write(bus, first, profile.high_word(value, address))?;
        if profile.size == Size::Long {
            self.physical_reserve()?;
            let size = if origin == StoreOrigin::Memory {
                Size::Long
            } else {
                Size::Word
            };
            crate::alu::logic(&mut self.registers, size, value);
            let second = profile.operand_phase(self, stream, address.wrapping_add(2), true);
            self.physical_write(bus, second, value as u16)?;
        }
        Ok(())
    }

    fn physical_predecrement_store<B: InstructionBus>(
        &mut self,
        bus: &mut B,
        stream: StoreStream,
        profile: StoreProfile,
        address: u32,
        value: u32,
    ) -> Result<u16, TransferExit<B::Error>> {
        self.physical_reserve()?;
        self.extension_store_flags(profile.size, value);
        let refill_address = stream.anchor.wrapping_add(2);
        let phase = self.prefetch_phase(
            InstructionPhase::OperandRefill,
            refill_address,
            refill_address,
            stream.opcode,
        );
        let refill = self.physical_read(bus, phase)?;
        self.physical_reserve()?;
        if profile.size == Size::Long {
            crate::alu::logic(&mut self.registers, Size::Long, value);
            let low = profile.operand_phase(self, stream, address.wrapping_sub(2), true);
            self.physical_write(bus, low, value as u16)?;
            self.physical_reserve()?;
            let updated = address.wrapping_sub(profile.step());
            self.registers
                .set_address_register(profile.address_register, updated);
            let high = profile.operand_phase(self, stream, updated, false);
            self.physical_write(bus, high, (value >> 16) as u16)?;
        } else {
            let updated = address.wrapping_sub(profile.step());
            self.registers
                .set_address_register(profile.address_register, updated);
            let phase = profile.operand_phase(self, stream, updated, false);
            self.physical_write(bus, phase, profile.high_word(value, updated))?;
        }
        Ok(refill)
    }

    fn extension_store_flags(&mut self, size: Size, value: u32) {
        crate::alu::logic(
            &mut self.registers,
            if size == Size::Long { Size::Word } else { size },
            value,
        );
    }

    fn physical_extension_store<B: InstructionBus>(
        &mut self,
        bus: &mut B,
        stream: StoreStream,
        profile: StoreProfile,
        address: u32,
        value: u32,
    ) -> Result<(), TransferExit<B::Error>> {
        self.physical_reserve()?;
        if profile.size == Size::Long && !profile.absolute() {
            // High-half N/Z fold into the old CCR without clearing V/C.
            let old_sr = self.registers.status_register();
            let negative = if value & Size::Long.sign_bit() != 0 {
                NEGATIVE
            } else {
                0
            };
            let zero = if value >> 16 == 0 { old_sr & ZERO } else { 0 };
            self.registers
                .set_status_register((old_sr & !(NEGATIVE | ZERO)) | negative | zero);
        } else {
            crate::alu::logic(&mut self.registers, profile.size, value);
        }
        let phase = profile.operand_phase(self, stream, address, false);
        self.physical_write(bus, phase, profile.high_word(value, address))?;
        if profile.size == Size::Long {
            self.physical_reserve()?;
            crate::alu::logic(
                &mut self.registers,
                if profile.absolute() {
                    Size::Long
                } else {
                    Size::Word
                },
                value,
            );
            let phase = profile.operand_phase(self, stream, address.wrapping_add(2), true);
            self.physical_write(bus, phase, value as u16)?;
        }
        Ok(())
    }
}
