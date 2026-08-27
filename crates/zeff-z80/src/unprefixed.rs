use super::*;

impl Cpu {
    pub(super) fn execute_unprefixed<B: Z80Bus>(
        &mut self,
        bus: &mut B,
        pc: u16,
        opcode: u8,
    ) -> u32 {
        match opcode {
            0x00 => CYCLES_NOP,
            0x01 => {
                let value = self.fetch_u16(bus);
                self.regs.set_bc(value);
                CYCLES_LD_RR_NN
            }
            0x02 => {
                bus.cpu_write(self.regs.bc(), self.regs.a);
                CYCLES_LD_INDIRECT_A
            }
            0x07 => {
                self.rotate_a_left_circular();
                CYCLES_ACCUMULATOR_ROTATE
            }
            0x08 => {
                self.exchange_af_with_shadow();
                CYCLES_EX_AF_AF_SHADOW
            }
            0x03 | 0x13 | 0x23 | 0x33 => {
                let pair = (opcode >> 4) & 0x03;
                let value = self.read_reg16(pair).wrapping_add(1);
                self.write_reg16(pair, value);
                CYCLES_INC_DEC_RR
            }
            0x04 | 0x0C | 0x14 | 0x1C | 0x24 | 0x2C | 0x34 | 0x3C => {
                let register = (opcode >> 3) & 0x07;
                let value = self.read_reg8(bus, register);
                let result = value.wrapping_add(1);
                self.write_reg8(bus, register, result);
                self.set_inc_flags(value, result);
                if register == REGISTER_MEMORY_INDEX {
                    CYCLES_INC_DEC_HL
                } else {
                    CYCLES_INC_DEC_R
                }
            }
            0x05 | 0x0D | 0x15 | 0x1D | 0x25 | 0x2D | 0x35 | 0x3D => {
                let register = (opcode >> 3) & 0x07;
                let value = self.read_reg8(bus, register);
                let result = value.wrapping_sub(1);
                self.write_reg8(bus, register, result);
                self.set_dec_flags(value, result);
                if register == REGISTER_MEMORY_INDEX {
                    CYCLES_INC_DEC_HL
                } else {
                    CYCLES_INC_DEC_R
                }
            }
            0x06 | 0x0E | 0x16 | 0x1E | 0x26 | 0x2E | 0x36 => {
                let register = (opcode >> 3) & 0x07;
                let value = self.fetch_u8(bus);
                self.write_reg8(bus, register, value);
                if register == REGISTER_MEMORY_INDEX {
                    CYCLES_LD_HL_N
                } else {
                    CYCLES_LD_R_N
                }
            }
            0x09 | 0x19 | 0x29 | 0x39 => {
                let pair = (opcode >> 4) & 0x03;
                self.add_hl(self.read_reg16(pair));
                CYCLES_ADD_HL_RR
            }
            0x0A => {
                self.regs.a = bus.cpu_read(self.regs.bc());
                CYCLES_LD_A_INDIRECT
            }
            0x0F => {
                self.rotate_a_right_circular();
                CYCLES_ACCUMULATOR_ROTATE
            }
            0x0B | 0x1B | 0x2B | 0x3B => {
                let pair = (opcode >> 4) & 0x03;
                let value = self.read_reg16(pair).wrapping_sub(1);
                self.write_reg16(pair, value);
                CYCLES_INC_DEC_RR
            }
            0x10 => {
                let displacement = self.fetch_u8(bus) as i8;
                self.regs.b = self.regs.b.wrapping_sub(1);
                if self.regs.b != 0 {
                    self.regs.pc = self.regs.pc.wrapping_add_signed(i16::from(displacement));
                    CYCLES_DJNZ
                } else {
                    CYCLES_DJNZ_NOT_TAKEN
                }
            }
            0x11 => {
                let value = self.fetch_u16(bus);
                self.regs.set_de(value);
                CYCLES_LD_RR_NN
            }
            0x12 => {
                bus.cpu_write(self.regs.de(), self.regs.a);
                CYCLES_LD_INDIRECT_A
            }
            0x17 => {
                self.rotate_a_left_through_carry();
                CYCLES_ACCUMULATOR_ROTATE
            }
            0x18 => {
                let displacement = self.fetch_u8(bus) as i8;
                self.regs.pc = self.regs.pc.wrapping_add_signed(i16::from(displacement));
                CYCLES_JR
            }
            0x1A => {
                self.regs.a = bus.cpu_read(self.regs.de());
                CYCLES_LD_A_INDIRECT
            }
            0x1F => {
                self.rotate_a_right_through_carry();
                CYCLES_ACCUMULATOR_ROTATE
            }
            0x20 | 0x28 | 0x30 | 0x38 => {
                let condition = (opcode >> 3) & 0x03;
                self.conditional_relative_jump(bus, condition)
            }
            0x21 => {
                let value = self.fetch_u16(bus);
                self.regs.set_hl(value);
                CYCLES_LD_RR_NN
            }
            0x22 => {
                let addr = self.fetch_u16(bus);
                self.write_mem_u16(bus, addr, self.regs.hl());
                CYCLES_LD_NN_HL
            }
            0x27 => {
                self.decimal_adjust_accumulator();
                CYCLES_FLAG_OP
            }
            0x2A => {
                let addr = self.fetch_u16(bus);
                let value = self.read_mem_u16(bus, addr);
                self.regs.set_hl(value);
                CYCLES_LD_HL_NN
            }
            0x2F => {
                self.complement_accumulator();
                CYCLES_FLAG_OP
            }
            0x31 => {
                self.regs.sp = self.fetch_u16(bus);
                CYCLES_LD_RR_NN
            }
            0x32 => {
                let addr = self.fetch_u16(bus);
                bus.cpu_write(addr, self.regs.a);
                CYCLES_LD_NN_A
            }
            0x37 => {
                self.set_carry_flag();
                CYCLES_FLAG_OP
            }
            0x3A => {
                let addr = self.fetch_u16(bus);
                self.regs.a = bus.cpu_read(addr);
                CYCLES_LD_A_NN
            }
            0x3F => {
                self.complement_carry_flag();
                CYCLES_FLAG_OP
            }
            0x3E => {
                self.regs.a = self.fetch_u8(bus);
                CYCLES_LD_A_N
            }
            0x40..=0x7F if opcode != 0x76 => {
                let dst = (opcode >> 3) & 0x07;
                let src = opcode & 0x07;
                let value = self.read_reg8(bus, src);
                self.write_reg8(bus, dst, value);
                if dst == REGISTER_MEMORY_INDEX || src == REGISTER_MEMORY_INDEX {
                    CYCLES_LD_R_HL_OR_HL_R
                } else {
                    CYCLES_LD_R_R
                }
            }
            0x76 => {
                self.state = CpuState::Halted;
                CYCLES_HALT
            }
            0x80..=0xBF => {
                let src = opcode & 0x07;
                let value = self.read_reg8(bus, src);
                self.execute_alu_group((opcode >> 3) & 0x07, value);
                if src == REGISTER_MEMORY_INDEX {
                    CYCLES_ALU_HL
                } else {
                    CYCLES_ALU_R
                }
            }
            0xC0 | 0xC8 | 0xD0 | 0xD8 | 0xE0 | 0xE8 | 0xF0 | 0xF8 => {
                let condition = (opcode >> 3) & 0x07;
                if self.condition_is_true(condition) {
                    self.regs.pc = self.pop_u16(bus);
                    CYCLES_RET_CC
                } else {
                    CYCLES_RET_CC_NOT_TAKEN
                }
            }
            0xC1 | 0xD1 | 0xE1 | 0xF1 => {
                let pair = (opcode >> 4) & 0x03;
                let value = self.pop_u16(bus);
                self.write_stack_reg16(pair, value);
                CYCLES_POP_RR
            }
            0xC2 | 0xCA | 0xD2 | 0xDA | 0xE2 | 0xEA | 0xF2 | 0xFA => {
                let condition = (opcode >> 3) & 0x07;
                let addr = self.fetch_u16(bus);
                if self.condition_is_true(condition) {
                    self.regs.pc = addr;
                }
                CYCLES_JP_NN
            }
            0xC3 => {
                self.regs.pc = self.fetch_u16(bus);
                CYCLES_JP_NN
            }
            0xC4 | 0xCC | 0xD4 | 0xDC | 0xE4 | 0xEC | 0xF4 | 0xFC => {
                let condition = (opcode >> 3) & 0x07;
                let addr = self.fetch_u16(bus);
                if self.condition_is_true(condition) {
                    self.push_u16(bus, self.regs.pc);
                    self.regs.pc = addr;
                    CYCLES_CALL_NN
                } else {
                    CYCLES_CALL_NN_NOT_TAKEN
                }
            }
            0xC5 | 0xD5 | 0xE5 | 0xF5 => {
                let pair = (opcode >> 4) & 0x03;
                self.push_u16(bus, self.read_stack_reg16(pair));
                CYCLES_PUSH_RR
            }
            0xC6 | 0xCE | 0xD6 | 0xDE | 0xE6 | 0xEE | 0xF6 | 0xFE => {
                let value = self.fetch_u8(bus);
                self.execute_alu_group((opcode >> 3) & 0x07, value);
                CYCLES_ALU_N
            }
            0xC7 | 0xCF | 0xD7 | 0xDF | 0xE7 | 0xEF | 0xF7 | 0xFF => {
                let vector = u16::from(opcode & 0x38);
                self.push_u16(bus, self.regs.pc);
                self.regs.pc = vector;
                CYCLES_RST
            }
            0xC9 => {
                self.regs.pc = self.pop_u16(bus);
                CYCLES_RET
            }
            Z80_PREFIX_CB => self.execute_cb(bus),
            0xCD => {
                let addr = self.fetch_u16(bus);
                self.push_u16(bus, self.regs.pc);
                self.regs.pc = addr;
                CYCLES_CALL_NN
            }
            0xD3 => {
                let port = self.fetch_u8(bus);
                let t_states_before = self.immediate_io_t_states_before();
                self.write_io(bus, port, self.regs.a, t_states_before);
                CYCLES_OUT_N_A
            }
            0xD9 => {
                self.exchange_shadow_registers();
                CYCLES_EXX
            }
            0xDB => {
                let port = self.fetch_u8(bus);
                self.regs.a = bus.io_read(port);
                CYCLES_IN_A_N
            }
            0xE3 => {
                self.exchange_stack_with_hl(bus);
                CYCLES_EX_SP_HL
            }
            0xE9 => {
                self.regs.pc = self.regs.hl();
                CYCLES_JP_HL
            }
            0xEB => {
                self.exchange_de_hl();
                CYCLES_EX_DE_HL
            }
            0xF3 => {
                self.disable_interrupts();
                CYCLES_DI_EI
            }
            0xF9 => {
                self.regs.sp = self.regs.hl();
                CYCLES_LD_SP_HL
            }
            0xFB => {
                self.schedule_enable_interrupts();
                CYCLES_DI_EI
            }
            Z80_PREFIX_DD | Z80_PREFIX_FD => self.execute_index(bus, pc, opcode),
            Z80_PREFIX_ED => self.execute_ed(bus, pc),
            _ => {
                self.state = CpuState::Suspended;
                self.trap = Some(CpuTrap::UnsupportedOpcode { pc, opcode });
                CYCLES_UNSUPPORTED
            }
        }
    }
}
