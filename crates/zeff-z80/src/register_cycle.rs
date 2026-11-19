use super::*;

#[derive(Clone, Copy)]
pub(super) enum Register {
    B,
    C,
    D,
    E,
    H,
    L,
    A,
}

impl Register {
    pub(super) fn decode(index: u8) -> Option<Self> {
        Some(match index {
            0 => Self::B,
            1 => Self::C,
            2 => Self::D,
            3 => Self::E,
            4 => Self::H,
            5 => Self::L,
            7 => Self::A,
            _ => return None,
        })
    }

    pub(super) fn read(self, cpu: &Cpu) -> u8 {
        match self {
            Self::B => cpu.regs.b,
            Self::C => cpu.regs.c,
            Self::D => cpu.regs.d,
            Self::E => cpu.regs.e,
            Self::H => cpu.regs.h,
            Self::L => cpu.regs.l,
            Self::A => cpu.regs.a,
        }
    }

    pub(super) fn write(self, cpu: &mut Cpu, value: u8) {
        match self {
            Self::B => cpu.regs.b = value,
            Self::C => cpu.regs.c = value,
            Self::D => cpu.regs.d = value,
            Self::E => cpu.regs.e = value,
            Self::H => cpu.regs.h = value,
            Self::L => cpu.regs.l = value,
            Self::A => cpu.regs.a = value,
        }
    }
}

#[derive(Clone, Copy)]
pub(super) enum RegisterCycle {
    Nop,
    Rlca,
    ExchangeAf,
    Rrca,
    Rla,
    Rra,
    Daa,
    Cpl,
    Scf,
    Ccf,
    Increment(Register),
    Decrement(Register),
    Move {
        destination: Register,
        source: Register,
    },
    Alu {
        group: u8,
        source: Register,
    },
    Exx,
    JumpHl,
    ExchangeDeHl,
    DisableInterrupts,
}

pub(super) fn decode(opcode: u8) -> Option<RegisterCycle> {
    use RegisterCycle::*;
    Some(match opcode {
        0x00 => Nop,
        0x07 => Rlca,
        0x08 => ExchangeAf,
        0x0f => Rrca,
        0x17 => Rla,
        0x1f => Rra,
        0x27 => Daa,
        0x2f => Cpl,
        0x37 => Scf,
        0x3f => Ccf,
        0x04 | 0x0c | 0x14 | 0x1c | 0x24 | 0x2c | 0x3c => {
            Increment(Register::decode((opcode >> 3) & 7)?)
        }
        0x05 | 0x0d | 0x15 | 0x1d | 0x25 | 0x2d | 0x3d => {
            Decrement(Register::decode((opcode >> 3) & 7)?)
        }
        0x40..=0x7f => Move {
            destination: Register::decode((opcode >> 3) & 7)?,
            source: Register::decode(opcode & 7)?,
        },
        0x80..=0xbf => Alu {
            group: (opcode >> 3) & 7,
            source: Register::decode(opcode & 7)?,
        },
        0xd9 => Exx,
        0xe9 => JumpHl,
        0xeb => ExchangeDeHl,
        0xf3 => DisableInterrupts,
        _ => return None,
    })
}

impl Cpu {
    pub(super) fn execute_register_cycle(&mut self, opcode: u8) -> Option<u32> {
        use RegisterCycle::*;
        Some(match decode(opcode)? {
            Nop => CYCLES_NOP,
            Rlca => {
                self.rotate_a_left_circular();
                CYCLES_ACCUMULATOR_ROTATE
            }
            ExchangeAf => {
                self.exchange_af_with_shadow();
                CYCLES_EX_AF_AF_SHADOW
            }
            Rrca => {
                self.rotate_a_right_circular();
                CYCLES_ACCUMULATOR_ROTATE
            }
            Rla => {
                self.rotate_a_left_through_carry();
                CYCLES_ACCUMULATOR_ROTATE
            }
            Rra => {
                self.rotate_a_right_through_carry();
                CYCLES_ACCUMULATOR_ROTATE
            }
            Daa => {
                self.decimal_adjust_accumulator();
                CYCLES_FLAG_OP
            }
            Cpl => {
                self.complement_accumulator();
                CYCLES_FLAG_OP
            }
            Scf => {
                self.set_carry_flag();
                CYCLES_FLAG_OP
            }
            Ccf => {
                self.complement_carry_flag();
                CYCLES_FLAG_OP
            }
            Increment(register) => {
                let value = register.read(self);
                let result = value.wrapping_add(1);
                register.write(self, result);
                self.set_inc_flags(value, result);
                CYCLES_INC_DEC_R
            }
            Decrement(register) => {
                let value = register.read(self);
                let result = value.wrapping_sub(1);
                register.write(self, result);
                self.set_dec_flags(value, result);
                CYCLES_INC_DEC_R
            }
            Move {
                destination,
                source,
            } => {
                let value = source.read(self);
                destination.write(self, value);
                CYCLES_LD_R_R
            }
            Alu { group, source } => {
                let value = source.read(self);
                self.execute_alu_group(group, value);
                CYCLES_ALU_R
            }
            Exx => {
                self.exchange_shadow_registers();
                CYCLES_EXX
            }
            JumpHl => {
                self.regs.pc = self.regs.hl();
                CYCLES_JP_HL
            }
            ExchangeDeHl => {
                self.exchange_de_hl();
                CYCLES_EX_DE_HL
            }
            DisableInterrupts => {
                self.disable_interrupts();
                CYCLES_DI_EI
            }
        })
    }
}
