use super::*;

pub(super) fn base_t_states(opcode: u8) -> u32 {
    if m1_register::admitted(opcode) {
        CYCLES_INC_DEC_RR
    } else if stack::Stack::prepares(opcode) {
        CYCLES_M1 + CYCLES_STACK_INTERNAL
    } else {
        CYCLES_M1
    }
}

impl Cpu {
    pub(super) fn sampled_m1_t_states(
        &self,
        bus: &BusState,
        kind: CycleKind,
        value: u8,
    ) -> Result<u32, CycleError> {
        if matches!(kind, CycleKind::M1 { halted: true, .. }) {
            return Ok(CYCLES_M1);
        }
        if bus.prefix.pending() {
            return self.index_sampled_t_states(bus, value);
        }
        if bus.transfer.is_some() {
            return self.prefix_sampled_t_states(bus, value);
        }
        let admitted = matches!(
            value,
            0x76 | Z80_PREFIX_CB | Z80_PREFIX_ED | Z80_PREFIX_DD | Z80_PREFIX_FD
        ) || load::Load::decode(self, value, None).is_some()
            || operand_cycle::Operand::decode(self, value).is_some()
            || stack::Stack::is_jump(value)
            || stack::Stack::is_pop(value)
            || stack::Stack::is_call_exchange(value)
            || stack::Stack::prepares(value)
            || m1_register::admitted(value)
            || rmw::Rmw::inc_dec(self, value).is_some()
            || register_cycle::decode(value).is_some();
        if !admitted {
            return Err(CycleError::UnsupportedOpcode {
                pc: self.regs.pc,
                opcode: value,
            });
        }
        Ok(base_t_states(value))
    }

    pub(super) fn complete_stack_m1(
        &mut self,
        bus: &mut BusState,
        pc: u16,
        opcode: u8,
        index: Option<u8>,
        cycles: u32,
    ) -> Result<BoundedStep, CycleError> {
        if cycles != CYCLES_M1 + CYCLES_STACK_INTERNAL {
            return Err(CycleError::InvalidCompletion);
        }
        let operation =
            stack::Stack::decode(self, opcode, index).ok_or(CycleError::InvalidCompletion)?;
        let next = operation
            .prepare(self)
            .map_err(|()| CycleError::InvalidCompletion)?;
        bus.transfer = next.map(|operation| Transfer {
            pc,
            operation: Operation::Stack(operation),
        });
        if next.is_none() {
            bus.prefix = PrefixState::default();
            self.finish_instruction(cycles);
        } else {
            self.cycles = self.cycles.wrapping_add(u64::from(cycles));
        }
        Ok(BoundedStep {
            instruction: FetchedInstruction {
                pc,
                opcode: self.last_opcode,
                cycles,
            },
            complete: next.is_none(),
        })
    }
}
