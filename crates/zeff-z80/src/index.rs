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
        if let Some(cycles) = self.execute_m1_register(opcode, Some(prefix)) {
            return cycles + Z80_PREFIX_OVERHEAD;
        }
        if let Some(cycles) = self.execute_index_register(prefix, opcode) {
            return cycles + Z80_PREFIX_OVERHEAD;
        }
        if let Some(single) = index_cycle::Single::decode(self, opcode, prefix) {
            single.step(self, bus, opcode);
            return Z80_PREFIX_OVERHEAD + CYCLES_M1 + CYCLES_MEMORY_TRANSFER;
        }
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

            Z80_PREFIX_CB => indexed_cb::IndexedCb::new(self, prefix).execute(self, bus),
            Z80_PREFIX_ED => {
                self.execute_ed(bus, self.regs.pc.wrapping_sub(1)) + Z80_PREFIX_OVERHEAD
            }
            0xDB => {
                let port = self.fetch_u8(bus);
                self.set_memptr(u16::from_be_bytes([self.regs.a, port]).wrapping_add(1));
                self.regs.a = bus.io_read(port);
                CYCLES_IN_A_N + Z80_PREFIX_OVERHEAD
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

    fn add_index(&mut self, use_iy: bool, value: u16) {
        let index = self.read_index(use_iy);
        self.set_memptr(index.wrapping_add(1));
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
