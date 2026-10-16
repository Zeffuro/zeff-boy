use super::*;

mod state;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Rmw {
    address: u16,
    register: Option<u8>,
    phase: Phase,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Read(Operation),
    DecimalInternal { value: u8, flags: u8, a: u8 },
    Write { value: u8, flags: u8, a: Option<u8> },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Operation {
    Increment,
    Decrement,
    Cb(u8),
    Decimal { opcode: u8, a: u8, carry: u8 },
}

impl Rmw {
    pub(super) fn decode_ed(cpu: &Cpu, opcode: u8) -> Option<Self> {
        matches!(opcode, 0x67 | 0x6f).then_some(Self {
            address: cpu.regs.hl(),
            register: None,
            phase: Phase::Read(Operation::Decimal {
                opcode,
                a: cpu.regs.a,
                carry: cpu.regs.f & Z80_FLAG_CARRY,
            }),
        })
    }

    pub(super) fn inc_dec(cpu: &Cpu, opcode: u8) -> Option<Self> {
        Self::inc_dec_at(cpu.regs.hl(), opcode)
    }

    pub(super) fn inc_dec_at(address: u16, opcode: u8) -> Option<Self> {
        let operation = match opcode {
            0x34 => Operation::Increment,
            0x35 => Operation::Decrement,
            _ => return None,
        };
        Some(Self {
            address,
            register: None,
            phase: Phase::Read(operation),
        })
    }

    pub(super) fn cb(cpu: &Cpu, opcode: u8) -> Self {
        Self {
            address: cpu.regs.hl(),
            register: None,
            phase: Phase::Read(Operation::Cb(opcode)),
        }
    }

    pub(super) fn indexed_cb(address: u16, opcode: u8) -> Self {
        Self {
            address,
            register: Some(opcode & 7),
            phase: Phase::Read(Operation::Cb(opcode)),
        }
    }

    pub(super) fn step<B: Z80Bus>(self, cpu: &mut Cpu, bus: &mut B) -> (Option<Self>, u32) {
        match self.phase {
            Phase::Read(operation) => {
                let value = bus.cpu_read(self.address);
                if let Operation::Decimal { opcode, a, carry } = operation {
                    let (value, a) = if opcode == 0x67 {
                        (
                            ((a & 0x0f) << 4) | (value >> 4),
                            (a & 0xf0) | (value & 0x0f),
                        )
                    } else {
                        ((value << 4) | (a & 0x0f), (a & 0xf0) | (value >> 4))
                    };
                    return (
                        Some(Self {
                            phase: Phase::DecimalInternal {
                                value,
                                flags: carry | flags::szp_flags(a),
                                a,
                            },
                            ..self
                        }),
                        CYCLES_MEMORY_TRANSFER,
                    );
                }
                let (value, mut flags, write) = match operation {
                    Operation::Increment => {
                        let result = value.wrapping_add(1);
                        (result, alu::inc_flags(value, result, cpu.regs.f), true)
                    }
                    Operation::Decrement => {
                        let result = value.wrapping_sub(1);
                        (result, alu::dec_flags(value, result, cpu.regs.f), true)
                    }
                    Operation::Cb(opcode) => cb::effect(opcode, value, cpu.regs.f),
                    Operation::Decimal { .. } => unreachable!(),
                };
                if !write && self.register.is_some() {
                    let xy = Z80_FLAG_BIT_5 | Z80_FLAG_BIT_3;
                    flags = (flags & !xy) | ((self.address >> 8) as u8 & xy);
                }
                if write {
                    (
                        Some(Self {
                            address: self.address,
                            register: self.register,
                            phase: Phase::Write {
                                value,
                                flags,
                                a: None,
                            },
                        }),
                        CYCLES_MEMORY_RMW_READ,
                    )
                } else {
                    cpu.regs.f = flags;
                    (None, CYCLES_MEMORY_RMW_READ)
                }
            }
            Phase::DecimalInternal { value, flags, a } => (
                Some(Self {
                    phase: Phase::Write {
                        value,
                        flags,
                        a: Some(a),
                    },
                    ..self
                }),
                CYCLES_ED_NIBBLE_INTERNAL,
            ),
            Phase::Write { value, flags, a } => {
                bus.cpu_write(self.address, value);
                if let Some(register) = self.register.filter(|&r| r != REGISTER_MEMORY_INDEX) {
                    cpu.write_reg8(bus, register, value);
                }
                if let Some(a) = a {
                    cpu.regs.a = a;
                }
                cpu.regs.f = flags;
                (None, CYCLES_MEMORY_TRANSFER)
            }
        }
    }

    pub(super) fn execute<B: Z80Bus>(self, cpu: &mut Cpu, bus: &mut B) {
        let mut pending = Some(self);
        while let Some(operation) = pending {
            pending = operation.step(cpu, bus).0;
        }
    }
}
