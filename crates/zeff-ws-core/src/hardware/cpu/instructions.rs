use super::alu::alu_group_op;
use super::*;

impl Cpu {
    pub(super) fn lea_reg_m(&mut self, segment_override: Option<SegmentRegister>, bus: &mut Bus) {
        let modrm = self.fetch_modrm(bus);
        if modrm.mode == 0b11 {
            self.unsupported_form(0x8D, modrm.byte);
            return;
        }
        let (offset, _) = self.decode_rm_effective_offset(modrm, bus);
        let _ = segment_override;
        self.set_reg16(modrm.reg, offset);
        self.add_cycles(bus, 4);
    }

    pub(super) fn pop_rm16(&mut self, segment_override: Option<SegmentRegister>, bus: &mut Bus) {
        let modrm = self.fetch_modrm(bus);
        if modrm.reg != 0 {
            self.unsupported_form(0x8F, modrm.byte);
            return;
        }
        let operand = self.decode_rm_operand(modrm, segment_override, bus);
        let value = self.pop16(bus);
        self.write_operand16(operand, value, bus);
        self.add_cycles(bus, 8);
    }

    pub(super) fn imul_reg_rm_imm16(
        &mut self,
        sign_extend_imm8: bool,
        segment_override: Option<SegmentRegister>,
        bus: &mut Bus,
    ) {
        let modrm = self.fetch_modrm(bus);
        let operand = self.decode_rm_operand(modrm, segment_override, bus);
        let lhs = i32::from(self.read_operand16(operand, bus) as i16);
        let rhs = if sign_extend_imm8 {
            i32::from(self.fetch8(bus) as i8)
        } else {
            i32::from(self.fetch16(bus) as i16)
        };
        let result = lhs.wrapping_mul(rhs);
        self.set_reg16(modrm.reg, result as u16);
        self.set_mul_flags(result < i32::from(i16::MIN) || result > i32::from(i16::MAX));
        self.add_cycles(bus, 16);
    }

    pub(super) fn enter(&mut self, frame_size: u16, nesting: u8, bus: &mut Bus) {
        self.push16(self.get_reg16(REG_BP), bus);
        let frame_temp = self.get_reg16(REG_SP);
        let nesting = nesting & 0x1F;
        if nesting != 0 {
            for _ in 1..nesting {
                let bp = self.get_reg16(REG_BP).wrapping_sub(2);
                self.set_reg16(REG_BP, bp);
                let addr = self.physical_address(SegmentRegister::Ss, bp);
                self.push16(bus.read16(addr), bus);
            }
            self.push16(frame_temp, bus);
        }
        self.set_reg16(REG_BP, frame_temp);
        self.set_reg16(REG_SP, self.get_reg16(REG_SP).wrapping_sub(frame_size));
        self.add_cycles(bus, 12 + u32::from(nesting) * 4);
    }

    pub(super) fn load_far_pointer(
        &mut self,
        segment: SegmentRegister,
        segment_override: Option<SegmentRegister>,
        bus: &mut Bus,
    ) {
        let modrm = self.fetch_modrm(bus);
        if modrm.mode == 0b11 {
            self.unsupported_form(
                if segment == SegmentRegister::Es {
                    0xC4
                } else {
                    0xC5
                },
                modrm.byte,
            );
            return;
        }
        let operand = self.decode_rm_operand(modrm, segment_override, bus);
        let Operand::Memory(addr) = operand else {
            unreachable!("register form rejected above");
        };
        let offset = bus.read16(addr);
        let seg_value = bus.read16(addr.wrapping_add(2));
        self.set_reg16(modrm.reg, offset);
        self.segments[segment.index()] = seg_value;
        self.add_cycles(bus, 8);
    }

    pub(super) fn xlat(&mut self, segment_override: Option<SegmentRegister>, bus: &mut Bus) {
        let offset = self
            .get_reg16(REG_BX)
            .wrapping_add(u16::from(self.get_reg8(REG_AX)));
        let value = bus.read8(self.overridden_address(segment_override, offset));
        self.set_reg8(REG_AX, value);
        self.add_cycles(bus, 5);
    }

    pub(super) fn group_f6(&mut self, segment_override: Option<SegmentRegister>, bus: &mut Bus) {
        let modrm = self.fetch_modrm(bus);
        let operand = self.decode_rm_operand(modrm, segment_override, bus);
        match modrm.reg {
            0 | 1 => {
                let lhs = self.read_operand8(operand, bus);
                let rhs = self.fetch8(bus);
                self.alu8(AluOp::And, lhs, rhs);
                self.add_cycles(bus, 4);
            }
            2 => {
                let value = self.read_operand8(operand, bus);
                self.write_operand8(operand, !value, bus);
                self.add_cycles(bus, 4);
            }
            3 => {
                let value = self.read_operand8(operand, bus);
                let result = value.wrapping_neg();
                self.set_sub_flags8(0, value, result);
                self.set_carry(value != 0);
                self.write_operand8(operand, result, bus);
                self.add_cycles(bus, 4);
            }
            4 => {
                let rhs = u16::from(self.read_operand8(operand, bus));
                let result = u16::from(self.get_reg8(REG_AX)) * rhs;
                self.set_reg16(REG_AX, result);
                self.set_mul_flags(result > 0x00FF);
                self.add_cycles(bus, 20);
            }
            5 => {
                let lhs = i16::from(self.get_reg8(REG_AX) as i8);
                let rhs = i16::from(self.read_operand8(operand, bus) as i8);
                let result = lhs.wrapping_mul(rhs);
                self.set_reg16(REG_AX, result as u16);
                self.set_mul_flags(result < i16::from(i8::MIN) || result > i16::from(i8::MAX));
                self.add_cycles(bus, 20);
            }
            6 => {
                let divisor = self.read_operand8(operand, bus);
                if divisor == 0 {
                    self.divide_error(0xF6, modrm.byte);
                    return;
                }
                let dividend = self.get_reg16(REG_AX);
                let quotient = dividend / u16::from(divisor);
                if quotient > 0x00FF {
                    self.divide_error(0xF6, modrm.byte);
                    return;
                }
                let remainder = dividend % u16::from(divisor);
                self.set_reg8(REG_AX, quotient as u8);
                self.set_reg8(REG_AX | 0x04, remainder as u8);
                self.add_cycles(bus, 24);
            }
            7 => {
                let divisor = self.read_operand8(operand, bus) as i8;
                if divisor == 0 {
                    self.divide_error(0xF6, modrm.byte);
                    return;
                }
                let dividend = self.get_reg16(REG_AX) as i16;
                let divisor = i16::from(divisor);
                if dividend == i16::MIN && divisor == -1 {
                    self.divide_error(0xF6, modrm.byte);
                    return;
                }
                let quotient = dividend / divisor;
                if quotient < i16::from(i8::MIN) || quotient > i16::from(i8::MAX) {
                    self.divide_error(0xF6, modrm.byte);
                    return;
                }
                let remainder = dividend % divisor;
                self.set_reg8(REG_AX, quotient as i8 as u8);
                self.set_reg8(REG_AX | 0x04, remainder as i8 as u8);
                self.add_cycles(bus, 24);
            }
            _ => self.unsupported_form(0xF6, modrm.byte),
        }
    }

    pub(super) fn group_f7(&mut self, segment_override: Option<SegmentRegister>, bus: &mut Bus) {
        let modrm = self.fetch_modrm(bus);
        let operand = self.decode_rm_operand(modrm, segment_override, bus);
        match modrm.reg {
            0 | 1 => {
                let lhs = self.read_operand16(operand, bus);
                let rhs = self.fetch16(bus);
                self.alu16(AluOp::And, lhs, rhs);
                self.add_cycles(bus, 4);
            }
            2 => {
                let value = self.read_operand16(operand, bus);
                self.write_operand16(operand, !value, bus);
                self.add_cycles(bus, 4);
            }
            3 => {
                let value = self.read_operand16(operand, bus);
                let result = value.wrapping_neg();
                self.set_sub_flags16(0, value, result);
                self.set_carry(value != 0);
                self.write_operand16(operand, result, bus);
                self.add_cycles(bus, 4);
            }
            4 => {
                let lhs = u32::from(self.get_reg16(REG_AX));
                let rhs = u32::from(self.read_operand16(operand, bus));
                let result = lhs * rhs;
                self.set_reg16(REG_AX, result as u16);
                self.set_reg16(REG_DX, (result >> 16) as u16);
                self.set_mul_flags((result >> 16) != 0);
                self.add_cycles(bus, 28);
            }
            5 => {
                let lhs = i32::from(self.get_reg16(REG_AX) as i16);
                let rhs = i32::from(self.read_operand16(operand, bus) as i16);
                let result = lhs.wrapping_mul(rhs);
                self.set_reg16(REG_AX, result as u16);
                self.set_reg16(REG_DX, (result >> 16) as u16);
                self.set_mul_flags(result < i32::from(i16::MIN) || result > i32::from(i16::MAX));
                self.add_cycles(bus, 28);
            }
            6 => {
                let divisor = self.read_operand16(operand, bus);
                if divisor == 0 {
                    self.divide_error(0xF7, modrm.byte);
                    return;
                }
                let dividend =
                    (u32::from(self.get_reg16(REG_DX)) << 16) | u32::from(self.get_reg16(REG_AX));
                let quotient = dividend / u32::from(divisor);
                if quotient > 0xFFFF {
                    self.divide_error(0xF7, modrm.byte);
                    return;
                }
                let remainder = dividend % u32::from(divisor);
                self.set_reg16(REG_AX, quotient as u16);
                self.set_reg16(REG_DX, remainder as u16);
                self.add_cycles(bus, 32);
            }
            7 => {
                let divisor = self.read_operand16(operand, bus) as i16;
                if divisor == 0 {
                    self.divide_error(0xF7, modrm.byte);
                    return;
                }
                let dividend = ((i32::from(self.get_reg16(REG_DX) as i16)) << 16)
                    | i32::from(self.get_reg16(REG_AX));
                let divisor = i32::from(divisor);
                if dividend == i32::MIN && divisor == -1 {
                    self.divide_error(0xF7, modrm.byte);
                    return;
                }
                let quotient = dividend / divisor;
                if quotient < i32::from(i16::MIN) || quotient > i32::from(i16::MAX) {
                    self.divide_error(0xF7, modrm.byte);
                    return;
                }
                let remainder = dividend % divisor;
                self.set_reg16(REG_AX, quotient as i16 as u16);
                self.set_reg16(REG_DX, remainder as i16 as u16);
                self.add_cycles(bus, 32);
            }
            _ => self.unsupported_form(0xF7, modrm.byte),
        }
    }

    pub(super) fn group_fe(&mut self, segment_override: Option<SegmentRegister>, bus: &mut Bus) {
        let modrm = self.fetch_modrm(bus);
        let operand = self.decode_rm_operand(modrm, segment_override, bus);
        let value = self.read_operand8(operand, bus);
        let result = match modrm.reg {
            0 => {
                let result = value.wrapping_add(1);
                self.set_inc_dec_flags8(result, false);
                result
            }
            1 => {
                let result = value.wrapping_sub(1);
                self.set_inc_dec_flags8(result, true);
                result
            }
            _ => {
                self.unsupported_form(0xFE, modrm.byte);
                return;
            }
        };
        self.write_operand8(operand, result, bus);
        self.add_cycles(bus, 4);
    }

    pub(super) fn group_ff(&mut self, segment_override: Option<SegmentRegister>, bus: &mut Bus) {
        let modrm = self.fetch_modrm(bus);
        let operand = self.decode_rm_operand(modrm, segment_override, bus);
        match modrm.reg {
            0 => {
                let value = self.read_operand16(operand, bus);
                let result = value.wrapping_add(1);
                self.set_inc_dec_flags16(result, false);
                self.write_operand16(operand, result, bus);
                self.add_cycles(bus, 4);
            }
            1 => {
                let value = self.read_operand16(operand, bus);
                let result = value.wrapping_sub(1);
                self.set_inc_dec_flags16(result, true);
                self.write_operand16(operand, result, bus);
                self.add_cycles(bus, 4);
            }
            2 => {
                let target = self.read_operand16(operand, bus);
                self.push16(self.ip, bus);
                self.ip = target;
                self.add_cycles(
                    bus,
                    if matches!(operand, Operand::Memory(_)) {
                        6
                    } else {
                        5
                    },
                );
            }
            3 => {
                let Operand::Memory(addr) = operand else {
                    self.unsupported_form(0xFF, modrm.byte);
                    return;
                };
                let ip = bus.read16(addr);
                let cs = bus.read16(addr.wrapping_add(2));
                self.push16(self.segments[SegmentRegister::Cs.index()], bus);
                self.push16(self.ip, bus);
                self.ip = ip;
                self.segments[SegmentRegister::Cs.index()] = cs;
                self.add_cycles(bus, 12);
            }
            4 => {
                self.ip = self.read_operand16(operand, bus);
                self.add_cycles(
                    bus,
                    if matches!(operand, Operand::Memory(_)) {
                        5
                    } else {
                        4
                    },
                );
            }
            5 => {
                let Operand::Memory(addr) = operand else {
                    self.unsupported_form(0xFF, modrm.byte);
                    return;
                };
                let ip = bus.read16(addr);
                let cs = bus.read16(addr.wrapping_add(2));
                self.ip = ip;
                self.segments[SegmentRegister::Cs.index()] = cs;
                self.add_cycles(bus, 10);
            }
            6 => {
                let value = self.read_operand16(operand, bus);
                self.push16(value, bus);
                self.add_cycles(
                    bus,
                    if matches!(operand, Operand::Memory(_)) {
                        2
                    } else {
                        1
                    },
                );
            }
            _ => self.unsupported_form(0xFF, modrm.byte),
        }
    }
    pub(super) fn software_interrupt(&mut self, vector: u8, cycles: u32, bus: &mut Bus) {
        self.enter_interrupt(vector, cycles, bus);
    }

    pub(super) fn loop_rel8(&mut self, loop_if_zero: bool, bus: &mut Bus) {
        let rel = self.fetch8(bus) as i8;
        let cx = self.get_reg16(REG_CX).wrapping_sub(1);
        self.set_reg16(REG_CX, cx);
        let zf = self.flags & FLAG_ZF != 0;
        if cx != 0 && zf == loop_if_zero {
            self.ip = self.ip.wrapping_add_signed(i16::from(rel));
        }
        self.add_cycles(bus, 4);
    }

    pub(super) fn loop_rel8_any(&mut self, bus: &mut Bus) {
        let rel = self.fetch8(bus) as i8;
        let cx = self.get_reg16(REG_CX).wrapping_sub(1);
        self.set_reg16(REG_CX, cx);
        if cx != 0 {
            self.ip = self.ip.wrapping_add_signed(i16::from(rel));
        }
        self.add_cycles(bus, 4);
    }

    pub(super) fn test_rm_reg8(
        &mut self,
        segment_override: Option<SegmentRegister>,
        bus: &mut Bus,
    ) {
        let modrm = self.fetch_modrm(bus);
        let operand = self.decode_rm_operand(modrm, segment_override, bus);
        let lhs = self.read_operand8(operand, bus);
        let rhs = self.get_reg8(modrm.reg);
        self.alu8(AluOp::And, lhs, rhs);
        self.add_cycles(bus, 4);
    }

    pub(super) fn test_rm_reg16(
        &mut self,
        segment_override: Option<SegmentRegister>,
        bus: &mut Bus,
    ) {
        let modrm = self.fetch_modrm(bus);
        let operand = self.decode_rm_operand(modrm, segment_override, bus);
        let lhs = self.read_operand16(operand, bus);
        let rhs = self.get_reg16(modrm.reg);
        self.alu16(AluOp::And, lhs, rhs);
        self.add_cycles(bus, 4);
    }

    pub(super) fn xchg_rm_reg8(
        &mut self,
        segment_override: Option<SegmentRegister>,
        bus: &mut Bus,
    ) {
        let modrm = self.fetch_modrm(bus);
        let operand = self.decode_rm_operand(modrm, segment_override, bus);
        let lhs = self.read_operand8(operand, bus);
        let rhs = self.get_reg8(modrm.reg);
        self.write_operand8(operand, rhs, bus);
        self.set_reg8(modrm.reg, lhs);
        self.add_cycles(bus, 4);
    }

    pub(super) fn xchg_rm_reg16(
        &mut self,
        segment_override: Option<SegmentRegister>,
        bus: &mut Bus,
    ) {
        let modrm = self.fetch_modrm(bus);
        let operand = self.decode_rm_operand(modrm, segment_override, bus);
        let lhs = self.read_operand16(operand, bus);
        let rhs = self.get_reg16(modrm.reg);
        self.write_operand16(operand, rhs, bus);
        self.set_reg16(modrm.reg, lhs);
        self.add_cycles(bus, 4);
    }

    pub(super) fn mov_rm_reg8(&mut self, segment_override: Option<SegmentRegister>, bus: &mut Bus) {
        let modrm = self.fetch_modrm(bus);
        let operand = self.decode_rm_operand(modrm, segment_override, bus);
        self.write_operand8(operand, self.get_reg8(modrm.reg), bus);
        self.add_cycles(bus, 4);
    }

    pub(super) fn mov_rm_reg16(
        &mut self,
        segment_override: Option<SegmentRegister>,
        bus: &mut Bus,
    ) {
        let modrm = self.fetch_modrm(bus);
        let operand = self.decode_rm_operand(modrm, segment_override, bus);
        self.write_operand16(operand, self.get_reg16(modrm.reg), bus);
        self.add_cycles(bus, 4);
    }

    pub(super) fn mov_reg_rm8(&mut self, segment_override: Option<SegmentRegister>, bus: &mut Bus) {
        let modrm = self.fetch_modrm(bus);
        let operand = self.decode_rm_operand(modrm, segment_override, bus);
        let value = self.read_operand8(operand, bus);
        self.set_reg8(modrm.reg, value);
        self.add_cycles(bus, 4);
    }

    pub(super) fn mov_reg_rm16(
        &mut self,
        segment_override: Option<SegmentRegister>,
        bus: &mut Bus,
    ) {
        let modrm = self.fetch_modrm(bus);
        let operand = self.decode_rm_operand(modrm, segment_override, bus);
        let value = self.read_operand16(operand, bus);
        self.set_reg16(modrm.reg, value);
        self.add_cycles(bus, 4);
    }

    pub(super) fn mov_rm_sreg(&mut self, segment_override: Option<SegmentRegister>, bus: &mut Bus) {
        let modrm = self.fetch_modrm(bus);
        let operand = self.decode_rm_operand(modrm, segment_override, bus);
        let segment = SegmentRegister::from_modrm_reg(modrm.reg);
        self.write_operand16(operand, self.segments[segment.index()], bus);
        self.add_cycles(bus, 4);
    }

    pub(super) fn mov_sreg_rm(&mut self, segment_override: Option<SegmentRegister>, bus: &mut Bus) {
        let modrm = self.fetch_modrm(bus);
        let operand = self.decode_rm_operand(modrm, segment_override, bus);
        let segment = SegmentRegister::from_modrm_reg(modrm.reg);
        let value = self.read_operand16(operand, bus);
        self.segments[segment.index()] = value;
        self.add_cycles(bus, 4);
    }

    pub(super) fn mov_rm_imm8(&mut self, segment_override: Option<SegmentRegister>, bus: &mut Bus) {
        let modrm = self.fetch_modrm(bus);
        if modrm.reg != 0 {
            self.unsupported_form(0xC6, modrm.byte);
            return;
        }
        let operand = self.decode_rm_operand(modrm, segment_override, bus);
        let value = self.fetch8(bus);
        self.write_operand8(operand, value, bus);
        self.add_cycles(bus, 4);
    }

    pub(super) fn mov_rm_imm16(
        &mut self,
        segment_override: Option<SegmentRegister>,
        bus: &mut Bus,
    ) {
        let modrm = self.fetch_modrm(bus);
        if modrm.reg != 0 {
            self.unsupported_form(0xC7, modrm.byte);
            return;
        }
        let operand = self.decode_rm_operand(modrm, segment_override, bus);
        let value = self.fetch16(bus);
        self.write_operand16(operand, value, bus);
        self.add_cycles(bus, 4);
    }

    pub(super) fn alu_rm_reg8(
        &mut self,
        op: AluOp,
        segment_override: Option<SegmentRegister>,
        bus: &mut Bus,
    ) {
        let modrm = self.fetch_modrm(bus);
        let operand = self.decode_rm_operand(modrm, segment_override, bus);
        let lhs = self.read_operand8(operand, bus);
        let rhs = self.get_reg8(modrm.reg);
        let result = self.alu8(op, lhs, rhs);
        if op != AluOp::Cmp {
            self.write_operand8(operand, result, bus);
        }
        self.add_cycles(bus, 4);
    }

    pub(super) fn alu_rm_reg16(
        &mut self,
        op: AluOp,
        segment_override: Option<SegmentRegister>,
        bus: &mut Bus,
    ) {
        let modrm = self.fetch_modrm(bus);
        let operand = self.decode_rm_operand(modrm, segment_override, bus);
        let lhs = self.read_operand16(operand, bus);
        let rhs = self.get_reg16(modrm.reg);
        let result = self.alu16(op, lhs, rhs);
        if op != AluOp::Cmp {
            self.write_operand16(operand, result, bus);
        }
        self.add_cycles(bus, 4);
    }

    pub(super) fn alu_reg_rm8(
        &mut self,
        op: AluOp,
        segment_override: Option<SegmentRegister>,
        bus: &mut Bus,
    ) {
        let modrm = self.fetch_modrm(bus);
        let operand = self.decode_rm_operand(modrm, segment_override, bus);
        let lhs = self.get_reg8(modrm.reg);
        let rhs = self.read_operand8(operand, bus);
        let result = self.alu8(op, lhs, rhs);
        if op != AluOp::Cmp {
            self.set_reg8(modrm.reg, result);
        }
        self.add_cycles(bus, 4);
    }

    pub(super) fn alu_reg_rm16(
        &mut self,
        op: AluOp,
        segment_override: Option<SegmentRegister>,
        bus: &mut Bus,
    ) {
        let modrm = self.fetch_modrm(bus);
        let operand = self.decode_rm_operand(modrm, segment_override, bus);
        let lhs = self.get_reg16(modrm.reg);
        let rhs = self.read_operand16(operand, bus);
        let result = self.alu16(op, lhs, rhs);
        if op != AluOp::Cmp {
            self.set_reg16(modrm.reg, result);
        }
        self.add_cycles(bus, 4);
    }

    pub(super) fn alu_rm_imm8(
        &mut self,
        signed_imm: bool,
        segment_override: Option<SegmentRegister>,
        bus: &mut Bus,
    ) {
        self.alu_rm_imm8_with_opcode(0x80, signed_imm, segment_override, bus);
    }

    pub(super) fn alu_rm_imm8_with_opcode(
        &mut self,
        opcode: u8,
        signed_imm: bool,
        segment_override: Option<SegmentRegister>,
        bus: &mut Bus,
    ) {
        let modrm = self.fetch_modrm(bus);
        let Some(op) = alu_group_op(modrm.reg) else {
            self.unsupported_form(opcode, modrm.byte);
            return;
        };
        let operand = self.decode_rm_operand(modrm, segment_override, bus);
        let lhs = self.read_operand8(operand, bus);
        let imm = self.fetch8(bus);
        let rhs = if signed_imm { imm as i8 as u8 } else { imm };
        let result = self.alu8(op, lhs, rhs);
        if op != AluOp::Cmp {
            self.write_operand8(operand, result, bus);
        }
        self.add_cycles(bus, 4);
    }

    pub(super) fn alu_rm_imm16(
        &mut self,
        signed_imm: bool,
        segment_override: Option<SegmentRegister>,
        bus: &mut Bus,
    ) {
        let opcode = if signed_imm { 0x83 } else { 0x81 };
        let modrm = self.fetch_modrm(bus);
        let Some(op) = alu_group_op(modrm.reg) else {
            self.unsupported_form(opcode, modrm.byte);
            return;
        };
        let operand = self.decode_rm_operand(modrm, segment_override, bus);
        let lhs = self.read_operand16(operand, bus);
        let rhs = if signed_imm {
            self.fetch8(bus) as i8 as i16 as u16
        } else {
            self.fetch16(bus)
        };
        let result = self.alu16(op, lhs, rhs);
        if op != AluOp::Cmp {
            self.write_operand16(operand, result, bus);
        }
        self.add_cycles(bus, 4);
    }

    pub(super) fn push_segment(&mut self, segment: SegmentRegister, bus: &mut Bus) {
        self.push16(self.segments[segment.index()], bus);
        self.add_cycles(bus, 2);
    }

    pub(super) fn pop_segment(&mut self, segment: SegmentRegister, bus: &mut Bus) {
        let value = self.pop16(bus);
        self.segments[segment.index()] = value;
        self.add_cycles(bus, 3);
    }
}
