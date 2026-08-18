use super::flags::szp_flags;
use super::*;

impl Cpu {
    pub(super) fn execute_cb<B: SegaCpuBus>(&mut self, bus: &mut B) -> u32 {
        let opcode = self.fetch_u8(bus);
        self.increment_refresh_register();

        let group = opcode >> 6;
        let operation = (opcode >> 3) & 0x07;
        let register = opcode & 0x07;
        let value = self.read_reg8(bus, register);
        let uses_memory = register == REGISTER_MEMORY_INDEX;

        match group {
            0 => {
                let (result, carry) = rotate_shift_cb(operation, value, self.regs.f);
                self.write_reg8(bus, register, result);
                self.regs.f = szp_flags(result) | if carry { Z80_FLAG_CARRY } else { 0 };
                if uses_memory {
                    CYCLES_CB_HL
                } else {
                    CYCLES_CB_R
                }
            }
            1 => {
                self.set_bit_flags(operation, value);
                if uses_memory {
                    CYCLES_CB_BIT_HL
                } else {
                    CYCLES_CB_R
                }
            }
            2 => {
                let result = value & !(1 << operation);
                self.write_reg8(bus, register, result);
                if uses_memory {
                    CYCLES_CB_HL
                } else {
                    CYCLES_CB_R
                }
            }
            3 => {
                let result = value | (1 << operation);
                self.write_reg8(bus, register, result);
                if uses_memory {
                    CYCLES_CB_HL
                } else {
                    CYCLES_CB_R
                }
            }
            _ => unreachable!("CB group is always two bits"),
        }
    }

    pub(super) fn set_bit_flags(&mut self, bit: u8, value: u8) {
        let bit_set = value & (1 << bit) != 0;
        let carry = self.regs.f & Z80_FLAG_CARRY;
        self.regs.f = carry
            | Z80_FLAG_HALF_CARRY
            | (value & (Z80_FLAG_BIT_5 | Z80_FLAG_BIT_3))
            | if bit == 7 && bit_set {
                Z80_FLAG_SIGN
            } else {
                0
            }
            | if bit_set {
                0
            } else {
                Z80_FLAG_ZERO | Z80_FLAG_PARITY_OVERFLOW
            };
    }
}

pub(super) fn rotate_shift_cb(operation: u8, value: u8, flags: u8) -> (u8, bool) {
    match operation {
        0 => (value.rotate_left(1), value & 0x80 != 0),
        1 => (value.rotate_right(1), value & 0x01 != 0),
        2 => {
            let carry_in = u8::from(flags & Z80_FLAG_CARRY != 0);
            ((value << 1) | carry_in, value & 0x80 != 0)
        }
        3 => {
            let carry_in = if flags & Z80_FLAG_CARRY != 0 { 0x80 } else { 0 };
            ((value >> 1) | carry_in, value & 0x01 != 0)
        }
        4 => (value << 1, value & 0x80 != 0),
        5 => ((value >> 1) | (value & 0x80), value & 0x01 != 0),
        6 => ((value << 1) | 0x01, value & 0x80 != 0),
        7 => (value >> 1, value & 0x01 != 0),
        _ => unreachable!("CB rotate/shift operation is always three bits"),
    }
}
