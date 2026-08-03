use super::*;

impl Cpu {
    pub(super) fn execute(
        &mut self,
        opcode: u8,
        segment_override: Option<SegmentRegister>,
        repeat_prefix: Option<RepeatPrefix>,
        bus: &mut Bus,
    ) {
        match opcode {
            0x00 => self.alu_rm_reg8(AluOp::Add, segment_override, bus),
            0x01 => self.alu_rm_reg16(AluOp::Add, segment_override, bus),
            0x02 => self.alu_reg_rm8(AluOp::Add, segment_override, bus),
            0x03 => self.alu_reg_rm16(AluOp::Add, segment_override, bus),
            0x04 => {
                let rhs = self.fetch8(bus);
                let lhs = self.get_reg8(REG_AX);
                let result = self.alu8(AluOp::Add, lhs, rhs);
                self.set_reg8(REG_AX, result);
                self.add_cycles(bus, 4);
            }
            0x05 => {
                let rhs = self.fetch16(bus);
                let lhs = self.get_reg16(REG_AX);
                let result = self.alu16(AluOp::Add, lhs, rhs);
                self.set_reg16(REG_AX, result);
                self.add_cycles(bus, 4);
            }
            0x06 => self.push_segment(SegmentRegister::Es, bus),
            0x07 => self.pop_segment(SegmentRegister::Es, bus),
            0x08 => self.alu_rm_reg8(AluOp::Or, segment_override, bus),
            0x09 => self.alu_rm_reg16(AluOp::Or, segment_override, bus),
            0x0A => self.alu_reg_rm8(AluOp::Or, segment_override, bus),
            0x0B => self.alu_reg_rm16(AluOp::Or, segment_override, bus),
            0x0C => {
                let rhs = self.fetch8(bus);
                let lhs = self.get_reg8(REG_AX);
                let result = self.alu8(AluOp::Or, lhs, rhs);
                self.set_reg8(REG_AX, result);
                self.add_cycles(bus, 4);
            }
            0x0D => {
                let rhs = self.fetch16(bus);
                let lhs = self.get_reg16(REG_AX);
                let result = self.alu16(AluOp::Or, lhs, rhs);
                self.set_reg16(REG_AX, result);
                self.add_cycles(bus, 4);
            }
            0x0E => self.push_segment(SegmentRegister::Cs, bus),
            0x10 => self.alu_rm_reg8(AluOp::Adc, segment_override, bus),
            0x11 => self.alu_rm_reg16(AluOp::Adc, segment_override, bus),
            0x12 => self.alu_reg_rm8(AluOp::Adc, segment_override, bus),
            0x13 => self.alu_reg_rm16(AluOp::Adc, segment_override, bus),
            0x14 => {
                let rhs = self.fetch8(bus);
                let lhs = self.get_reg8(REG_AX);
                let result = self.alu8(AluOp::Adc, lhs, rhs);
                self.set_reg8(REG_AX, result);
                self.add_cycles(bus, 4);
            }
            0x15 => {
                let rhs = self.fetch16(bus);
                let lhs = self.get_reg16(REG_AX);
                let result = self.alu16(AluOp::Adc, lhs, rhs);
                self.set_reg16(REG_AX, result);
                self.add_cycles(bus, 4);
            }
            0x16 => self.push_segment(SegmentRegister::Ss, bus),
            0x17 => self.pop_segment(SegmentRegister::Ss, bus),
            0x18 => self.alu_rm_reg8(AluOp::Sbb, segment_override, bus),
            0x19 => self.alu_rm_reg16(AluOp::Sbb, segment_override, bus),
            0x1A => self.alu_reg_rm8(AluOp::Sbb, segment_override, bus),
            0x1B => self.alu_reg_rm16(AluOp::Sbb, segment_override, bus),
            0x1C => {
                let rhs = self.fetch8(bus);
                let lhs = self.get_reg8(REG_AX);
                let result = self.alu8(AluOp::Sbb, lhs, rhs);
                self.set_reg8(REG_AX, result);
                self.add_cycles(bus, 4);
            }
            0x1D => {
                let rhs = self.fetch16(bus);
                let lhs = self.get_reg16(REG_AX);
                let result = self.alu16(AluOp::Sbb, lhs, rhs);
                self.set_reg16(REG_AX, result);
                self.add_cycles(bus, 4);
            }
            0x1E => self.push_segment(SegmentRegister::Ds, bus),
            0x1F => self.pop_segment(SegmentRegister::Ds, bus),
            0x20 => self.alu_rm_reg8(AluOp::And, segment_override, bus),
            0x21 => self.alu_rm_reg16(AluOp::And, segment_override, bus),
            0x22 => self.alu_reg_rm8(AluOp::And, segment_override, bus),
            0x23 => self.alu_reg_rm16(AluOp::And, segment_override, bus),
            0x24 => {
                let rhs = self.fetch8(bus);
                let lhs = self.get_reg8(REG_AX);
                let result = self.alu8(AluOp::And, lhs, rhs);
                self.set_reg8(REG_AX, result);
                self.add_cycles(bus, 4);
            }
            0x25 => {
                let rhs = self.fetch16(bus);
                let lhs = self.get_reg16(REG_AX);
                let result = self.alu16(AluOp::And, lhs, rhs);
                self.set_reg16(REG_AX, result);
                self.add_cycles(bus, 4);
            }
            0x27 => self.daa(bus),
            0x28 => self.alu_rm_reg8(AluOp::Sub, segment_override, bus),
            0x29 => self.alu_rm_reg16(AluOp::Sub, segment_override, bus),
            0x2A => self.alu_reg_rm8(AluOp::Sub, segment_override, bus),
            0x2B => self.alu_reg_rm16(AluOp::Sub, segment_override, bus),
            0x2C => {
                let rhs = self.fetch8(bus);
                let lhs = self.get_reg8(REG_AX);
                let result = self.alu8(AluOp::Sub, lhs, rhs);
                self.set_reg8(REG_AX, result);
                self.add_cycles(bus, 4);
            }
            0x2D => {
                let rhs = self.fetch16(bus);
                let lhs = self.get_reg16(REG_AX);
                let result = self.alu16(AluOp::Sub, lhs, rhs);
                self.set_reg16(REG_AX, result);
                self.add_cycles(bus, 4);
            }
            0x2F => self.das(bus),
            0x30 => self.alu_rm_reg8(AluOp::Xor, segment_override, bus),
            0x31 => self.alu_rm_reg16(AluOp::Xor, segment_override, bus),
            0x32 => self.alu_reg_rm8(AluOp::Xor, segment_override, bus),
            0x33 => self.alu_reg_rm16(AluOp::Xor, segment_override, bus),
            0x34 => {
                let rhs = self.fetch8(bus);
                let lhs = self.get_reg8(REG_AX);
                let result = self.alu8(AluOp::Xor, lhs, rhs);
                self.set_reg8(REG_AX, result);
                self.add_cycles(bus, 4);
            }
            0x35 => {
                let rhs = self.fetch16(bus);
                let lhs = self.get_reg16(REG_AX);
                let result = self.alu16(AluOp::Xor, lhs, rhs);
                self.set_reg16(REG_AX, result);
                self.add_cycles(bus, 4);
            }
            0x38 => self.alu_rm_reg8(AluOp::Cmp, segment_override, bus),
            0x39 => self.alu_rm_reg16(AluOp::Cmp, segment_override, bus),
            0x3A => self.alu_reg_rm8(AluOp::Cmp, segment_override, bus),
            0x3B => self.alu_reg_rm16(AluOp::Cmp, segment_override, bus),
            0x3C => {
                let rhs = self.fetch8(bus);
                let lhs = self.get_reg8(REG_AX);
                self.alu8(AluOp::Cmp, lhs, rhs);
                self.add_cycles(bus, 4);
            }
            0x3D => {
                let rhs = self.fetch16(bus);
                let lhs = self.get_reg16(REG_AX);
                self.alu16(AluOp::Cmp, lhs, rhs);
                self.add_cycles(bus, 4);
            }
            0x40..=0x47 => {
                let reg = opcode - 0x40;
                let value = self.get_reg16(reg).wrapping_add(1);
                self.set_inc_dec_flags16(value, false);
                self.set_reg16(reg, value);
                self.add_cycles(bus, 3);
            }
            0x48..=0x4F => {
                let reg = opcode - 0x48;
                let value = self.get_reg16(reg).wrapping_sub(1);
                self.set_inc_dec_flags16(value, true);
                self.set_reg16(reg, value);
                self.add_cycles(bus, 3);
            }
            0x50..=0x57 => {
                self.push16(self.get_reg16(opcode - 0x50), bus);
                self.add_cycles(bus, 1);
            }
            0x58..=0x5F => {
                let value = self.pop16(bus);
                self.set_reg16(opcode - 0x58, value);
                self.add_cycles(bus, 1);
            }
            0x60 => {
                let original_sp = self.get_reg16(REG_SP);
                for reg in [
                    REG_AX, REG_CX, REG_DX, REG_BX, REG_SP, REG_BP, REG_SI, REG_DI,
                ] {
                    let value = if reg == REG_SP {
                        original_sp
                    } else {
                        self.get_reg16(reg)
                    };
                    self.push16(value, bus);
                }
                self.add_cycles(bus, 9);
            }
            0x61 => {
                for reg in [
                    REG_DI, REG_SI, REG_BP, REG_SP, REG_BX, REG_DX, REG_CX, REG_AX,
                ] {
                    let value = self.pop16(bus);
                    if reg != REG_SP {
                        self.set_reg16(reg, value);
                    }
                }
                self.add_cycles(bus, 8);
            }
            0x68 => {
                let value = self.fetch16(bus);
                self.push16(value, bus);
                self.add_cycles(bus, 1);
            }
            0x69 => self.imul_reg_rm_imm16(false, segment_override, bus),
            0x6A => {
                let value = self.fetch8(bus) as i8 as i16 as u16;
                self.push16(value, bus);
                self.add_cycles(bus, 1);
            }
            0x6B => self.imul_reg_rm_imm16(true, segment_override, bus),
            0x6C => self.ins8(repeat_prefix, bus),
            0x6D => self.ins16(repeat_prefix, bus),
            0x6E => self.outs8(segment_override, repeat_prefix, bus),
            0x6F => self.outs16(segment_override, repeat_prefix, bus),
            0x70..=0x7F => {
                let rel = self.fetch8(bus) as i8;
                if self.condition(opcode & 0x0F) {
                    self.ip = self.ip.wrapping_add_signed(i16::from(rel));
                }
                self.add_cycles(bus, 4);
            }
            0x80 => self.alu_rm_imm8(false, segment_override, bus),
            0x81 => self.alu_rm_imm16(false, segment_override, bus),
            0x82 => self.alu_rm_imm8_with_opcode(0x82, false, segment_override, bus),
            0x83 => self.alu_rm_imm16(true, segment_override, bus),
            0x84 => self.test_rm_reg8(segment_override, bus),
            0x85 => self.test_rm_reg16(segment_override, bus),
            0x86 => self.xchg_rm_reg8(segment_override, bus),
            0x87 => self.xchg_rm_reg16(segment_override, bus),
            0x88 => self.mov_rm_reg8(segment_override, bus),
            0x89 => self.mov_rm_reg16(segment_override, bus),
            0x8A => self.mov_reg_rm8(segment_override, bus),
            0x8B => self.mov_reg_rm16(segment_override, bus),
            0x8C => self.mov_rm_sreg(segment_override, bus),
            0x8D => self.lea_reg_m(segment_override, bus),
            0x8E => self.mov_sreg_rm(segment_override, bus),
            0x8F => self.pop_rm16(segment_override, bus),
            0x90 => self.add_cycles(bus, 3),
            0x91..=0x97 => {
                let reg = opcode - 0x90;
                let ax = self.get_reg16(REG_AX);
                let other = self.get_reg16(reg);
                self.set_reg16(REG_AX, other);
                self.set_reg16(reg, ax);
                self.add_cycles(bus, 3);
            }
            0x98 => {
                let al = self.get_reg8(REG_AX) as i8 as i16 as u16;
                self.set_reg16(REG_AX, al);
                self.add_cycles(bus, 2);
            }
            0x99 => {
                let dx = if self.get_reg16(REG_AX) & 0x8000 != 0 {
                    0xFFFF
                } else {
                    0
                };
                self.set_reg16(REG_DX, dx);
                self.add_cycles(bus, 2);
            }
            0x9A => {
                let ip = self.fetch16(bus);
                let cs = self.fetch16(bus);
                self.push16(self.segments[SegmentRegister::Cs.index()], bus);
                self.push16(self.ip, bus);
                self.ip = ip;
                self.segments[SegmentRegister::Cs.index()] = cs;
                self.add_cycles(bus, 10);
            }
            0x9B => self.add_cycles(bus, 3),
            0x9C => {
                self.push16(self.flags | FLAG_FIXED, bus);
                self.add_cycles(bus, 8);
            }
            0x9D => {
                self.flags = self.pop16(bus) | FLAG_FIXED;
                self.add_cycles(bus, 8);
            }
            0x9E => {
                let ah = u16::from(self.get_reg8(REG_AX | 0x04));
                let mask = FLAG_SF | FLAG_ZF | FLAG_AF | FLAG_PF | FLAG_CF;
                self.flags = (self.flags & !mask) | (ah & mask) | FLAG_FIXED;
                self.add_cycles(bus, 2);
            }
            0x9F => {
                let mask = FLAG_SF | FLAG_ZF | FLAG_AF | FLAG_PF | FLAG_CF;
                self.set_reg8(
                    REG_AX | 0x04,
                    ((self.flags & mask) as u8) | FLAG_FIXED as u8,
                );
                self.add_cycles(bus, 2);
            }
            0xA4 => self.movs8(segment_override, repeat_prefix, bus),
            0xA5 => self.movs16(segment_override, repeat_prefix, bus),
            0xA6 => self.cmps8(segment_override, repeat_prefix, bus),
            0xA7 => self.cmps16(segment_override, repeat_prefix, bus),
            0xA8 => {
                let rhs = self.fetch8(bus);
                self.alu8(AluOp::And, self.get_reg8(REG_AX), rhs);
                self.add_cycles(bus, 4);
            }
            0xA9 => {
                let rhs = self.fetch16(bus);
                self.alu16(AluOp::And, self.get_reg16(REG_AX), rhs);
                self.add_cycles(bus, 4);
            }
            0xAA => self.stos8(repeat_prefix, bus),
            0xAB => self.stos16(repeat_prefix, bus),
            0xAC => self.lods8(segment_override, repeat_prefix, bus),
            0xAD => self.lods16(segment_override, repeat_prefix, bus),
            0xAE => self.scas8(repeat_prefix, bus),
            0xAF => self.scas16(repeat_prefix, bus),
            0xA0 => {
                let offset = self.fetch16(bus);
                let value = bus.read8(self.overridden_address(segment_override, offset));
                self.set_reg8(REG_AX, value);
                self.add_cycles(bus, 10);
            }
            0xA1 => {
                let offset = self.fetch16(bus);
                let value = bus.read16(self.overridden_address(segment_override, offset));
                self.set_reg16(REG_AX, value);
                self.add_cycles(bus, 10);
            }
            0xA2 => {
                let offset = self.fetch16(bus);
                bus.write8(
                    self.overridden_address(segment_override, offset),
                    self.get_reg8(REG_AX),
                );
                self.add_cycles(bus, 10);
            }
            0xA3 => {
                let offset = self.fetch16(bus);
                bus.write16(
                    self.overridden_address(segment_override, offset),
                    self.get_reg16(REG_AX),
                );
                self.add_cycles(bus, 10);
            }
            0xB0..=0xB7 => {
                let value = self.fetch8(bus);
                self.set_reg8(opcode - 0xB0, value);
                self.add_cycles(bus, 4);
            }
            0xB8..=0xBF => {
                let value = self.fetch16(bus);
                self.set_reg16(opcode - 0xB8, value);
                self.add_cycles(bus, 4);
            }
            0xC2 => {
                let add = self.fetch16(bus);
                self.ip = self.pop16(bus);
                self.set_reg16(REG_SP, self.get_reg16(REG_SP).wrapping_add(add));
                self.add_cycles(bus, 6);
            }
            0xC0 => {
                let modrm = self.fetch_modrm(bus);
                let operand = self.decode_rm_operand(modrm, segment_override, bus);
                let count = self.fetch8(bus);
                self.shift_rotate_operand8(modrm, operand, count, bus);
            }
            0xC1 => {
                let modrm = self.fetch_modrm(bus);
                let operand = self.decode_rm_operand(modrm, segment_override, bus);
                let count = self.fetch8(bus);
                self.shift_rotate_operand16(modrm, operand, count, bus);
            }
            0xC3 => {
                self.ip = self.pop16(bus);
                self.add_cycles(bus, 6);
            }
            0xC4 => self.load_far_pointer(SegmentRegister::Es, segment_override, bus),
            0xC5 => self.load_far_pointer(SegmentRegister::Ds, segment_override, bus),
            0xC6 => self.mov_rm_imm8(segment_override, bus),
            0xC7 => self.mov_rm_imm16(segment_override, bus),
            0xC8 => {
                let frame_size = self.fetch16(bus);
                let nesting = self.fetch8(bus);
                self.enter(frame_size, nesting, bus);
            }
            0xC9 => {
                self.set_reg16(REG_SP, self.get_reg16(REG_BP));
                let bp = self.pop16(bus);
                self.set_reg16(REG_BP, bp);
                self.add_cycles(bus, 2);
            }
            0xCA => {
                let add = self.fetch16(bus);
                self.ip = self.pop16(bus);
                let cs = self.pop16(bus);
                self.segments[SegmentRegister::Cs.index()] = cs;
                self.set_reg16(REG_SP, self.get_reg16(REG_SP).wrapping_add(add));
                self.add_cycles(bus, 9);
            }
            0xCB => {
                self.ip = self.pop16(bus);
                let cs = self.pop16(bus);
                self.segments[SegmentRegister::Cs.index()] = cs;
                self.add_cycles(bus, 8);
            }
            0xCC => self.software_interrupt(3, 9, bus),
            0xCD => {
                let vector = self.fetch8(bus);
                self.software_interrupt(vector, 10, bus);
            }
            0xCF => {
                self.ip = self.pop16(bus);
                let cs = self.pop16(bus);
                self.segments[SegmentRegister::Cs.index()] = cs;
                self.flags = self.pop16(bus) | FLAG_FIXED;
                self.add_cycles(bus, 10);
            }
            0xD0 => {
                let modrm = self.fetch_modrm(bus);
                self.shift_rotate_rm8(modrm, 1, segment_override, bus);
            }
            0xD1 => {
                let modrm = self.fetch_modrm(bus);
                self.shift_rotate_rm16(modrm, 1, segment_override, bus);
            }
            0xD2 => {
                let modrm = self.fetch_modrm(bus);
                self.shift_rotate_rm8(modrm, self.get_reg8(REG_CX), segment_override, bus);
            }
            0xD3 => {
                let modrm = self.fetch_modrm(bus);
                self.shift_rotate_rm16(modrm, self.get_reg8(REG_CX), segment_override, bus);
            }
            0xD4 => {
                let base = self.fetch8(bus);
                let al = self.get_reg8(REG_AX);
                if base == 0 {
                    self.divide_error(0xD4, base);
                    return;
                }
                self.set_reg8(REG_AX + 4, al / base);
                self.set_reg8(REG_AX, al % base);
                self.set_logic_flags8(self.get_reg8(REG_AX));
                self.add_cycles(bus, 16);
            }
            0xD5 => {
                let base = self.fetch8(bus);
                let al = self.get_reg8(REG_AX);
                let ah = self.get_reg8(REG_AX + 4);
                let result = ah.wrapping_mul(base).wrapping_add(al);
                self.set_reg8(REG_AX, result);
                self.set_reg8(REG_AX + 4, 0);
                self.set_logic_flags8(result);
                self.add_cycles(bus, 4);
            }
            0xD6 => {
                self.set_reg8(REG_AX, if self.carry_set() { 0xFF } else { 0x00 });
                self.add_cycles(bus, 2);
            }
            0xD7 => self.xlat(segment_override, bus),
            0xE0 => self.loop_rel8(false, bus),
            0xE1 => self.loop_rel8(true, bus),
            0xE2 => self.loop_rel8_any(bus),
            0xE3 => {
                let rel = self.fetch8(bus) as i8;
                if self.get_reg16(REG_CX) == 0 {
                    self.ip = self.ip.wrapping_add_signed(i16::from(rel));
                }
                self.add_cycles(bus, 4);
            }
            0xE4 => {
                let port = u16::from(self.fetch8(bus));
                let value = bus.io_read8(port);
                self.set_reg8(REG_AX, value);
                self.add_cycles(bus, 8);
            }
            0xE5 => {
                let port = u16::from(self.fetch8(bus));
                let value = bus.io_read16(port);
                self.set_reg16(REG_AX, value);
                self.add_cycles(bus, 8);
            }
            0xE6 => {
                let port = u16::from(self.fetch8(bus));
                bus.io_write8(port, self.get_reg8(REG_AX));
                self.add_cycles(bus, 8);
            }
            0xE7 => {
                let port = u16::from(self.fetch8(bus));
                bus.io_write16(port, self.get_reg16(REG_AX));
                self.add_cycles(bus, 8);
            }
            0xE8 => {
                let rel = self.fetch16(bus) as i16;
                self.push16(self.ip, bus);
                self.ip = self.ip.wrapping_add_signed(rel);
                self.add_cycles(bus, 5);
            }
            0xE9 => {
                let rel = self.fetch16(bus) as i16;
                self.ip = self.ip.wrapping_add_signed(rel);
                self.add_cycles(bus, 4);
            }
            0xEA => {
                let ip = self.fetch16(bus);
                let cs = self.fetch16(bus);
                self.ip = ip;
                self.segments[SegmentRegister::Cs.index()] = cs;
                self.add_cycles(bus, 8);
            }
            0xEB => {
                let rel = self.fetch8(bus) as i8;
                self.ip = self.ip.wrapping_add_signed(i16::from(rel));
                self.add_cycles(bus, 4);
            }
            0xEC => {
                let value = bus.io_read8(self.get_reg16(REG_DX));
                self.set_reg8(REG_AX, value);
                self.add_cycles(bus, 8);
            }
            0xED => {
                let value = bus.io_read16(self.get_reg16(REG_DX));
                self.set_reg16(REG_AX, value);
                self.add_cycles(bus, 8);
            }
            0xEE => {
                bus.io_write8(self.get_reg16(REG_DX), self.get_reg8(REG_AX));
                self.add_cycles(bus, 8);
            }
            0xEF => {
                bus.io_write16(self.get_reg16(REG_DX), self.get_reg16(REG_AX));
                self.add_cycles(bus, 8);
            }
            0xF4 => {
                self.state = CpuState::Halted;
                self.add_cycles(bus, 2);
            }
            0xF6 => self.group_f6(segment_override, bus),
            0xF7 => self.group_f7(segment_override, bus),
            0xF5 => {
                self.flags ^= FLAG_CF;
                self.flags |= FLAG_FIXED;
                self.add_cycles(bus, 2);
            }
            0xF8 => {
                self.flags &= !FLAG_CF;
                self.flags |= FLAG_FIXED;
                self.add_cycles(bus, 2);
            }
            0xF9 => {
                self.flags |= FLAG_CF | FLAG_FIXED;
                self.add_cycles(bus, 2);
            }
            0xFA => {
                self.flags &= !FLAG_IF;
                self.flags |= FLAG_FIXED;
                self.add_cycles(bus, 2);
            }
            0xFB => {
                self.flags |= FLAG_IF | FLAG_FIXED;
                self.add_cycles(bus, 2);
            }
            0xFC => {
                self.flags &= !FLAG_DF;
                self.flags |= FLAG_FIXED;
                self.add_cycles(bus, 2);
            }
            0xFD => {
                self.flags |= FLAG_DF | FLAG_FIXED;
                self.add_cycles(bus, 2);
            }
            0xFE => self.group_fe(segment_override, bus),
            0xFF => self.group_ff(segment_override, bus),
            _ => self.unsupported_opcode(opcode),
        }
    }
}
