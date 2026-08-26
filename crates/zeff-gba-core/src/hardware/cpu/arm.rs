use super::ops::{
    MultiplyCarryKind, add_overflow, arm_immediate_operand, arm7tdmi_multiply_carry, rotate_right,
    shift_operand, sign_extend, sub_overflow,
};
use super::*;

impl Cpu {
    pub(super) fn execute_arm_data_processing(&mut self, pc: u32, raw: u32) {
        if self.execute_arm_psr_transfer(raw) {
            return;
        }

        let opcode = ((raw >> 21) & 0xF) as u8;
        let set_flags = raw & (1 << 20) != 0;
        let rn = ((raw >> 16) & 0xF) as usize;
        let rd = ((raw >> 12) & 0xF) as usize;
        let register_shift = raw & (1 << 25) == 0 && raw & (1 << 4) != 0;
        let lhs = if register_shift && rn == 15 {
            pc.wrapping_add(12)
        } else {
            self.reg_read_arm(rn, pc)
        };
        let (rhs, shifter_carry) = if raw & (1 << 25) != 0 {
            arm_immediate_operand(raw, self.carry())
        } else {
            self.arm_register_operand(raw, pc)
        };

        let write_result = !matches!(opcode, 0x8..=0xB);
        let result = match opcode {
            0x0 | 0x8 => lhs & rhs,
            0x1 | 0x9 => lhs ^ rhs,
            0x2 | 0xA => lhs.wrapping_sub(rhs),
            0x3 => rhs.wrapping_sub(lhs),
            0x4 | 0xB => lhs.wrapping_add(rhs),
            0x5 => lhs.wrapping_add(rhs).wrapping_add(u32::from(self.carry())),
            0x6 => lhs.wrapping_sub(rhs).wrapping_sub(u32::from(!self.carry())),
            0x7 => rhs.wrapping_sub(lhs).wrapping_sub(u32::from(!self.carry())),
            0xC => lhs | rhs,
            0xD => rhs,
            0xE => lhs & !rhs,
            0xF => !rhs,
            _ => return,
        };

        if set_flags || !write_result {
            match opcode {
                0x0 | 0x1 | 0x8 | 0x9 | 0xC | 0xD | 0xE | 0xF => {
                    self.set_nzc(result, shifter_carry)
                }
                0x2 | 0xA => self.set_nzcv(result, lhs >= rhs, sub_overflow(lhs, rhs, result)),
                0x3 => self.set_nzcv(result, rhs >= lhs, sub_overflow(rhs, lhs, result)),
                0x4 | 0xB => {
                    let (sum, carry) = lhs.overflowing_add(rhs);
                    self.set_nzcv(sum, carry, add_overflow(lhs, rhs, sum));
                }
                0x5 => {
                    let carry_in = u32::from(self.carry());
                    let (sum1, c1) = lhs.overflowing_add(rhs);
                    let (sum2, c2) = sum1.overflowing_add(carry_in);
                    self.set_nzcv(sum2, c1 || c2, add_overflow(lhs, rhs, sum2));
                }
                0x6 => {
                    let borrow = u32::from(!self.carry());
                    let carry = u64::from(lhs) >= u64::from(rhs) + u64::from(borrow);
                    self.set_nzcv(result, carry, sub_overflow(lhs, rhs, result));
                }
                0x7 => {
                    let borrow = u32::from(!self.carry());
                    let carry = u64::from(rhs) >= u64::from(lhs) + u64::from(borrow);
                    self.set_nzcv(result, carry, sub_overflow(rhs, lhs, result));
                }
                _ => {}
            }
            if !write_result && rd == 15 && mode_has_spsr(self.mode()) {
                self.set_cpsr(self.spsr);
            }
        }

        if write_result {
            if set_flags && rd == 15 {
                self.return_from_exception(result, false);
            } else {
                self.write_reg(rd, result, false);
            }
        }
    }

    fn execute_arm_psr_transfer(&mut self, raw: u32) -> bool {
        if raw & 0x0FBF_0FFF == 0x010F_0000 {
            let rd = ((raw >> 12) & 0xF) as usize;
            self.regs[rd] = if raw & (1 << 22) != 0 {
                self.spsr
            } else {
                self.cpsr
            };
            return true;
        }

        if raw & 0x0FB0_FFF0 == 0x0120_F000 {
            let rm = (raw & 0xF) as usize;
            self.write_psr(raw, self.regs[rm]);
            return true;
        }

        if raw & 0x0FB0_F000 == 0x0320_F000 {
            let (value, _) = arm_immediate_operand(raw, self.carry());
            self.write_psr(raw, value);
            return true;
        }

        false
    }

    fn write_psr(&mut self, raw: u32, value: u32) {
        let field_mask = (raw >> 16) & 0xF;
        let mut mask = 0u32;
        if field_mask & 0x1 != 0 {
            mask |= 0x0000_00FF;
        }
        if field_mask & 0x2 != 0 {
            mask |= 0x0000_FF00;
        }
        if field_mask & 0x4 != 0 {
            mask |= 0x00FF_0000;
        }
        if field_mask & 0x8 != 0 {
            mask |= 0xFF00_0000;
        }

        if raw & (1 << 22) != 0 {
            self.spsr = (self.spsr & !mask) | (value & mask);
        } else {
            self.set_cpsr((self.cpsr & !mask) | (value & mask));
        }
    }

    fn return_from_exception(&mut self, pc: u32, thumb: bool) {
        self.set_cpsr(self.spsr);
        self.write_reg(15, pc, thumb || self.thumb_state());
    }

    fn arm_register_operand(&self, raw: u32, pc: u32) -> (u32, bool) {
        let rm = (raw & 0xF) as usize;
        let shift_type = (raw >> 5) & 0x3;
        let by_register = raw & (1 << 4) != 0;
        let value = if by_register && rm == 15 {
            pc.wrapping_add(12)
        } else {
            self.reg_read_arm(rm, pc)
        };
        let amount = if by_register {
            let rs = ((raw >> 8) & 0xF) as usize;
            if rs == 15 {
                pc.wrapping_add(12) & 0xFF
            } else {
                self.regs[rs] & 0xFF
            }
        } else {
            (raw >> 7) & 0x1F
        };
        shift_operand(value, shift_type, amount, by_register, self.carry())
    }

    pub(super) fn execute_arm_multiply(&mut self, raw: u32) {
        let accumulate = raw & (1 << 21) != 0;
        let set_flags = raw & (1 << 20) != 0;
        let rd = ((raw >> 16) & 0xF) as usize;
        let rn = ((raw >> 12) & 0xF) as usize;
        let rs = ((raw >> 8) & 0xF) as usize;
        let rm = (raw & 0xF) as usize;
        let mut result = self.regs[rm].wrapping_mul(self.regs[rs]);
        let accumulator = if accumulate { self.regs[rn] } else { 0 };
        if accumulate {
            result = result.wrapping_add(accumulator);
        }
        self.regs[rd] = result;
        if set_flags {
            let carry = arm7tdmi_multiply_carry(
                MultiplyCarryKind::Short,
                self.regs[rm],
                self.regs[rs],
                u64::from(accumulator),
            );
            self.set_nzc(result, carry);
        }
    }

    pub(super) fn execute_arm_multiply_long(&mut self, raw: u32) {
        let signed = raw & (1 << 22) != 0;
        let accumulate = raw & (1 << 21) != 0;
        let set_flags = raw & (1 << 20) != 0;
        let rd_hi = ((raw >> 16) & 0xF) as usize;
        let rd_lo = ((raw >> 12) & 0xF) as usize;
        let rs = ((raw >> 8) & 0xF) as usize;
        let rm = (raw & 0xF) as usize;
        let rm_value = self.regs[rm];
        let rs_value = self.regs[rs];
        let mut result = if signed {
            ((rm_value as i32 as i64).wrapping_mul(rs_value as i32 as i64)) as u64
        } else {
            u64::from(rm_value).wrapping_mul(u64::from(rs_value))
        };

        let accumulator = if accumulate {
            (u64::from(self.regs[rd_hi]) << 32) | u64::from(self.regs[rd_lo])
        } else {
            0
        };
        if accumulate {
            result = result.wrapping_add(accumulator);
        }

        self.regs[rd_lo] = result as u32;
        self.regs[rd_hi] = (result >> 32) as u32;

        if set_flags {
            let carry = arm7tdmi_multiply_carry(
                if signed {
                    MultiplyCarryKind::LongSigned
                } else {
                    MultiplyCarryKind::LongUnsigned
                },
                rm_value,
                rs_value,
                accumulator,
            );
            if result & (1 << 63) != 0 {
                self.cpsr |= CPSR_NEGATIVE;
            } else {
                self.cpsr &= !CPSR_NEGATIVE;
            }
            if result == 0 {
                self.cpsr |= CPSR_ZERO;
            } else {
                self.cpsr &= !CPSR_ZERO;
            }
            self.cpsr &= !CPSR_CARRY;
            if carry {
                self.cpsr |= CPSR_CARRY;
            }
        }
    }

    pub(super) fn execute_arm_single_data_swap(&mut self, bus: &mut Bus, pc: u32, raw: u32) {
        let byte = raw & (1 << 22) != 0;
        let rn = ((raw >> 16) & 0xF) as usize;
        let rd = ((raw >> 12) & 0xF) as usize;
        let rm = (raw & 0xF) as usize;
        let addr = self.reg_read_arm(rn, pc);
        let store_value = self.reg_read_arm(rm, pc);

        let loaded = if byte {
            let value = u32::from(self.cpu_read8(bus, addr));
            self.cpu_write8(bus, addr, store_value as u8);
            value
        } else {
            let value = rotate_right(self.cpu_read32(bus, addr), (addr & 3) * 8);
            self.cpu_write32(bus, addr, store_value);
            value
        };
        self.write_reg(rd, loaded, false);
    }

    pub(super) fn execute_arm_single_data_transfer(&mut self, bus: &mut Bus, pc: u32, raw: u32) {
        if raw & 0x0E00_0090 == 0x0000_0090 && raw & 0x60 != 0 {
            self.execute_arm_halfword_data_transfer(bus, pc, raw);
            return;
        }

        let immediate_register = raw & (1 << 25) != 0;
        let pre_index = raw & (1 << 24) != 0;
        let add = raw & (1 << 23) != 0;
        let byte = raw & (1 << 22) != 0;
        let writeback = raw & (1 << 21) != 0;
        let load = raw & (1 << 20) != 0;
        let rn = ((raw >> 16) & 0xF) as usize;
        let rd = ((raw >> 12) & 0xF) as usize;
        let base = self.reg_read_arm(rn, pc);
        let offset = if immediate_register {
            self.arm_register_operand(raw, pc).0
        } else {
            raw & 0xFFF
        };
        let offset_base = if add {
            base.wrapping_add(offset)
        } else {
            base.wrapping_sub(offset)
        };
        let addr = if pre_index { offset_base } else { base };

        if load {
            let value = if byte {
                u32::from(self.cpu_read8(bus, addr))
            } else {
                rotate_right(self.cpu_read32(bus, addr), (addr & 3) * 8)
            };
            self.write_reg(rd, value, false);
        } else if byte {
            self.cpu_write8(bus, addr, self.reg_read_arm(rd, pc) as u8);
        } else {
            self.cpu_write32(
                bus,
                addr,
                self.reg_read_arm(rd, pc)
                    .wrapping_add(if rd == 15 { 4 } else { 0 }),
            );
        }

        if (!pre_index || writeback) && !(load && rn == rd) {
            self.regs[rn] = offset_base;
        }
    }

    fn execute_arm_halfword_data_transfer(&mut self, bus: &mut Bus, pc: u32, raw: u32) {
        let pre_index = raw & (1 << 24) != 0;
        let add = raw & (1 << 23) != 0;
        let immediate = raw & (1 << 22) != 0;
        let writeback = raw & (1 << 21) != 0;
        let load = raw & (1 << 20) != 0;
        let rn = ((raw >> 16) & 0xF) as usize;
        let rd = ((raw >> 12) & 0xF) as usize;
        let mode = (raw >> 5) & 0x3;
        let base = self.reg_read_arm(rn, pc);
        let offset = if immediate {
            ((raw >> 4) & 0xF0) | (raw & 0xF)
        } else {
            self.reg_read_arm((raw & 0xF) as usize, pc)
        };
        let offset_base = if add {
            base.wrapping_add(offset)
        } else {
            base.wrapping_sub(offset)
        };
        let addr = if pre_index { offset_base } else { base };

        if load {
            let value = match mode {
                0b01 => self.load_arm_unsigned_halfword(bus, addr),
                0b10 => sign_extend(u32::from(self.cpu_read8(bus, addr)), 8) as u32,
                0b11 => self.load_arm_signed_halfword(bus, addr),
                _ => 0,
            };
            self.write_reg(rd, value, false);
        } else if mode == 0b01 {
            self.cpu_write16(bus, addr, self.reg_read_arm(rd, pc) as u16);
        }

        if (!pre_index || writeback) && !(load && rn == rd) {
            self.regs[rn] = offset_base;
        }
    }

    pub(super) fn load_arm_unsigned_halfword(&mut self, bus: &Bus, addr: u32) -> u32 {
        let value = u32::from(self.cpu_read16(bus, addr));
        rotate_right(value, (addr & 1) * 8)
    }

    pub(super) fn load_arm_signed_halfword(&mut self, bus: &Bus, addr: u32) -> u32 {
        if addr & 1 != 0 {
            sign_extend(u32::from(self.cpu_read8(bus, addr)), 8) as u32
        } else {
            sign_extend(u32::from(self.cpu_read16(bus, addr)), 16) as u32
        }
    }

    pub(super) fn execute_arm_block_data_transfer(&mut self, bus: &mut Bus, pc: u32, raw: u32) {
        let pre = raw & (1 << 24) != 0;
        let up = raw & (1 << 23) != 0;
        let psr_force_user = raw & (1 << 22) != 0;
        let writeback = raw & (1 << 21) != 0;
        let load = raw & (1 << 20) != 0;
        let rn = ((raw >> 16) & 0xF) as usize;
        let reg_list = raw & 0xFFFF;
        let count = if reg_list == 0 {
            16
        } else {
            reg_list.count_ones()
        };
        let restore_cpsr = load && psr_force_user && reg_list & (1 << 15) != 0;
        let force_user_regs = psr_force_user && !restore_cpsr;
        let base = self.regs[rn];
        let mut addr = match (up, pre) {
            (true, false) => base,
            (true, true) => base.wrapping_add(4),
            (false, false) => base.wrapping_sub(4 * (count - 1)),
            (false, true) => base.wrapping_sub(4 * count),
        };
        let writeback_value = if up {
            base.wrapping_add(4 * count)
        } else {
            base.wrapping_sub(4 * count)
        };
        let first_reg = (0..16).find(|reg| reg_list & (1 << reg) != 0);
        let mut exception_return_pc = None;

        if reg_list == 0 {
            if load {
                let value = self.cpu_read32(bus, addr);
                if psr_force_user {
                    exception_return_pc = Some(value);
                } else {
                    self.write_reg(15, value, false);
                }
            } else {
                self.cpu_write32(bus, addr, pc.wrapping_add(12));
            }
        } else {
            let mut first_access = true;
            for reg in 0..16 {
                if reg_list & (1 << reg) == 0 {
                    continue;
                }
                if load {
                    let value = if first_access {
                        self.cpu_read32(bus, addr)
                    } else {
                        self.cpu_read32_sequential(bus, addr)
                    };
                    if restore_cpsr && reg == 15 {
                        exception_return_pc = Some(value);
                    } else {
                        self.block_transfer_write_reg(reg, value, force_user_regs);
                    }
                } else {
                    let mut value = self.block_transfer_read_reg(reg, pc, force_user_regs);
                    if writeback && reg == rn && first_reg != Some(reg) {
                        value = writeback_value;
                    }
                    if reg == 15 {
                        value = value.wrapping_add(4);
                    }
                    if first_access {
                        self.cpu_write32(bus, addr, value);
                    } else {
                        self.cpu_write32_sequential(bus, addr, value);
                    }
                }
                first_access = false;
                addr = addr.wrapping_add(4);
            }
        }

        if writeback && !(load && reg_list & (1 << rn) != 0) {
            self.regs[rn] = writeback_value;
        }

        if let Some(pc) = exception_return_pc {
            self.return_from_exception(pc, false);
        }
    }

    fn block_transfer_read_reg(&self, reg: usize, pc: u32, force_user: bool) -> u32 {
        if force_user && bank_index(self.mode()) != BANK_USER_SYSTEM {
            match reg {
                8..=12 if self.mode() == CpuMode::Fiq => {
                    self.banked_r8_r12[R8_R12_USER_SYSTEM_BANK][reg - 8]
                }
                13 => self.banked_sp[BANK_USER_SYSTEM],
                14 => self.banked_lr[BANK_USER_SYSTEM],
                _ => self.reg_read_arm(reg, pc),
            }
        } else {
            self.reg_read_arm(reg, pc)
        }
    }

    fn block_transfer_write_reg(&mut self, reg: usize, value: u32, force_user: bool) {
        if force_user && bank_index(self.mode()) != BANK_USER_SYSTEM {
            match reg {
                8..=12 if self.mode() == CpuMode::Fiq => {
                    self.banked_r8_r12[R8_R12_USER_SYSTEM_BANK][reg - 8] = value
                }
                13 => self.banked_sp[BANK_USER_SYSTEM] = value,
                14 => self.banked_lr[BANK_USER_SYSTEM] = value,
                _ => self.write_reg(reg, value, false),
            }
        } else {
            self.write_reg(reg, value, false);
        }
    }

    pub(super) fn execute_arm_branch_exchange(&mut self, raw: u32) {
        let rm = (raw & 0xF) as usize;
        self.branch_exchange(self.regs[rm]);
    }

    pub(super) fn execute_arm_branch(&mut self, pc: u32, raw: u32) {
        let offset = sign_extend(raw & 0x00FF_FFFF, 24) << 2;
        if raw & (1 << 24) != 0 {
            self.regs[14] = pc.wrapping_add(4);
        }
        self.set_pc(pc.wrapping_add(8).wrapping_add_signed(offset));
        self.next_fetch_sequential = false;
    }
}
