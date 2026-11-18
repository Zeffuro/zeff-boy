use super::*;

impl Cpu {
    fn prefix_context(&self, transfer: Transfer, prefix: u8, fetches: u8, bytes: u8) -> bool {
        self.last_opcode == prefix
            && self.last_opcode_pc == transfer.pc
            && self.instruction_bytes[0] == prefix
            && self.instruction_byte_count == bytes
            && self.last_m1_fetch_count == fetches
            && self.regs.pc == transfer.pc.wrapping_add(u16::from(bytes))
    }

    fn physical_terminal(&self, transfer: Transfer) -> Option<(Option<u8>, u8)> {
        let prefix =
            matches!(self.last_opcode, Z80_PREFIX_CB | Z80_PREFIX_ED).then_some(self.last_opcode);
        let count = if prefix.is_some() { 2 } else { 1 };
        if !self.prefix_context(transfer, self.last_opcode, count, count) {
            return None;
        }
        Some((
            prefix,
            if prefix.is_some() {
                self.instruction_bytes[1]
            } else {
                self.last_opcode
            },
        ))
    }

    pub(super) fn physical_rmw(
        &self,
        transfer: Transfer,
        operation: rmw::Rmw,
    ) -> Option<CycleKind> {
        let (prefix, opcode) = self.physical_terminal(transfer)?;
        match prefix {
            None => operation.physical_cycle(self, opcode, false),
            Some(Z80_PREFIX_CB) => operation.physical_cycle(self, opcode, true),
            _ => None,
        }
    }

    pub(super) fn physical_stack(
        &self,
        transfer: Transfer,
        operation: stack::Stack,
    ) -> Option<CycleKind> {
        if stack::Stack::is_call_exchange(self.last_opcode) {
            if !self.prefix_context(transfer, self.last_opcode, 1, self.instruction_byte_count) {
                return None;
            }
            return operation.call_exchange_cycle(self, self.last_opcode, None);
        }
        if let Some(address) = operation.jump_cycle(self, self.last_opcode) {
            return Some(CycleKind::MemoryRead { address });
        }
        let (prefix, opcode) = self.physical_terminal(transfer)?;
        match prefix {
            None if stack::Stack::prepares(opcode) => operation.extra_cycle(self, opcode, None),
            None => operation
                .pop_cycle(self, opcode, false)
                .map(|address| CycleKind::MemoryRead { address }),
            Some(Z80_PREFIX_ED)
                if !self.interrupt_flip_flop_1
                    && !self.interrupt_flip_flop_2
                    && self.enable_interrupts_delay == 0 =>
            {
                operation
                    .pop_cycle(self, opcode, true)
                    .map(|address| CycleKind::MemoryRead { address })
            }
            _ => None,
        }
    }

    pub(super) fn physical_prefix(&self, transfer: Transfer) -> Option<u8> {
        let prefix = match transfer.operation {
            Operation::CbFetch => Z80_PREFIX_CB,
            Operation::EdFetch { pc } if pc == transfer.pc => Z80_PREFIX_ED,
            _ => return None,
        };
        self.prefix_context(transfer, prefix, 1, 1)
            .then_some(prefix)
    }

    pub(super) fn physical_load(
        &self,
        transfer: Transfer,
        load: load::Load,
    ) -> Option<load::LoadCycle> {
        if self.last_opcode == Z80_PREFIX_ED {
            if !self.prefix_context(transfer, Z80_PREFIX_ED, 2, self.instruction_byte_count) {
                return None;
            }
            load.physical_ed_cycle(self, self.instruction_bytes[1])
        } else {
            load::Load::decode(self, self.last_opcode, None)?;
            load.physical_cycle(self, self.last_opcode)
        }
    }

    pub(super) fn prefix_sampled_t_states(
        &self,
        bus: &BusState,
        value: u8,
    ) -> Result<u32, CycleError> {
        let transfer = bus.transfer.ok_or(CycleError::InvalidCompletion)?;
        let prefix = self
            .physical_prefix(transfer)
            .ok_or(CycleError::InvalidCompletion)?;
        let admitted = match prefix {
            Z80_PREFIX_CB => {
                value & 7 != REGISTER_MEMORY_INDEX
                    || value >> 6 != 1
                    || self.memptr_profile == MemptrProfile::Nmos && self.memptr.is_some()
            }
            Z80_PREFIX_ED => {
                ed::is_neg(value)
                    || load::Load::decode_ed(self, value).is_some()
                    || (stack::Stack::is_ed_return(value)
                        && !self.interrupt_flip_flop_1
                        && !self.interrupt_flip_flop_2
                        && self.enable_interrupts_delay == 0)
            }
            _ => false,
        };
        if !admitted {
            return Err(CycleError::UnsupportedOpcode {
                pc: self.regs.pc,
                opcode: value,
            });
        }
        Ok(CYCLES_M1)
    }

    pub(super) fn complete_prefix_fetch<C: retirement_evidence::Capture>(
        &mut self,
        bus: &mut BusState,
        value: u8,
        capture: &C,
    ) -> Result<BoundedStep, CycleError> {
        let transfer = bus.transfer.ok_or(CycleError::InvalidCompletion)?;
        let prefix = self
            .physical_prefix(transfer)
            .ok_or(CycleError::InvalidCompletion)?;
        self.prefix_sampled_t_states(bus, value)?;
        self.accept_instruction_input(value, InstructionByteClass::M1, capture);
        self.increment_refresh_register();
        let next = if prefix == Z80_PREFIX_CB {
            if self.execute_cb_register(value).is_some() {
                None
            } else {
                Some(Operation::Rmw(rmw::Rmw::cb(self, value)))
            }
        } else if self.execute_ed_neg(value).is_some() {
            None
        } else if let Some(operation) = load::Load::decode_ed(self, value) {
            Some(Operation::Load(operation))
        } else {
            Some(Operation::Stack(
                stack::Stack::decode_ed(self, value).ok_or(CycleError::InvalidCompletion)?,
            ))
        };
        bus.transfer = next.map(|operation| Transfer {
            pc: transfer.pc,
            operation,
        });
        if next.is_none() {
            self.finish_instruction(CYCLES_M1);
        } else {
            self.cycles = self.cycles.wrapping_add(u64::from(CYCLES_M1));
        }
        Ok(BoundedStep {
            instruction: FetchedInstruction {
                pc: transfer.pc,
                opcode: self.last_opcode,
                cycles: CYCLES_M1,
            },
            complete: next.is_none(),
        })
    }
}
