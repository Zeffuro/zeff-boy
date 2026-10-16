use super::*;

impl Cpu {
    pub(super) fn execute_index<B: Z80Bus>(&mut self, bus: &mut B, pc: u16, prefix: u8) -> u32 {
        let opcode = self.fetch_u8(bus);
        self.increment_refresh_register();
        self.execute_index_terminal(bus, pc, prefix, opcode)
    }

    pub(super) fn execute_index_terminal<B: Z80Bus>(
        &mut self,
        bus: &mut B,
        pc: u16,
        prefix: u8,
        opcode: u8,
    ) -> u32 {
        if let Some(load) = load::Load::decode(self, opcode, Some(prefix)) {
            return load.execute(self, bus) + Z80_PREFIX_OVERHEAD;
        }
        if let Some(operation) = stack::Stack::decode(self, opcode, Some(prefix)) {
            return operation.execute(self, bus) + Z80_PREFIX_OVERHEAD;
        }
        if let Some(operation) = indexed_memory::IndexedMemory::decode(self, opcode, prefix) {
            return operation.execute(self, bus);
        }
        let use_iy = prefix == Z80_PREFIX_FD;

        match opcode {
            Z80_PREFIX_DD | Z80_PREFIX_FD => {
                self.execute_index(bus, pc, opcode) + Z80_PREFIX_OVERHEAD
            }
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

            0x23 => {
                self.write_index(use_iy, self.read_index(use_iy).wrapping_add(1));
                CYCLES_INDEX_INC_DEC_RR
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
            Z80_PREFIX_CB => indexed_cb::IndexedCb::new(self, prefix).execute(self, bus),
            Z80_PREFIX_ED => {
                self.execute_ed(bus, self.regs.pc.wrapping_sub(1)) + Z80_PREFIX_OVERHEAD
            }
            0x40..=0x7F if opcode != 0x76 => {
                let dst = (opcode >> 3) & 0x07;
                let src = opcode & 0x07;
                let value = self.read_indexed_reg8(bus, use_iy, src);
                self.write_indexed_reg8(bus, use_iy, dst, value);
                CYCLES_INDEX_LD_R_R
            }
            0x80..=0xBF => {
                let src = opcode & 0x07;
                let value = self.read_indexed_reg8(bus, use_iy, src);
                self.execute_alu_group((opcode >> 3) & 0x07, value);
                CYCLES_INDEX_LD_R_R
            }
            0xDB => {
                let port = self.fetch_u8(bus);
                self.regs.a = bus.io_read(port);
                CYCLES_IN_A_N + Z80_PREFIX_OVERHEAD
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

    fn read_indexed_reg8<B: Z80Bus>(&self, bus: &B, use_iy: bool, register: u8) -> u8 {
        match register {
            4 => (self.read_index(use_iy) >> 8) as u8,
            5 => self.read_index(use_iy) as u8,
            REGISTER_MEMORY_INDEX => bus.cpu_read(self.read_index(use_iy)),
            _ => self.read_reg8(bus, register),
        }
    }

    fn write_indexed_reg8<B: Z80Bus>(
        &mut self,
        bus: &mut B,
        use_iy: bool,
        register: u8,
        value: u8,
    ) {
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
}
