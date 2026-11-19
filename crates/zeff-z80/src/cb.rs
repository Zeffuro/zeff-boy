use super::flags::szp_flags;
use super::*;

impl Cpu {
    pub(super) fn execute_cb<B: Z80Bus>(&mut self, bus: &mut B) -> u32 {
        let opcode = self.fetch_u8(bus);
        self.increment_refresh_register();
        self.execute_cb_terminal(bus, opcode)
    }

    pub(super) fn execute_cb_terminal<B: Z80Bus>(&mut self, bus: &mut B, opcode: u8) -> u32 {
        if self.execute_cb_register(opcode).is_some() {
            CYCLES_CB_R
        } else {
            rmw::Rmw::cb(self, opcode).execute(self, bus);
            if opcode >> 6 == 1 {
                CYCLES_CB_BIT_HL
            } else {
                CYCLES_CB_HL
            }
        }
    }

    pub(super) fn execute_cb_register(&mut self, opcode: u8) -> Option<()> {
        let register = register_cycle::Register::decode(opcode & 7)?;
        let (result, flags, write) = effect(opcode, register.read(self), self.regs.f);
        if write {
            register.write(self, result);
        }
        self.regs.f = flags;
        Some(())
    }
}

pub(super) fn bit_flags(bit: u8, value: u8, flags: u8, xy: u8) -> u8 {
    let bit_set = value & (1 << bit) != 0;
    (flags & Z80_FLAG_CARRY)
        | Z80_FLAG_HALF_CARRY
        | (xy & (Z80_FLAG_BIT_5 | Z80_FLAG_BIT_3))
        | if bit == 7 && bit_set {
            Z80_FLAG_SIGN
        } else {
            0
        }
        | if bit_set {
            0
        } else {
            Z80_FLAG_ZERO | Z80_FLAG_PARITY_OVERFLOW
        }
}

pub(super) fn effect(opcode: u8, value: u8, flags: u8) -> (u8, u8, bool) {
    let operation = (opcode >> 3) & 7;
    match opcode >> 6 {
        0 => {
            let (result, carry) = rotate_shift_cb(operation, value, flags);
            (
                result,
                szp_flags(result) | if carry { Z80_FLAG_CARRY } else { 0 },
                true,
            )
        }
        1 => (value, bit_flags(operation, value, flags, value), false),
        2 => (value & !(1 << operation), flags, true),
        3 => (value | (1 << operation), flags, true),
        _ => unreachable!("CB group is always two bits"),
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
