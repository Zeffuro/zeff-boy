use super::*;

impl Cpu {
    pub(super) fn enter_halt(&mut self) -> u32 {
        self.state = CpuState::Halted;
        CYCLES_HALT
    }

    pub(super) fn execute_unprefixed<B: Z80Bus>(
        &mut self,
        bus: &mut B,
        pc: u16,
        opcode: u8,
    ) -> u32 {
        if let Some(cycles) = self.execute_m1_register(opcode, None) {
            return cycles;
        }
        if let Some(cycles) = self.execute_register_cycle(opcode) {
            return cycles;
        }
        if let Some(load) = load::Load::decode(self, opcode, None) {
            return load.execute(self, bus);
        }
        if let Some(operand) = operand_cycle::Operand::decode(self, opcode) {
            operand.step(self, bus);
            return CYCLES_M1 + CYCLES_MEMORY_TRANSFER;
        }
        if let Some(operation) = stack::Stack::decode(self, opcode, None) {
            return operation.execute(self, bus);
        }
        if let Some(rmw) = rmw::Rmw::inc_dec(self, opcode) {
            rmw.execute(self, bus);
            return CYCLES_INC_DEC_HL;
        }
        match opcode {
            0x09 | 0x19 | 0x29 | 0x39 => {
                let pair = (opcode >> 4) & 0x03;
                self.add_hl(self.read_reg16(pair));
                CYCLES_ADD_HL_RR
            }
            0x10 => {
                let displacement = self.fetch_u8(bus) as i8;
                self.regs.b = self.regs.b.wrapping_sub(1);
                if self.regs.b != 0 {
                    self.apply_relative_jump(displacement);
                    CYCLES_DJNZ
                } else {
                    CYCLES_DJNZ_NOT_TAKEN
                }
            }
            0x18 => {
                let displacement = self.fetch_u8(bus) as i8;
                self.apply_relative_jump(displacement);
                CYCLES_JR
            }
            0x20 | 0x28 | 0x30 | 0x38 => {
                let condition = (opcode >> 3) & 0x03;
                self.conditional_relative_jump(bus, condition)
            }

            0x76 => self.enter_halt(),
            Z80_PREFIX_CB => self.execute_cb(bus),
            0xD3 => {
                let port = self.fetch_u8(bus);
                let t_states_before = self.immediate_io_t_states_before();
                self.write_io(bus, port, self.regs.a, t_states_before);
                self.memptr_accumulator_store(u16::from(port), self.regs.a);
                CYCLES_OUT_N_A
            }
            0xDB => {
                let port = self.fetch_u8(bus);
                self.set_memptr(u16::from_be_bytes([self.regs.a, port]).wrapping_add(1));
                self.regs.a = bus.io_read(port);
                CYCLES_IN_A_N
            }
            0xFB => {
                self.schedule_enable_interrupts();
                CYCLES_DI_EI
            }
            Z80_PREFIX_DD | Z80_PREFIX_FD => self.execute_index(bus, pc, opcode),
            Z80_PREFIX_ED => self.execute_ed(bus, pc),
            _ => {
                self.memptr = None;
                self.state = CpuState::Suspended;
                self.trap = Some(CpuTrap::UnsupportedOpcode { pc, opcode });
                CYCLES_UNSUPPORTED
            }
        }
    }
}
