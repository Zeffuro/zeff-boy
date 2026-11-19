use super::*;
use crate::index_cycle as indexed;

impl Cpu {
    pub(super) fn index_context(
        &self,
        bus: &BusState,
        bytes: u8,
        fetches: u8,
    ) -> Option<(u16, u8)> {
        let (pc, prefix) = bus.prefix.selected_index()?;
        if !matches!(prefix, Z80_PREFIX_DD | Z80_PREFIX_FD)
            || self.last_opcode != prefix
            || self.last_opcode_pc != pc
            || self.instruction_bytes[0] != prefix
            || self.instruction_byte_count != bytes
            || self.last_m1_fetch_count != fetches
            || self.regs.pc != pc.wrapping_add(u16::from(bytes))
        {
            return None;
        }
        Some((pc, prefix))
    }

    pub(super) fn physical_index_cycle(&self, bus: &BusState) -> Option<CycleKind> {
        let Some(transfer) = bus.transfer else {
            self.index_context(bus, 1, 1)?;
            return Some(CycleKind::M1 {
                address: self.regs.pc,
                halted: false,
                refresh_address: u16::from_be_bytes([self.regs.i, self.regs.r]),
            });
        };
        let (pc, prefix) = self.index_context(bus, self.instruction_byte_count, 2)?;
        let opcode = self.instruction_bytes[1];
        if transfer.pc != pc || !indexed::admitted(opcode) {
            return None;
        }
        let cycle = match transfer.operation {
            Operation::Load(operation) => operation.physical_index_cycle(self, opcode, prefix)?,
            Operation::Stack(operation) => {
                if stack::Stack::is_call_exchange(opcode) {
                    return operation.call_exchange_cycle(self, opcode, Some(prefix));
                }
                if stack::Stack::prepares(opcode) {
                    if self.instruction_byte_count != 2 {
                        return None;
                    }
                    return operation.extra_cycle(self, opcode, Some(prefix));
                }
                return Some(CycleKind::MemoryRead {
                    address: operation.index_cycle(self, opcode, prefix)?,
                });
            }
            Operation::Single {
                opcode: stored,
                index: Some(selected),
            } if stored == opcode && selected == prefix && self.instruction_byte_count == 2 => {
                indexed::Single::decode(self, opcode, prefix)?.cycle(self, opcode)?
            }
            _ => return None,
        };
        Some(match cycle {
            load::LoadCycle::Fetch { address } | load::LoadCycle::Read { address } => {
                CycleKind::MemoryRead { address }
            }
            load::LoadCycle::Write { address, value } => CycleKind::MemoryWrite { address, value },
        })
    }

    pub(super) fn index_sampled_t_states(
        &self,
        bus: &BusState,
        value: u8,
    ) -> Result<u32, CycleError> {
        self.index_context(bus, 1, 1)
            .ok_or(CycleError::InvalidCompletion)?;
        if bus.transfer.is_some() {
            return Err(CycleError::InvalidCompletion);
        }
        if !indexed::admitted(value) {
            return Err(CycleError::UnsupportedOpcode {
                pc: self.regs.pc,
                opcode: value,
            });
        }
        Ok(m1_timing::base_t_states(value))
    }

    pub(super) fn complete_index_fetch<C: retirement_evidence::Capture>(
        &mut self,
        bus: &mut BusState,
        value: u8,
        cycles: u32,
        capture: &C,
    ) -> Result<BoundedStep, CycleError> {
        let (pc, prefix) = self
            .index_context(bus, 1, 1)
            .ok_or(CycleError::InvalidCompletion)?;
        if bus.transfer.is_some() {
            return Err(CycleError::InvalidCompletion);
        }
        self.index_sampled_t_states(bus, value)?;
        self.accept_instruction_input(value, InstructionByteClass::M1, capture);
        self.increment_refresh_register();
        if stack::Stack::prepares(value) {
            return self.complete_stack_m1(bus, pc, value, Some(prefix), cycles);
        }
        let next = if self.execute_m1_register(value, Some(prefix)).is_some()
            || self.execute_index_register(prefix, value).is_some()
        {
            None
        } else if value == 0x76 {
            self.enter_halt();
            None
        } else if let Some(operation) = load::Load::decode(self, value, Some(prefix)) {
            Some(Operation::Load(operation))
        } else if let Some(operation) = stack::Stack::decode(self, value, Some(prefix)) {
            Some(Operation::Stack(operation))
        } else if indexed::Single::decode(self, value, prefix).is_some() {
            Some(Operation::Single {
                opcode: value,
                index: Some(prefix),
            })
        } else {
            return Err(CycleError::InvalidCompletion);
        };
        bus.transfer = next.map(|operation| Transfer { pc, operation });
        if next.is_none() {
            bus.prefix = PrefixState::default();
            self.finish_instruction(cycles);
        } else {
            self.cycles = self.cycles.wrapping_add(u64::from(cycles));
        }
        Ok(BoundedStep {
            instruction: FetchedInstruction {
                pc,
                opcode: prefix,
                cycles,
            },
            complete: next.is_none(),
        })
    }
}
