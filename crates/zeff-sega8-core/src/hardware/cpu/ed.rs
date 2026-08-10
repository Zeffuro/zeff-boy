use super::flags::{sz53_flags, szp_flags};
use super::*;

impl Cpu {
    pub(super) fn execute_ed(&mut self, bus: &mut Bus, pc: u16) -> u32 {
        let opcode = self.fetch_u8(bus);
        self.increment_refresh_register();
        match opcode {
            0x42 | 0x52 | 0x62 | 0x72 => {
                let pair = (opcode >> 4) & 0x03;
                self.sbc_hl(self.read_reg16(pair));
                CYCLES_ED_16BIT_ALU
            }
            0x43 | 0x53 | 0x63 | 0x73 => {
                let pair = (opcode >> 4) & 0x03;
                let addr = self.fetch_u16(bus);
                self.write_mem_u16(bus, addr, self.read_reg16(pair));
                CYCLES_ED_16BIT_MEMORY
            }
            0x44 | 0x4C | 0x54 | 0x5C | 0x64 | 0x6C | 0x74 | 0x7C => {
                self.neg_a();
                CYCLES_ED_NEG
            }
            0x45 | 0x4D | 0x55 | 0x5D | 0x65 | 0x6D | 0x75 | 0x7D => {
                self.interrupt_flip_flop_1 = self.interrupt_flip_flop_2;
                self.regs.pc = self.pop_u16(bus);
                CYCLES_RETI_RETN
            }
            0x46 | 0x4E | 0x66 | 0x6E => {
                self.interrupt_mode = InterruptMode::Im0;
                CYCLES_IM
            }
            0x47 => {
                self.regs.i = self.regs.a;
                CYCLES_ED_SPECIAL_REGISTER
            }
            0x4A | 0x5A | 0x6A | 0x7A => {
                let pair = (opcode >> 4) & 0x03;
                self.adc_hl(self.read_reg16(pair));
                CYCLES_ED_16BIT_ALU
            }
            0x4B | 0x5B | 0x6B | 0x7B => {
                let pair = (opcode >> 4) & 0x03;
                let addr = self.fetch_u16(bus);
                let value = self.read_mem_u16(bus, addr);
                self.write_reg16(pair, value);
                CYCLES_ED_16BIT_MEMORY
            }
            0x4F => {
                self.regs.r = self.regs.a;
                CYCLES_ED_SPECIAL_REGISTER
            }
            0x40 | 0x48 | 0x50 | 0x58 | 0x60 | 0x68 | 0x78 => {
                let register = (opcode >> 3) & 0x07;
                let value = bus.io_read(self.regs.c);
                self.write_reg8(bus, register, value);
                self.set_szp_flags_preserving_carry(value);
                CYCLES_ED_IO
            }
            0x57 => {
                self.load_a_from_special_register(self.regs.i);
                CYCLES_ED_SPECIAL_REGISTER
            }
            0x5F => {
                self.load_a_from_special_register(self.regs.r);
                CYCLES_ED_SPECIAL_REGISTER
            }
            0x67 => {
                self.rotate_decimal_right(bus);
                CYCLES_ED_NIBBLE_ROTATE
            }
            0x6F => {
                self.rotate_decimal_left(bus);
                CYCLES_ED_NIBBLE_ROTATE
            }
            0x70 => {
                let value = bus.io_read(self.regs.c);
                self.set_szp_flags_preserving_carry(value);
                CYCLES_ED_IO
            }
            0x41 | 0x49 | 0x51 | 0x59 | 0x61 | 0x69 | 0x79 => {
                let register = (opcode >> 3) & 0x07;
                let value = self.read_reg8(bus, register);
                bus.io_write(self.regs.c, value);
                CYCLES_ED_IO
            }
            0x71 => {
                bus.io_write(self.regs.c, 0);
                CYCLES_ED_IO
            }
            0x56 | 0x76 => {
                self.interrupt_mode = InterruptMode::Im1;
                CYCLES_IM
            }
            0x5E | 0x7E => {
                self.interrupt_mode = InterruptMode::Im2;
                CYCLES_IM
            }
            0xA0 => {
                self.block_copy(bus, 1);
                CYCLES_ED_BLOCK
            }
            0xA1 => {
                self.block_compare(bus, 1);
                CYCLES_ED_BLOCK
            }
            0xA2 => {
                self.block_input(bus, 1);
                CYCLES_ED_BLOCK
            }
            0xA3 => {
                self.block_output(bus, 1);
                CYCLES_ED_BLOCK
            }
            0xA8 => {
                self.block_copy(bus, -1);
                CYCLES_ED_BLOCK
            }
            0xA9 => {
                self.block_compare(bus, -1);
                CYCLES_ED_BLOCK
            }
            0xAA => {
                self.block_input(bus, -1);
                CYCLES_ED_BLOCK
            }
            0xAB => {
                self.block_output(bus, -1);
                CYCLES_ED_BLOCK
            }
            0xB0 => {
                self.block_copy(bus, 1);
                if self.regs.bc() != 0 {
                    self.regs.pc = pc;
                    CYCLES_ED_BLOCK_REPEAT
                } else {
                    CYCLES_ED_BLOCK
                }
            }
            0xB1 => {
                let matched = self.block_compare(bus, 1);
                if self.regs.bc() != 0 && !matched {
                    self.regs.pc = pc;
                    CYCLES_ED_BLOCK_REPEAT
                } else {
                    CYCLES_ED_BLOCK
                }
            }
            0xB2 => {
                self.block_input(bus, 1);
                if self.regs.b != 0 {
                    self.regs.pc = pc;
                    CYCLES_ED_BLOCK_REPEAT
                } else {
                    CYCLES_ED_BLOCK
                }
            }
            0xB3 => {
                self.block_output(bus, 1);
                if self.regs.b != 0 {
                    self.regs.pc = pc;
                    CYCLES_ED_BLOCK_REPEAT
                } else {
                    CYCLES_ED_BLOCK
                }
            }
            0xB8 => {
                self.block_copy(bus, -1);
                if self.regs.bc() != 0 {
                    self.regs.pc = pc;
                    CYCLES_ED_BLOCK_REPEAT
                } else {
                    CYCLES_ED_BLOCK
                }
            }
            0xB9 => {
                let matched = self.block_compare(bus, -1);
                if self.regs.bc() != 0 && !matched {
                    self.regs.pc = pc;
                    CYCLES_ED_BLOCK_REPEAT
                } else {
                    CYCLES_ED_BLOCK
                }
            }
            0xBA => {
                self.block_input(bus, -1);
                if self.regs.b != 0 {
                    self.regs.pc = pc;
                    CYCLES_ED_BLOCK_REPEAT
                } else {
                    CYCLES_ED_BLOCK
                }
            }
            0xBB => {
                self.block_output(bus, -1);
                if self.regs.b != 0 {
                    self.regs.pc = pc;
                    CYCLES_ED_BLOCK_REPEAT
                } else {
                    CYCLES_ED_BLOCK
                }
            }
            _ => CYCLES_ED_NOP,
        }
    }

    fn block_copy(&mut self, bus: &mut Bus, delta: i16) {
        let value = bus.cpu_read(self.regs.hl());
        bus.cpu_write(self.regs.de(), value);
        self.regs.set_hl(self.regs.hl().wrapping_add_signed(delta));
        self.regs.set_de(self.regs.de().wrapping_add_signed(delta));
        self.regs.set_bc(self.regs.bc().wrapping_sub(1));

        let preserved = self.regs.f & (Z80_FLAG_SIGN | Z80_FLAG_ZERO | Z80_FLAG_CARRY);
        let undocumented = self.regs.a.wrapping_add(value) & (Z80_FLAG_BIT_5 | Z80_FLAG_BIT_3);
        self.regs.f = preserved
            | undocumented
            | if self.regs.bc() != 0 {
                Z80_FLAG_PARITY_OVERFLOW
            } else {
                0
            };
    }

    fn block_compare(&mut self, bus: &Bus, delta: i16) -> bool {
        let value = bus.cpu_read(self.regs.hl());
        let result = self.regs.a.wrapping_sub(value);
        let half_borrow = (self.regs.a & 0x0F) < (value & 0x0F);
        self.regs.set_hl(self.regs.hl().wrapping_add_signed(delta));
        self.regs.set_bc(self.regs.bc().wrapping_sub(1));

        self.regs.f = (self.regs.f & Z80_FLAG_CARRY)
            | (result & (Z80_FLAG_SIGN | Z80_FLAG_BIT_5 | Z80_FLAG_BIT_3))
            | if result == 0 { Z80_FLAG_ZERO } else { 0 }
            | Z80_FLAG_SUBTRACT
            | if half_borrow { Z80_FLAG_HALF_CARRY } else { 0 }
            | if self.regs.bc() != 0 {
                Z80_FLAG_PARITY_OVERFLOW
            } else {
                0
            };

        result == 0
    }

    fn block_input(&mut self, bus: &mut Bus, delta: i16) {
        let value = bus.io_read(self.regs.c);
        bus.cpu_write(self.regs.hl(), value);
        self.regs.set_hl(self.regs.hl().wrapping_add_signed(delta));
        self.regs.b = self.regs.b.wrapping_sub(1);
        self.set_block_io_flags(value);
    }

    fn block_output(&mut self, bus: &mut Bus, delta: i16) {
        let value = bus.cpu_read(self.regs.hl());
        bus.io_write(self.regs.c, value);
        self.regs.set_hl(self.regs.hl().wrapping_add_signed(delta));
        self.regs.b = self.regs.b.wrapping_sub(1);
        self.set_block_io_flags(value);
    }

    fn set_block_io_flags(&mut self, value: u8) {
        self.regs.f = (self.regs.f & Z80_FLAG_CARRY)
            | (self.regs.b & (Z80_FLAG_SIGN | Z80_FLAG_BIT_5 | Z80_FLAG_BIT_3))
            | if self.regs.b == 0 { Z80_FLAG_ZERO } else { 0 }
            | if value & 0x80 != 0 {
                Z80_FLAG_SUBTRACT
            } else {
                0
            };
    }

    fn adc_hl(&mut self, value: u16) {
        let hl = self.regs.hl();
        let carry_in = u16::from(self.regs.f & Z80_FLAG_CARRY != 0);
        let result = hl.wrapping_add(value).wrapping_add(carry_in);
        let half_carry = (hl & 0x0FFF) + (value & 0x0FFF) + carry_in > 0x0FFF;
        let carry = u32::from(hl) + u32::from(value) + u32::from(carry_in) > 0xFFFF;
        let overflow = (!(hl ^ value) & (hl ^ result) & 0x8000) != 0;

        self.regs.set_hl(result);
        self.regs.f = flags_from_u16_result(result)
            | if half_carry { Z80_FLAG_HALF_CARRY } else { 0 }
            | if overflow {
                Z80_FLAG_PARITY_OVERFLOW
            } else {
                0
            }
            | if carry { Z80_FLAG_CARRY } else { 0 };
    }

    fn sbc_hl(&mut self, value: u16) {
        let hl = self.regs.hl();
        let carry_in = u16::from(self.regs.f & Z80_FLAG_CARRY != 0);
        let result = hl.wrapping_sub(value).wrapping_sub(carry_in);
        let half_borrow = (hl & 0x0FFF) < ((value & 0x0FFF) + carry_in);
        let carry = u32::from(hl) < u32::from(value) + u32::from(carry_in);
        let overflow = ((hl ^ value) & (hl ^ result) & 0x8000) != 0;

        self.regs.set_hl(result);
        self.regs.f = flags_from_u16_result(result)
            | Z80_FLAG_SUBTRACT
            | if half_borrow { Z80_FLAG_HALF_CARRY } else { 0 }
            | if overflow {
                Z80_FLAG_PARITY_OVERFLOW
            } else {
                0
            }
            | if carry { Z80_FLAG_CARRY } else { 0 };
    }

    fn neg_a(&mut self) {
        let value = self.regs.a;
        let result = 0u8.wrapping_sub(value);
        self.regs.a = result;
        self.regs.f = sz53_flags(result)
            | Z80_FLAG_SUBTRACT
            | if value & 0x0F != 0 {
                Z80_FLAG_HALF_CARRY
            } else {
                0
            }
            | if value == 0x80 {
                Z80_FLAG_PARITY_OVERFLOW
            } else {
                0
            }
            | if value != 0 { Z80_FLAG_CARRY } else { 0 };
    }

    fn load_a_from_special_register(&mut self, value: u8) {
        self.regs.a = value;
        self.regs.f = (self.regs.f & Z80_FLAG_CARRY)
            | sz53_flags(value)
            | if self.interrupt_flip_flop_2 {
                Z80_FLAG_PARITY_OVERFLOW
            } else {
                0
            };
    }

    fn rotate_decimal_right(&mut self, bus: &mut Bus) {
        let value = bus.cpu_read(self.regs.hl());
        let a = self.regs.a;
        bus.cpu_write(self.regs.hl(), ((a & 0x0F) << 4) | (value >> 4));
        self.regs.a = (a & 0xF0) | (value & 0x0F);
        self.regs.f = (self.regs.f & Z80_FLAG_CARRY) | szp_flags(self.regs.a);
    }

    fn rotate_decimal_left(&mut self, bus: &mut Bus) {
        let value = bus.cpu_read(self.regs.hl());
        let a = self.regs.a;
        bus.cpu_write(self.regs.hl(), (value << 4) | (a & 0x0F));
        self.regs.a = (a & 0xF0) | (value >> 4);
        self.regs.f = (self.regs.f & Z80_FLAG_CARRY) | szp_flags(self.regs.a);
    }
}

fn flags_from_u16_result(value: u16) -> u8 {
    let hi = (value >> 8) as u8;
    (hi & (Z80_FLAG_SIGN | Z80_FLAG_BIT_5 | Z80_FLAG_BIT_3))
        | if value == 0 { Z80_FLAG_ZERO } else { 0 }
}
