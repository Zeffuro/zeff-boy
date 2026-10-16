use super::*;

impl Cpu {
    pub(super) fn execute_unprefixed<B: Z80Bus>(
        &mut self,
        bus: &mut B,
        pc: u16,
        opcode: u8,
    ) -> u32 {
        if let Some(load) = load::Load::decode(self, opcode, None) {
            return load.execute(self, bus);
        }
        if let Some(operation) = stack::Stack::decode(self, opcode, None) {
            return operation.execute(self, bus);
        }
        if let Some(rmw) = rmw::Rmw::inc_dec(self, opcode) {
            rmw.execute(self, bus);
            return CYCLES_INC_DEC_HL;
        }
        match opcode {
            0x00 => CYCLES_NOP,
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

            0x27 => {
                self.decimal_adjust_accumulator();
                CYCLES_FLAG_OP
            }
            0x2F => {
                self.complement_accumulator();
                CYCLES_FLAG_OP
            }

            0x37 => {
                self.set_carry_flag();
                CYCLES_FLAG_OP
            }
            0x3F => {
                self.complement_carry_flag();
                CYCLES_FLAG_OP
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
            0xC6 | 0xCE | 0xD6 | 0xDE | 0xE6 | 0xEE | 0xF6 | 0xFE => {
                let value = self.fetch_u8(bus);
                self.execute_alu_group((opcode >> 3) & 0x07, value);
                CYCLES_ALU_N
            }
            Z80_PREFIX_CB => self.execute_cb(bus),
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
