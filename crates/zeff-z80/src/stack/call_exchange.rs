use super::*;

impl Stack {
    pub(crate) fn is_call(opcode: u8) -> bool {
        matches!(
            opcode,
            0xcd | 0xc4 | 0xcc | 0xd4 | 0xdc | 0xe4 | 0xec | 0xf4 | 0xfc
        )
    }

    pub(crate) fn is_call_exchange(opcode: u8) -> bool {
        Self::is_call(opcode) || opcode == 0xe3
    }

    pub(crate) fn is_call_exchange_phase(&self) -> bool {
        matches!(
            self.phase,
            Phase::CallLow(_)
                | Phase::CallHigh(_, _)
                | Phase::ExchangeLow(_)
                | Phase::ExchangeHigh(_, _)
                | Phase::ExchangeWriteHigh(_, _)
                | Phase::ExchangeWriteLow(_, _)
        )
    }

    pub(crate) fn call_exchange_cycle(
        &self,
        cpu: &Cpu,
        opcode: u8,
        index: Option<u8>,
    ) -> Option<CycleKind> {
        if !Self::is_call_exchange(opcode)
            || self.fetch != access::Fetch::Memory
            || self.address != cpu.regs.sp
        {
            return None;
        }
        let pair = match ((opcode >> 4) & 3, index) {
            (2, Some(prefix)) => Pair::Index(prefix == Z80_PREFIX_FD),
            (register, _) => Pair::Register(register),
        };
        if self.pair != pair {
            return None;
        }
        let operand = 1 + usize::from(index.is_some());
        let count = usize::from(cpu.instruction_byte_count);
        if Self::is_call(opcode) {
            let taken = opcode == 0xcd || cpu.condition_is_true((opcode >> 3) & 7);
            let read = CycleKind::MemoryRead {
                address: cpu.regs.pc,
            };
            return match self.phase {
                Phase::CallLow(saved)
                    if count == operand && saved == taken && self.target.is_none() =>
                {
                    Some(read)
                }
                Phase::CallHigh(saved, low)
                    if count == operand + 1
                        && saved == taken
                        && low == cpu.instruction_bytes[operand]
                        && self.target.is_none() =>
                {
                    Some(if taken {
                        CycleKind::MemoryReadExtended {
                            address: cpu.regs.pc,
                        }
                    } else {
                        read
                    })
                }
                Phase::PushHigh(_) | Phase::PushLow(_) if count == operand + 2 && taken => {
                    let target = u16::from_le_bytes([
                        cpu.instruction_bytes[operand],
                        cpu.instruction_bytes[operand + 1],
                    ]);
                    if self.target != Some(target) {
                        return None;
                    }
                    let valid = match self.phase {
                        Phase::PushHigh(word) => word == cpu.regs.pc,
                        Phase::PushLow(low) => low == cpu.regs.pc.to_le_bytes()[0],
                        _ => false,
                    };
                    let (address, value) = self.write_cycle()?;
                    valid.then_some(CycleKind::MemoryWrite { address, value })
                }
                _ => None,
            };
        }
        if count != operand || self.target.is_some() {
            return None;
        }
        let original = pair.read(cpu);
        let high_address = self.address.wrapping_add(1);
        match self.phase {
            Phase::ExchangeLow(word) if word == original => Some(CycleKind::MemoryRead {
                address: self.address,
            }),
            Phase::ExchangeHigh(word, _) if word == original => {
                Some(CycleKind::MemoryReadExtended {
                    address: high_address,
                })
            }
            Phase::ExchangeWriteHigh(word, _) if word == original => Some(CycleKind::MemoryWrite {
                address: high_address,
                value: word.to_le_bytes()[1],
            }),
            Phase::ExchangeWriteLow(low, _) if low == original.to_le_bytes()[0] => {
                Some(CycleKind::MemoryWriteExtended {
                    address: self.address,
                    value: low,
                })
            }
            _ => None,
        }
    }

    pub(crate) fn complete_call_exchange<C: retirement_evidence::Capture>(
        mut self,
        cpu: &mut Cpu,
        sample: load::LoadSample,
        prepare: bool,
        capture: &C,
    ) -> Result<(Option<Self>, u32), ()> {
        let mut cycles = CYCLES_MEMORY_TRANSFER;
        self.phase = match (self.phase, sample) {
            (Phase::CallLow(taken), load::LoadSample::Read(value)) => {
                if self.fetch == access::Fetch::Memory {
                    cpu.accept_instruction_input(value, InstructionByteClass::Operand, capture);
                }
                Phase::CallHigh(taken, value)
            }
            (Phase::CallHigh(taken, low), load::LoadSample::Read(high)) => {
                if self.fetch == access::Fetch::Memory {
                    cpu.accept_instruction_input(high, InstructionByteClass::Operand, capture);
                }
                cpu.set_memptr(u16::from_le_bytes([low, high]));
                if !taken {
                    return Ok((None, cycles));
                }
                self.target = Some(u16::from_le_bytes([low, high]));
                self.phase = Phase::PushPrepare(cpu.regs.pc);
                if prepare {
                    return Ok((self.prepare(cpu)?, cycles + CYCLES_STACK_INTERNAL));
                }
                return Ok((Some(self), cycles));
            }
            (Phase::ExchangeLow(word), load::LoadSample::Read(low)) => {
                Phase::ExchangeHigh(word, low)
            }
            (Phase::ExchangeHigh(word, low), load::LoadSample::Read(high)) => {
                cycles = CYCLES_EXCHANGE_READ_HIGH;
                Phase::ExchangeWriteHigh(word, u16::from_le_bytes([low, high]))
            }
            (Phase::ExchangeWriteHigh(word, result), load::LoadSample::Written) => {
                Phase::ExchangeWriteLow(word.to_le_bytes()[0], result)
            }
            (Phase::ExchangeWriteLow(_, result), load::LoadSample::Written) => {
                self.pair.write(cpu, result);
                cpu.set_memptr(result);
                return Ok((None, CYCLES_EXCHANGE_WRITE_LOW));
            }
            _ => return Err(()),
        };
        Ok((Some(self), cycles))
    }
}
