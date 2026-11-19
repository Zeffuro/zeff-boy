use super::*;

impl Block {
    pub(crate) fn copy_kind(self, cpu: &Cpu) -> Result<CopyCycleKind, CycleError> {
        if !matches!(self.opcode, 0xa0 | 0xa8 | 0xb0 | 0xb8)
            || self.pc != cpu.last_opcode_pc
            || cpu.last_opcode != Z80_PREFIX_ED
            || cpu.instruction_byte_count != 2
            || cpu.last_m1_fetch_count != 2
            || cpu.instruction_bytes[0] != Z80_PREFIX_ED
            || cpu.instruction_bytes[1] != self.opcode
            || cpu.instruction_bytes[2..].iter().any(|v| *v != 0)
            || cpu.regs.pc != self.pc.wrapping_add(2)
        {
            return Err(CycleError::UnsupportedCycle);
        }
        let delta = if self.opcode & 8 == 0 { 1 } else { -1 };
        match self.phase {
            BlockPhase::Read | BlockPhase::Write(_) => {
                if cpu.regs.hl() != self.address
                    || cpu.regs.de() != self.destination
                    || cpu.regs.c != self.port
                {
                    return Err(CycleError::UnsupportedCycle);
                }
                Ok(match self.phase {
                    BlockPhase::Read => CopyCycleKind::Read {
                        address: self.address,
                    },
                    BlockPhase::Write(value) => CopyCycleKind::Write {
                        address: self.destination,
                        value,
                    },
                    _ => unreachable!(),
                })
            }
            BlockPhase::Repeat(value) => {
                let retained = Z80_FLAG_SIGN | Z80_FLAG_ZERO | Z80_FLAG_CARRY;
                if self.opcode & 0x10 == 0
                    || cpu.regs.bc() == 0
                    || cpu.regs.hl() != self.address.wrapping_add_signed(delta)
                    || cpu.regs.de() != self.destination.wrapping_add_signed(delta)
                    || cpu.regs.c.wrapping_add(1) != self.port
                    || cpu.regs.f & !retained
                        != (block_xy(cpu.regs.a.wrapping_add(value)) | Z80_FLAG_PARITY_OVERFLOW)
                {
                    return Err(CycleError::UnsupportedCycle);
                }
                Ok(CopyCycleKind::Repeat {
                    retained_address: self.destination,
                })
            }
            _ => Err(CycleError::UnsupportedCycle),
        }
    }
    pub(crate) fn copy_byte(self) -> Option<u8> {
        match self.phase {
            BlockPhase::Write(value) | BlockPhase::Repeat(value) => Some(value),
            _ => None,
        }
    }
    pub(crate) fn copy_finishes(self, cpu: &Cpu) -> bool {
        matches!(self.phase, BlockPhase::Repeat(_))
            || matches!(self.phase, BlockPhase::Write(_))
                && (self.opcode & 0x10 == 0 || cpu.regs.bc() == 1)
    }
    pub(crate) fn complete_copy(
        self,
        cpu: &mut Cpu,
        receipt: CopyReceipt,
    ) -> Result<Option<Self>, CycleError> {
        match (self.phase, receipt) {
            (BlockPhase::Read, CopyReceipt::Read { value, known: 255 }) => Ok(Some(Self {
                phase: BlockPhase::Write(value),
                ..self
            })),
            (BlockPhase::Write(value), CopyReceipt::Write) => Ok(self.finish(cpu, value)),
            (BlockPhase::Repeat(value), CopyReceipt::Repeat) => {
                self.repeat(cpu, value);
                Ok(None)
            }
            _ => Err(CycleError::InvalidCompletion),
        }
    }
}
