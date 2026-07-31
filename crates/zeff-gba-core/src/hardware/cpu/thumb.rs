use super::ops::{add_overflow, rotate_right, shift_operand, sign_extend, sub_overflow};
use super::*;

impl Cpu {
    pub(super) fn execute_thumb_move_shifted_register(&mut self, raw: u16) {
        let op = (raw >> 11) & 0x3;
        let offset = u32::from((raw >> 6) & 0x1F);
        let rs = ((raw >> 3) & 0x7) as usize;
        let rd = (raw & 0x7) as usize;
        let value = self.regs[rs];
        let (result, carry) = match op {
            0 => shift_operand(value, 0, offset, false, self.carry()),
            1 => shift_operand(value, 1, offset, false, self.carry()),
            2 => shift_operand(value, 2, offset, false, self.carry()),
            _ => return,
        };
        self.regs[rd] = result;
        self.set_nzc(result, carry);
    }

    pub(super) fn execute_thumb_add_subtract(&mut self, raw: u16) {
        let immediate = raw & (1 << 10) != 0;
        let subtract = raw & (1 << 9) != 0;
        let rn = ((raw >> 6) & 0x7) as usize;
        let rs = ((raw >> 3) & 0x7) as usize;
        let rd = (raw & 0x7) as usize;
        let lhs = self.regs[rs];
        let rhs = if immediate {
            u32::from((raw >> 6) & 0x7)
        } else {
            self.regs[rn]
        };
        let result = if subtract {
            lhs.wrapping_sub(rhs)
        } else {
            lhs.wrapping_add(rhs)
        };
        self.regs[rd] = result;
        if subtract {
            self.set_nzcv(result, lhs >= rhs, sub_overflow(lhs, rhs, result));
        } else {
            let (_, carry) = lhs.overflowing_add(rhs);
            self.set_nzcv(result, carry, add_overflow(lhs, rhs, result));
        }
    }

    pub(super) fn execute_thumb_immediate(&mut self, raw: u16) {
        let op = (raw >> 11) & 0x3;
        let rd = ((raw >> 8) & 0x7) as usize;
        let imm = u32::from(raw & 0xFF);
        match op {
            0 => {
                self.regs[rd] = imm;
                self.set_nz(imm);
            }
            1 => {
                let lhs = self.regs[rd];
                let result = lhs.wrapping_sub(imm);
                self.set_nzcv(result, lhs >= imm, sub_overflow(lhs, imm, result));
            }
            2 => {
                let lhs = self.regs[rd];
                let result = lhs.wrapping_add(imm);
                self.regs[rd] = result;
                let (_, carry) = lhs.overflowing_add(imm);
                self.set_nzcv(result, carry, add_overflow(lhs, imm, result));
            }
            3 => {
                let lhs = self.regs[rd];
                let result = lhs.wrapping_sub(imm);
                self.regs[rd] = result;
                self.set_nzcv(result, lhs >= imm, sub_overflow(lhs, imm, result));
            }
            _ => {}
        }
    }

    pub(super) fn execute_thumb_alu(&mut self, raw: u16) {
        let op = (raw >> 6) & 0xF;
        let rs = ((raw >> 3) & 0x7) as usize;
        let rd = (raw & 0x7) as usize;
        let lhs = self.regs[rd];
        let rhs = self.regs[rs];
        let old_carry = self.carry();
        let (result, write, flags) = match op {
            0x0 => (lhs & rhs, true, Some((false, false))),
            0x1 => (lhs ^ rhs, true, Some((false, false))),
            0x2 => {
                let (v, c) = shift_operand(lhs, 0, rhs & 0xFF, true, old_carry);
                (v, true, Some((c, false)))
            }
            0x3 => {
                let (v, c) = shift_operand(lhs, 1, rhs & 0xFF, true, old_carry);
                (v, true, Some((c, false)))
            }
            0x4 => {
                let (v, c) = shift_operand(lhs, 2, rhs & 0xFF, true, old_carry);
                (v, true, Some((c, false)))
            }
            0x5 => {
                let carry_in = u32::from(old_carry);
                let (s1, c1) = lhs.overflowing_add(rhs);
                let (s2, c2) = s1.overflowing_add(carry_in);
                (s2, true, Some((c1 || c2, add_overflow(lhs, rhs, s2))))
            }
            0x6 => {
                let borrow = u32::from(!old_carry);
                (
                    lhs.wrapping_sub(rhs).wrapping_sub(borrow),
                    true,
                    Some((
                        u64::from(lhs) >= u64::from(rhs) + u64::from(borrow),
                        sub_overflow(lhs, rhs, lhs.wrapping_sub(rhs).wrapping_sub(borrow)),
                    )),
                )
            }
            0x7 => {
                let (v, c) = shift_operand(lhs, 3, rhs & 0xFF, true, old_carry);
                (v, true, Some((c, false)))
            }
            0x8 => (lhs & rhs, false, Some((old_carry, false))),
            0x9 => (
                0u32.wrapping_sub(rhs),
                true,
                Some((rhs == 0, sub_overflow(0, rhs, 0u32.wrapping_sub(rhs)))),
            ),
            0xA => (
                lhs.wrapping_sub(rhs),
                false,
                Some((lhs >= rhs, sub_overflow(lhs, rhs, lhs.wrapping_sub(rhs)))),
            ),
            0xB => {
                let result = lhs.wrapping_add(rhs);
                let (_, carry) = lhs.overflowing_add(rhs);
                (result, false, Some((carry, add_overflow(lhs, rhs, result))))
            }
            0xC => (lhs | rhs, true, Some((old_carry, false))),
            0xD => (lhs.wrapping_mul(rhs), true, Some((old_carry, false))),
            0xE => (lhs & !rhs, true, Some((old_carry, false))),
            0xF => (!rhs, true, Some((old_carry, false))),
            _ => return,
        };
        if write {
            self.regs[rd] = result;
        }
        if let Some((carry, overflow)) = flags {
            if matches!(op, 0x5 | 0x6 | 0x9 | 0xA | 0xB) {
                self.set_nzcv(result, carry, overflow);
            } else {
                self.set_nzc(result, carry);
            }
        }
    }

    pub(super) fn execute_thumb_conditional_branch(&mut self, pc: u32, raw: u16) {
        let condition = ((raw >> 8) & 0xF) as u8;
        if condition >= 0xE || !self.condition_passed(condition) {
            return;
        }

        let offset = sign_extend(u32::from(raw & 0x00FF), 8) << 1;
        self.set_pc(pc.wrapping_add(4).wrapping_add_signed(offset));
        self.next_fetch_sequential = false;
    }

    pub(super) fn execute_thumb_unconditional_branch(&mut self, pc: u32, raw: u16) {
        let offset = sign_extend(u32::from(raw & 0x07FF), 11) << 1;
        self.set_pc(pc.wrapping_add(4).wrapping_add_signed(offset));
        self.next_fetch_sequential = false;
    }

    pub(super) fn execute_thumb_long_branch_with_link(&mut self, pc: u32, raw: u16) {
        let offset = u32::from(raw & 0x07FF);
        if raw & 0x0800 == 0 {
            self.regs[14] = pc
                .wrapping_add(4)
                .wrapping_add_signed(sign_extend(offset, 11) << 12);
        } else {
            let target = self.regs[14].wrapping_add(offset << 1);
            self.regs[14] = pc.wrapping_add(2) | 1;
            self.set_pc(target & !1);
            self.next_fetch_sequential = false;
        }
    }

    pub(super) fn execute_thumb_hi_register_branch_exchange(&mut self, pc: u32, raw: u16) {
        let op = (raw >> 8) & 0x3;
        let h1 = ((raw >> 7) & 1) as usize;
        let h2 = ((raw >> 6) & 1) as usize;
        let rs = ((raw >> 3) & 0x7) as usize | (h2 << 3);
        let rd = (raw & 0x7) as usize | (h1 << 3);
        match op {
            0 => self.write_reg(
                rd,
                self.reg_read_thumb(rd, pc)
                    .wrapping_add(self.reg_read_thumb(rs, pc)),
                true,
            ),
            1 => {
                let lhs = self.reg_read_thumb(rd, pc);
                let rhs = self.reg_read_thumb(rs, pc);
                let result = lhs.wrapping_sub(rhs);
                self.set_nzcv(result, lhs >= rhs, sub_overflow(lhs, rhs, result));
            }
            2 => self.write_reg(rd, self.reg_read_thumb(rs, pc), true),
            3 => self.branch_exchange(self.reg_read_thumb(rs, pc)),
            _ => {}
        }
    }

    pub(super) fn execute_thumb_pc_relative_load(&mut self, bus: &mut Bus, pc: u32, raw: u16) {
        let rd = ((raw >> 8) & 0x7) as usize;
        let addr = (pc.wrapping_add(4) & !3).wrapping_add(u32::from(raw & 0xFF) << 2);
        self.regs[rd] = self.cpu_read32(bus, addr);
    }

    pub(super) fn execute_thumb_load_store(&mut self, bus: &mut Bus, raw: u16) {
        let rb = ((raw >> 3) & 0x7) as usize;
        let rd = (raw & 0x7) as usize;
        if raw & 0xF000 == 0x5000 {
            let ro = ((raw >> 6) & 0x7) as usize;
            let addr = self.regs[rb].wrapping_add(self.regs[ro]);
            match (raw >> 9) & 0x7 {
                0b000 => self.cpu_write32(bus, addr, self.regs[rd]),
                0b001 => self.cpu_write16(bus, addr, self.regs[rd] as u16),
                0b010 => self.cpu_write8(bus, addr, self.regs[rd] as u8),
                0b011 => {
                    self.regs[rd] = sign_extend(u32::from(self.cpu_read8(bus, addr)), 8) as u32
                }
                0b100 => self.regs[rd] = rotate_right(self.cpu_read32(bus, addr), (addr & 3) * 8),
                0b101 => self.regs[rd] = self.load_arm_unsigned_halfword(bus, addr),
                0b110 => self.regs[rd] = u32::from(self.cpu_read8(bus, addr)),
                0b111 => self.regs[rd] = self.load_arm_signed_halfword(bus, addr),
                _ => {}
            }
        } else {
            let byte = raw & (1 << 12) != 0;
            let load = raw & (1 << 11) != 0;
            let offset = u32::from((raw >> 6) & 0x1F) << if byte { 0 } else { 2 };
            let addr = self.regs[rb].wrapping_add(offset);
            if load {
                self.regs[rd] = if byte {
                    u32::from(self.cpu_read8(bus, addr))
                } else {
                    rotate_right(self.cpu_read32(bus, addr), (addr & 3) * 8)
                };
            } else if byte {
                self.cpu_write8(bus, addr, self.regs[rd] as u8);
            } else {
                self.cpu_write32(bus, addr, self.regs[rd]);
            }
        }
    }

    pub(super) fn execute_thumb_load_store_halfword(&mut self, bus: &mut Bus, raw: u16) {
        let rb = ((raw >> 3) & 0x7) as usize;
        let rd = (raw & 0x7) as usize;
        let load = raw & (1 << 11) != 0;
        let addr = self.regs[rb].wrapping_add(u32::from((raw >> 6) & 0x1F) << 1);
        if load {
            self.regs[rd] = self.load_arm_unsigned_halfword(bus, addr);
        } else {
            self.cpu_write16(bus, addr, self.regs[rd] as u16);
        }
    }

    pub(super) fn execute_thumb_sp_relative_load(&mut self, bus: &mut Bus, raw: u16) {
        let load = raw & (1 << 11) != 0;
        let rd = ((raw >> 8) & 0x7) as usize;
        let addr = self.regs[13].wrapping_add(u32::from(raw & 0xFF) << 2);
        if load {
            self.regs[rd] = rotate_right(self.cpu_read32(bus, addr), (addr & 3) * 8);
        } else {
            self.cpu_write32(bus, addr, self.regs[rd]);
        }
    }

    pub(super) fn execute_thumb_load_address(&mut self, pc: u32, raw: u16) {
        let rd = ((raw >> 8) & 0x7) as usize;
        let offset = u32::from(raw & 0xFF) << 2;
        self.regs[rd] = if raw & (1 << 11) == 0 {
            (pc.wrapping_add(4) & !3).wrapping_add(offset)
        } else {
            self.regs[13].wrapping_add(offset)
        };
    }

    pub(super) fn execute_thumb_add_offset_sp(&mut self, raw: u16) {
        if raw & 0x0F00 == 0x0000 {
            let offset = u32::from(raw & 0x7F) << 2;
            if raw & (1 << 7) != 0 {
                self.regs[13] = self.regs[13].wrapping_sub(offset);
            } else {
                self.regs[13] = self.regs[13].wrapping_add(offset);
            }
        }
    }

    pub(super) fn execute_thumb_push_pop(&mut self, bus: &mut Bus, raw: u16) {
        let pop = raw & (1 << 11) != 0;
        let extra = raw & (1 << 8) != 0;
        let list = raw & 0xFF;
        if pop {
            for reg in 0..8 {
                if list & (1 << reg) != 0 {
                    self.regs[reg] = self.cpu_read32(bus, self.regs[13]);
                    self.regs[13] = self.regs[13].wrapping_add(4);
                }
            }
            if extra {
                let pc = self.cpu_read32(bus, self.regs[13]);
                self.regs[13] = self.regs[13].wrapping_add(4);
                self.write_reg(15, pc, true);
            }
        } else {
            let count = list.count_ones() + u32::from(extra);
            self.regs[13] = self.regs[13].wrapping_sub(4 * count);
            let mut addr = self.regs[13];
            for reg in 0..8 {
                if list & (1 << reg) != 0 {
                    self.cpu_write32(bus, addr, self.regs[reg]);
                    addr = addr.wrapping_add(4);
                }
            }
            if extra {
                self.cpu_write32(bus, addr, self.regs[14]);
            }
        }
    }

    pub(super) fn execute_thumb_multiple_load_store(&mut self, bus: &mut Bus, raw: u16) {
        let load = raw & (1 << 11) != 0;
        let rb = ((raw >> 8) & 0x7) as usize;
        let list = raw & 0xFF;
        let mut addr = self.regs[rb];
        if list == 0 {
            if load {
                let value = self.cpu_read32(bus, addr);
                self.write_reg(15, value, true);
            } else {
                self.cpu_write32(bus, addr, self.regs[15].wrapping_add(4));
            }
            self.regs[rb] = self.regs[rb].wrapping_add(0x40);
            return;
        }
        let final_addr = addr.wrapping_add(4 * list.count_ones());
        let base_is_first_stored_reg = list & ((1 << rb) - 1) == 0;
        for reg in 0..8 {
            if list & (1 << reg) == 0 {
                continue;
            }
            if load {
                self.regs[reg] = self.cpu_read32(bus, addr);
            } else {
                let value = if reg == rb && !base_is_first_stored_reg {
                    final_addr
                } else {
                    self.regs[reg]
                };
                self.cpu_write32(bus, addr, value);
            }
            addr = addr.wrapping_add(4);
        }
        if !load || list & (1 << rb) == 0 {
            self.regs[rb] = addr;
        }
    }

    pub(super) fn branch_exchange(&mut self, target: u32) {
        if target & 1 != 0 {
            self.cpsr |= CPSR_THUMB;
            self.set_pc(target & !1);
        } else {
            self.cpsr &= !CPSR_THUMB;
            self.set_pc(target & !3);
        }
        self.next_fetch_sequential = false;
    }
}
