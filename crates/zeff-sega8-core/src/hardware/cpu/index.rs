use super::cb::rotate_shift_cb;
use super::flags::szp_flags;
use super::*;

impl Cpu {
    pub(super) fn execute_index(&mut self, bus: &mut Bus, pc: u16, prefix: u8) -> u32 {
        let opcode = self.fetch_u8(bus);
        self.increment_refresh_register();
        let use_iy = prefix == Z80_PREFIX_FD;

        match opcode {
            Z80_PREFIX_DD | Z80_PREFIX_FD => self.execute_index(bus, pc, opcode),
            0x09 | 0x19 | 0x29 | 0x39 => {
                let pair = (opcode >> 4) & 0x03;
                let value = if pair == 2 {
                    self.read_index(use_iy)
                } else {
                    self.read_reg16(pair)
                };
                self.add_index(use_iy, value);
                CYCLES_INDEX_ADD_RR
            }
            0x21 => {
                let value = self.fetch_u16(bus);
                self.write_index(use_iy, value);
                CYCLES_INDEX_LD_RR_NN
            }
            0x22 => {
                let addr = self.fetch_u16(bus);
                self.write_mem_u16(bus, addr, self.read_index(use_iy));
                CYCLES_INDEX_LD_NN_RR
            }
            0x23 => {
                self.write_index(use_iy, self.read_index(use_iy).wrapping_add(1));
                CYCLES_INDEX_INC_DEC_RR
            }
            0x2A => {
                let addr = self.fetch_u16(bus);
                let value = self.read_mem_u16(bus, addr);
                self.write_index(use_iy, value);
                CYCLES_INDEX_LD_RR_NN_INDIRECT
            }
            0x2B => {
                self.write_index(use_iy, self.read_index(use_iy).wrapping_sub(1));
                CYCLES_INDEX_INC_DEC_RR
            }
            0x04 | 0x0C | 0x14 | 0x1C | 0x3C => {
                let register = (opcode >> 3) & 0x07;
                let value = self.read_reg8(bus, register);
                let result = value.wrapping_add(1);
                self.write_reg8(bus, register, result);
                self.set_inc_flags(value, result);
                CYCLES_INC_DEC_R + Z80_PREFIX_OVERHEAD
            }
            0x24 | 0x2C => {
                let register = (opcode >> 3) & 0x07;
                let value = self.read_indexed_reg8(bus, use_iy, register);
                let result = value.wrapping_add(1);
                self.write_indexed_reg8(bus, use_iy, register, result);
                self.set_inc_flags(value, result);
                CYCLES_INDEX_INC_DEC_R
            }
            0x05 | 0x0D | 0x15 | 0x1D | 0x3D => {
                let register = (opcode >> 3) & 0x07;
                let value = self.read_reg8(bus, register);
                let result = value.wrapping_sub(1);
                self.write_reg8(bus, register, result);
                self.set_dec_flags(value, result);
                CYCLES_INC_DEC_R + Z80_PREFIX_OVERHEAD
            }
            0x25 | 0x2D => {
                let register = (opcode >> 3) & 0x07;
                let value = self.read_indexed_reg8(bus, use_iy, register);
                let result = value.wrapping_sub(1);
                self.write_indexed_reg8(bus, use_iy, register, result);
                self.set_dec_flags(value, result);
                CYCLES_INDEX_INC_DEC_R
            }
            0x06 | 0x0E | 0x16 | 0x1E | 0x3E => {
                let register = (opcode >> 3) & 0x07;
                let value = self.fetch_u8(bus);
                self.write_reg8(bus, register, value);
                CYCLES_LD_R_N + Z80_PREFIX_OVERHEAD
            }
            0x26 | 0x2E => {
                let register = (opcode >> 3) & 0x07;
                let value = self.fetch_u8(bus);
                self.write_indexed_reg8(bus, use_iy, register, value);
                CYCLES_INDEX_LD_R_N
            }
            0x34 => {
                let addr = self.fetch_indexed_addr(bus, use_iy);
                let value = bus.cpu_read(addr);
                let result = value.wrapping_add(1);
                bus.cpu_write(addr, result);
                self.set_inc_flags(value, result);
                CYCLES_INDEX_INC_DEC_MEM
            }
            0x35 => {
                let addr = self.fetch_indexed_addr(bus, use_iy);
                let value = bus.cpu_read(addr);
                let result = value.wrapping_sub(1);
                bus.cpu_write(addr, result);
                self.set_dec_flags(value, result);
                CYCLES_INDEX_INC_DEC_MEM
            }
            0x36 => {
                let addr = self.fetch_indexed_addr(bus, use_iy);
                let value = self.fetch_u8(bus);
                bus.cpu_write(addr, value);
                CYCLES_INDEX_LD_MEM_N
            }
            Z80_PREFIX_CB => {
                let displacement = self.fetch_u8(bus) as i8;
                let cb_opcode = self.fetch_u8(bus);
                self.execute_index_cb(bus, use_iy, displacement, cb_opcode);
                CYCLES_INDEX_CB
            }
            0x46 | 0x4E | 0x56 | 0x5E | 0x66 | 0x6E | 0x7E => {
                let register = (opcode >> 3) & 0x07;
                let addr = self.fetch_indexed_addr(bus, use_iy);
                let value = bus.cpu_read(addr);
                self.write_reg8(bus, register, value);
                CYCLES_INDEX_LD_R_MEM
            }
            0x70..=0x75 | 0x77 => {
                let register = opcode & 0x07;
                let value = self.read_reg8(bus, register);
                let addr = self.fetch_indexed_addr(bus, use_iy);
                bus.cpu_write(addr, value);
                CYCLES_INDEX_LD_MEM_R
            }
            0x40..=0x7F if opcode != 0x76 => {
                let dst = (opcode >> 3) & 0x07;
                let src = opcode & 0x07;
                let value = self.read_indexed_reg8(bus, use_iy, src);
                self.write_indexed_reg8(bus, use_iy, dst, value);
                CYCLES_INDEX_LD_R_R
            }
            0x86 | 0x8E | 0x96 | 0x9E | 0xA6 | 0xAE | 0xB6 | 0xBE => {
                let group = (opcode >> 3) & 0x07;
                let addr = self.fetch_indexed_addr(bus, use_iy);
                let value = bus.cpu_read(addr);
                self.execute_alu_group(group, value);
                CYCLES_INDEX_ALU_MEM
            }
            0x80..=0xBF => {
                let src = opcode & 0x07;
                let value = self.read_indexed_reg8(bus, use_iy, src);
                self.execute_alu_group((opcode >> 3) & 0x07, value);
                CYCLES_INDEX_LD_R_R
            }
            0xC0 | 0xC8 | 0xD0 | 0xD8 | 0xE0 | 0xE8 | 0xF0 | 0xF8 => {
                let condition = (opcode >> 3) & 0x07;
                if self.condition_is_true(condition) {
                    self.regs.pc = self.pop_u16(bus);
                    CYCLES_RET_CC + Z80_PREFIX_OVERHEAD
                } else {
                    CYCLES_RET_CC_NOT_TAKEN + Z80_PREFIX_OVERHEAD
                }
            }
            0xC7 | 0xCF | 0xD7 | 0xDF | 0xE7 | 0xEF | 0xF7 | 0xFF => {
                let vector = u16::from(opcode & 0x38);
                self.push_u16(bus, self.regs.pc);
                self.regs.pc = vector;
                CYCLES_RST + Z80_PREFIX_OVERHEAD
            }
            0xDB => {
                let port = self.fetch_u8(bus);
                self.regs.a = bus.io_read(port);
                CYCLES_IN_A_N + Z80_PREFIX_OVERHEAD
            }
            0xE1 => {
                let value = self.pop_u16(bus);
                self.write_index(use_iy, value);
                CYCLES_INDEX_POP_RR
            }
            0xE3 => {
                let value = self.read_mem_u16(bus, self.regs.sp);
                self.write_mem_u16(bus, self.regs.sp, self.read_index(use_iy));
                self.write_index(use_iy, value);
                CYCLES_INDEX_EX_SP_RR
            }
            0xE5 => {
                self.push_u16(bus, self.read_index(use_iy));
                CYCLES_INDEX_PUSH_RR
            }
            0xE9 => {
                self.regs.pc = self.read_index(use_iy);
                CYCLES_INDEX_JP_RR
            }
            0xF9 => {
                self.regs.sp = self.read_index(use_iy);
                CYCLES_INDEX_LD_SP_RR
            }
            _ => self.execute_unprefixed(bus, pc, opcode) + Z80_PREFIX_OVERHEAD,
        }
    }

    fn read_index(&self, use_iy: bool) -> u16 {
        if use_iy { self.regs.iy } else { self.regs.ix }
    }

    fn write_index(&mut self, use_iy: bool, value: u16) {
        if use_iy {
            self.regs.iy = value;
        } else {
            self.regs.ix = value;
        }
    }

    fn read_indexed_reg8(&self, bus: &Bus, use_iy: bool, register: u8) -> u8 {
        match register {
            4 => (self.read_index(use_iy) >> 8) as u8,
            5 => self.read_index(use_iy) as u8,
            REGISTER_MEMORY_INDEX => bus.cpu_read(self.read_index(use_iy)),
            _ => self.read_reg8(bus, register),
        }
    }

    fn write_indexed_reg8(&mut self, bus: &mut Bus, use_iy: bool, register: u8, value: u8) {
        match register {
            4 => {
                let low = self.read_index(use_iy) as u8;
                self.write_index(use_iy, u16::from_be_bytes([value, low]));
            }
            5 => {
                let high = (self.read_index(use_iy) >> 8) as u8;
                self.write_index(use_iy, u16::from_be_bytes([high, value]));
            }
            REGISTER_MEMORY_INDEX => bus.cpu_write(self.read_index(use_iy), value),
            _ => self.write_reg8(bus, register, value),
        }
    }

    fn fetch_indexed_addr(&mut self, bus: &Bus, use_iy: bool) -> u16 {
        let displacement = self.fetch_u8(bus) as i8;
        self.read_index(use_iy)
            .wrapping_add_signed(i16::from(displacement))
    }

    fn add_index(&mut self, use_iy: bool, value: u16) {
        let index = self.read_index(use_iy);
        let result = index.wrapping_add(value);
        let half_carry = (index & 0x0FFF) + (value & 0x0FFF) > 0x0FFF;
        let carry = u32::from(index) + u32::from(value) > 0xFFFF;
        let preserved = self.regs.f & (Z80_FLAG_SIGN | Z80_FLAG_ZERO | Z80_FLAG_PARITY_OVERFLOW);
        let undocumented = ((result >> 8) as u8) & (Z80_FLAG_BIT_5 | Z80_FLAG_BIT_3);

        self.write_index(use_iy, result);
        self.regs.f = preserved
            | undocumented
            | if half_carry { Z80_FLAG_HALF_CARRY } else { 0 }
            | if carry { Z80_FLAG_CARRY } else { 0 };
    }

    fn execute_index_cb(&mut self, bus: &mut Bus, use_iy: bool, displacement: i8, opcode: u8) {
        let addr = self
            .read_index(use_iy)
            .wrapping_add_signed(i16::from(displacement));
        let group = opcode >> 6;
        let operation = (opcode >> 3) & 0x07;
        let register = opcode & 0x07;
        let value = bus.cpu_read(addr);

        match group {
            0 => {
                let (result, carry) = rotate_shift_cb(operation, value, self.regs.f);
                bus.cpu_write(addr, result);
                if register != REGISTER_MEMORY_INDEX {
                    self.write_reg8(bus, register, result);
                }
                self.regs.f = szp_flags(result) | if carry { Z80_FLAG_CARRY } else { 0 };
            }
            1 => self.set_bit_flags(operation, value),
            2 => {
                let result = value & !(1 << operation);
                bus.cpu_write(addr, result);
                if register != REGISTER_MEMORY_INDEX {
                    self.write_reg8(bus, register, result);
                }
            }
            3 => {
                let result = value | (1 << operation);
                bus.cpu_write(addr, result);
                if register != REGISTER_MEMORY_INDEX {
                    self.write_reg8(bus, register, result);
                }
            }
            _ => unreachable!("CB group is always two bits"),
        }
    }
}
