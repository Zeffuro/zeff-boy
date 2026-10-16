use super::flags::sz53_flags;
use super::*;

mod state;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Io {
    port: u8,
    register: u8,
    output: Option<u8>,
}

impl Io {
    pub(super) fn decode<B: Z80Bus>(cpu: &Cpu, bus: &B, opcode: u8) -> Option<Self> {
        if opcode & 0xc6 != 0x40 {
            return None;
        }
        let register = (opcode >> 3) & 7;
        let output = (opcode & 1 != 0).then(|| {
            if register == REGISTER_MEMORY_INDEX {
                0
            } else {
                cpu.read_reg8(bus, register)
            }
        });
        Some(Self {
            port: cpu.regs.c,
            register,
            output,
        })
    }

    pub(super) fn execute<B: Z80Bus>(self, cpu: &mut Cpu, bus: &mut B, offset: u32) {
        if let Some(value) = self.output {
            cpu.write_io(bus, self.port, value, offset);
        } else {
            let value = bus.io_read(self.port);
            if self.register != REGISTER_MEMORY_INDEX {
                cpu.write_reg8(bus, self.register, value);
            }
            cpu.set_szp_flags_preserving_carry(value);
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Internal {
    opcode: u8,
    final_phase: bool,
}

impl Internal {
    pub(super) fn decode(opcode: u8) -> Option<Self> {
        let final_phase = match opcode {
            0x42 | 0x52 | 0x62 | 0x72 | 0x4a | 0x5a | 0x6a | 0x7a => false,
            0x47 | 0x4f | 0x57 | 0x5f => true,
            _ => return None,
        };
        Some(Self {
            opcode,
            final_phase,
        })
    }

    pub(super) fn step<B: Z80Bus>(self, cpu: &mut Cpu, bus: &mut B) -> (Option<Self>, u32) {
        if !self.final_phase {
            return (
                Some(Self {
                    final_phase: true,
                    ..self
                }),
                CYCLES_ED_ALU_INTERNAL,
            );
        }
        cpu.execute_ed_terminal(bus, cpu.last_opcode_pc, self.opcode);
        (
            None,
            if self.opcode & 7 == 2 {
                CYCLES_ED_ALU_FINISH
            } else {
                CYCLES_ED_SPECIAL_INTERNAL
            },
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Block {
    opcode: u8,
    pc: u16,
    address: u16,
    destination: u16,
    port: u8,
    phase: BlockPhase,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BlockPhase {
    Lead,
    Read,
    Write(u8),
    Finish(u8),
    Repeat(u8),
}

impl Block {
    pub(super) fn decode(cpu: &Cpu, pc: u16, opcode: u8) -> Option<Self> {
        (opcode & 0xe4 == 0xa0).then_some(Self {
            opcode,
            pc,
            address: cpu.regs.hl(),
            destination: cpu.regs.de(),
            port: cpu.regs.c,
            phase: if opcode & 2 != 0 {
                BlockPhase::Lead
            } else {
                BlockPhase::Read
            },
        })
    }

    pub(super) fn step<B: Z80Bus>(
        self,
        cpu: &mut Cpu,
        bus: &mut B,
        offset: u32,
    ) -> (Option<Self>, u32) {
        match self.phase {
            BlockPhase::Lead => (
                Some(Self {
                    phase: BlockPhase::Read,
                    ..self
                }),
                CYCLES_ED_BLOCK_IO_INTERNAL,
            ),
            BlockPhase::Read => {
                let input = self.opcode & 3 == 2;
                let value = if input {
                    bus.io_read(self.port)
                } else {
                    bus.cpu_read(self.address)
                };
                (
                    Some(Self {
                        phase: if self.opcode & 3 == 1 {
                            BlockPhase::Finish(value)
                        } else {
                            BlockPhase::Write(value)
                        },
                        ..self
                    }),
                    if input {
                        CYCLES_IO_MACHINE
                    } else {
                        CYCLES_MEMORY_TRANSFER
                    },
                )
            }
            BlockPhase::Write(value) => {
                let kind = self.opcode & 3;
                if kind == 3 {
                    cpu.write_io(bus, self.port, value, offset);
                } else {
                    bus.cpu_write(
                        if kind == 0 {
                            self.destination
                        } else {
                            self.address
                        },
                        value,
                    );
                }
                (
                    if kind == 0 {
                        Some(Self {
                            phase: BlockPhase::Finish(value),
                            ..self
                        })
                    } else {
                        self.finish(cpu, value)
                    },
                    if kind == 3 {
                        CYCLES_IO_MACHINE
                    } else {
                        CYCLES_MEMORY_TRANSFER
                    },
                )
            }
            BlockPhase::Finish(value) => (
                self.finish(cpu, value),
                if self.opcode & 3 == 0 {
                    CYCLES_ED_BLOCK_COPY_INTERNAL
                } else {
                    CYCLES_ED_BLOCK_COMPARE_INTERNAL
                },
            ),
            BlockPhase::Repeat(value) => {
                cpu.regs.pc = self.pc;
                let xy = Z80_FLAG_BIT_5 | Z80_FLAG_BIT_3;
                cpu.regs.f = (cpu.regs.f & !xy) | ((self.pc >> 8) as u8 & xy);
                if self.opcode & 2 != 0 {
                    let carry = cpu.regs.f & Z80_FLAG_CARRY != 0;
                    let negative = value & Z80_FLAG_SIGN != 0;
                    let b = cpu.regs.b;
                    let adjusted = if !carry {
                        b
                    } else if negative {
                        b.wrapping_sub(1)
                    } else {
                        b.wrapping_add(1)
                    };
                    let parity = (adjusted & 7).count_ones().is_multiple_of(2);
                    let overflow = cpu.regs.f & Z80_FLAG_PARITY_OVERFLOW != 0;
                    cpu.regs.f &= !(Z80_FLAG_HALF_CARRY | Z80_FLAG_PARITY_OVERFLOW);
                    if parity == overflow {
                        cpu.regs.f |= Z80_FLAG_PARITY_OVERFLOW;
                    }
                    if carry && b & 0x0f == if negative { 0 } else { 0x0f } {
                        cpu.regs.f |= Z80_FLAG_HALF_CARRY;
                    }
                }
                (None, CYCLES_ED_BLOCK_REPEAT_INTERNAL)
            }
        }
    }

    fn finish(self, cpu: &mut Cpu, value: u8) -> Option<Self> {
        let delta = if self.opcode & 8 == 0 { 1 } else { -1 };
        cpu.regs.set_hl(self.address.wrapping_add_signed(delta));
        let kind = self.opcode & 3;
        let repeats = match kind {
            0 => {
                cpu.regs.set_de(self.destination.wrapping_add_signed(delta));
                cpu.regs.set_bc(cpu.regs.bc().wrapping_sub(1));
                cpu.regs.f = (cpu.regs.f & (Z80_FLAG_SIGN | Z80_FLAG_ZERO | Z80_FLAG_CARRY))
                    | block_xy(cpu.regs.a.wrapping_add(value))
                    | if cpu.regs.bc() != 0 {
                        Z80_FLAG_PARITY_OVERFLOW
                    } else {
                        0
                    };
                cpu.regs.bc() != 0
            }
            1 => {
                let result = cpu.regs.a.wrapping_sub(value);
                let half = (cpu.regs.a & 0x0f) < (value & 0x0f);
                cpu.regs.set_bc(cpu.regs.bc().wrapping_sub(1));
                cpu.regs.f = (cpu.regs.f & Z80_FLAG_CARRY)
                    | (sz53_flags(result) & (Z80_FLAG_SIGN | Z80_FLAG_ZERO))
                    | block_xy(result.wrapping_sub(u8::from(half)))
                    | Z80_FLAG_SUBTRACT
                    | if half { Z80_FLAG_HALF_CARRY } else { 0 }
                    | if cpu.regs.bc() != 0 {
                        Z80_FLAG_PARITY_OVERFLOW
                    } else {
                        0
                    };
                cpu.regs.bc() != 0 && result != 0
            }
            _ => {
                cpu.regs.b = cpu.regs.b.wrapping_sub(1);
                let addend = if kind == 2 {
                    self.port.wrapping_add_signed(delta as i8)
                } else {
                    cpu.regs.l
                };
                let sum = u16::from(value) + u16::from(addend);
                cpu.regs.f = sz53_flags(cpu.regs.b)
                    | if value & Z80_FLAG_SIGN != 0 {
                        Z80_FLAG_SUBTRACT
                    } else {
                        0
                    }
                    | if sum > u16::from(u8::MAX) {
                        Z80_FLAG_HALF_CARRY | Z80_FLAG_CARRY
                    } else {
                        0
                    }
                    | (flags::szp_flags((sum as u8 & 7) ^ cpu.regs.b) & Z80_FLAG_PARITY_OVERFLOW);
                cpu.regs.b != 0
            }
        };
        (self.opcode & 0x10 != 0 && repeats).then_some(Self {
            phase: BlockPhase::Repeat(value),
            ..self
        })
    }

    pub(super) fn execute<B: Z80Bus>(self, cpu: &mut Cpu, bus: &mut B) -> u32 {
        let mut pending = Some(self);
        let mut cycles = 2 * CYCLES_M1;
        let mut offset = cpu.ed_io_t_states_before();
        while let Some(operation) = pending {
            let (next, clocks) = operation.step(cpu, bus, offset);
            pending = next;
            cycles += clocks;
            offset += clocks;
        }
        cycles
    }
}

fn block_xy(value: u8) -> u8 {
    (value & Z80_FLAG_BIT_3) | ((value << 4) & Z80_FLAG_BIT_5)
}

impl Cpu {
    pub(super) fn execute_ed<B: Z80Bus>(&mut self, bus: &mut B, pc: u16) -> u32 {
        let opcode = self.fetch_u8(bus);
        self.increment_refresh_register();
        self.execute_ed_terminal(bus, pc, opcode)
    }

    pub(super) fn execute_ed_terminal<B: Z80Bus>(
        &mut self,
        bus: &mut B,
        pc: u16,
        opcode: u8,
    ) -> u32 {
        if let Some(load) = load::Load::decode_ed(self, opcode) {
            return load.execute(self, bus) + CYCLES_M1;
        }
        if let Some(operation) = stack::Stack::decode_ed(self, opcode) {
            return operation.execute(self, bus) + CYCLES_M1;
        }
        if let Some(operation) = rmw::Rmw::decode_ed(self, opcode) {
            operation.execute(self, bus);
            return CYCLES_ED_NIBBLE_ROTATE;
        }
        if let Some(operation) = Io::decode(self, bus, opcode) {
            operation.execute(self, bus, self.ed_io_t_states_before());
            return CYCLES_ED_IO;
        }
        if let Some(operation) = Block::decode(self, pc, opcode) {
            return operation.execute(self, bus);
        }
        match opcode {
            0x42 | 0x52 | 0x62 | 0x72 => {
                let pair = (opcode >> 4) & 0x03;
                self.sbc_hl(self.read_reg16(pair));
                CYCLES_ED_16BIT_ALU
            }
            0x44 | 0x4C | 0x54 | 0x5C | 0x64 | 0x6C | 0x74 | 0x7C => {
                self.neg_a();
                CYCLES_ED_NEG
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
            0x4F => {
                self.regs.r = self.regs.a;
                CYCLES_ED_SPECIAL_REGISTER
            }
            0x57 => {
                self.load_a_from_special_register(self.regs.i);
                CYCLES_ED_SPECIAL_REGISTER
            }
            0x5F => {
                self.load_a_from_special_register(self.regs.r);
                CYCLES_ED_SPECIAL_REGISTER
            }
            0x56 | 0x76 => {
                self.interrupt_mode = InterruptMode::Im1;
                CYCLES_IM
            }
            0x5E | 0x7E => {
                self.interrupt_mode = InterruptMode::Im2;
                CYCLES_IM
            }
            _ => CYCLES_ED_NOP,
        }
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
}

fn flags_from_u16_result(value: u16) -> u8 {
    let hi = (value >> 8) as u8;
    (hi & (Z80_FLAG_SIGN | Z80_FLAG_BIT_5 | Z80_FLAG_BIT_3))
        | if value == 0 { Z80_FLAG_ZERO } else { 0 }
}
