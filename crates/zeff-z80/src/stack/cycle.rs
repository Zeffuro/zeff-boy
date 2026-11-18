use super::*;

impl Stack {
    pub(crate) fn im1_write(&self, cpu: &Cpu) -> Option<(u16, u8, bool)> {
        if self.pair != Pair::ProgramCounter
            || self.fetch != access::Fetch::Memory
            || self.target != Some(Z80_INTERRUPT_VECTOR_IM1)
            || self.address != cpu.regs.sp
        {
            return None;
        }
        let final_write = match self.phase {
            Phase::PushHigh(value) if value == cpu.regs.pc => false,
            Phase::PushLow(value) if value == cpu.regs.pc.to_le_bytes()[0] => true,
            _ => return None,
        };
        let (address, value) = self.write_cycle()?;
        Some((address, value, final_write))
    }

    pub(crate) fn prepares(opcode: u8) -> bool {
        matches!(
            opcode,
            0xc5 | 0xd5
                | 0xe5
                | 0xf5
                | 0xc7
                | 0xcf
                | 0xd7
                | 0xdf
                | 0xe7
                | 0xef
                | 0xf7
                | 0xff
                | 0xc0
                | 0xc8
                | 0xd0
                | 0xd8
                | 0xe0
                | 0xe8
                | 0xf0
                | 0xf8
        )
    }

    pub(crate) fn prepare(mut self, cpu: &mut Cpu) -> Result<Option<Self>, ()> {
        self.phase = match self.phase {
            Phase::ReturnPrepare(false) => return Ok(None),
            Phase::ReturnPrepare(true) => Phase::PopLow,
            Phase::PushPrepare(value) => {
                self.address = self.address.wrapping_sub(1);
                cpu.regs.sp = self.address;
                Phase::PushHigh(value)
            }
            _ => return Err(()),
        };
        Ok(Some(self))
    }

    pub(crate) fn write_cycle(&self) -> Option<(u16, u8)> {
        match self.phase {
            Phase::PushHigh(value) => Some((self.address, value.to_le_bytes()[1])),
            Phase::PushLow(value) => Some((self.address, value)),
            _ => None,
        }
    }

    pub(crate) fn complete_push(mut self, cpu: &mut Cpu) -> Result<Option<Self>, ()> {
        match self.phase {
            Phase::PushHigh(value) => {
                self.address = self.address.wrapping_sub(1);
                cpu.regs.sp = self.address;
                self.phase = Phase::PushLow(value.to_le_bytes()[0]);
                Ok(Some(self))
            }
            Phase::PushLow(_) => {
                if let Some(target) = self.target {
                    cpu.regs.pc = target;
                    cpu.set_memptr(target);
                }
                Ok(None)
            }
            _ => Err(()),
        }
    }

    pub(crate) fn extra_cycle(
        &self,
        cpu: &Cpu,
        opcode: u8,
        index: Option<u8>,
    ) -> Option<CycleKind> {
        if !Self::prepares(opcode)
            || self.fetch != access::Fetch::Memory
            || self.address != cpu.regs.sp
        {
            return None;
        }
        let conditional_return = opcode & 0xc7 == 0xc0;
        if conditional_return {
            return (self.pair == Pair::ProgramCounter
                && self.target.is_none()
                && cpu.condition_is_true((opcode >> 3) & 7)
                && matches!(self.phase, Phase::PopLow | Phase::PopHigh(_)))
            .then_some(CycleKind::MemoryRead {
                address: self.address,
            });
        }
        let pair = match ((opcode >> 4) & 3, index) {
            (2, Some(prefix)) => Pair::Index(prefix == Z80_PREFIX_FD),
            (register, _) => Pair::Register(register),
        };
        let restart = opcode & 0xc7 == 0xc7;
        let target = restart.then_some(u16::from(opcode & 0x38));
        if self.pair != pair || self.target != target {
            return None;
        }
        let word = if restart { cpu.regs.pc } else { pair.read(cpu) };
        let valid = match self.phase {
            Phase::PushHigh(value) => value == word,
            Phase::PushLow(low) => low == word.to_le_bytes()[0],
            _ => false,
        };
        if !valid {
            return None;
        }
        let (address, value) = self.write_cycle()?;
        Some(CycleKind::MemoryWrite { address, value })
    }
}
