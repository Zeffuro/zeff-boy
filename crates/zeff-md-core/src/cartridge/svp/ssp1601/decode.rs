use super::Fault;

#[derive(Clone, Copy)]
pub(super) struct Pointer {
    pub index: u8,
    pub modifier: u8,
}

impl Pointer {
    fn operand(word: u16) -> Self {
        Self {
            index: ((word >> 6) & 4) as u8 | (word & 3) as u8,
            modifier: ((word >> 2) & 3) as u8,
        }
    }
}

#[derive(Clone, Copy)]
pub(super) enum Source {
    Register(u8),
    Indirect(Pointer),
    Double(Pointer),
    Direct { bank: u8, address: u8 },
    Pointer(u8),
    Immediate,
    Short(u8),
    ProgramA,
}

#[derive(Clone, Copy)]
pub(super) enum Target {
    Register(u8),
    Indirect(Pointer),
    Direct { bank: u8, address: u8 },
    Pointer(u8),
}

#[derive(Clone, Copy)]
pub(super) enum Alu {
    Sub,
    Compare,
    Add,
    And,
    Or,
    Xor,
}

#[derive(Clone, Copy)]
pub(super) enum Multiply {
    Load,
    Add,
    Sub,
}

pub(super) enum Instruction {
    Transfer {
        to: Target,
        from: Source,
    },
    Alu {
        operation: Alu,
        from: Source,
    },
    Branch {
        call: bool,
        condition: u8,
        expected: bool,
    },
    Modify {
        operation: u8,
        condition: u8,
        expected: bool,
    },
    Multiply {
        operation: Multiply,
        x: Pointer,
        y: Pointer,
    },
}

fn condition(word: u16) -> Result<(u8, bool), Fault> {
    let code = ((word >> 4) & 15) as u8;
    let expected = word & 0x0100 != 0;
    if code == 1 || code > 7 || (code == 0 && expected) {
        return Err(Fault::Opcode(word));
    }
    Ok((code, expected))
}

pub(super) fn decode(word: u16) -> Result<Instruction, Fault> {
    let register = ((word >> 4) & 15) as u8;
    let pointer = Pointer::operand(word);
    let direct = Source::Direct {
        bank: ((word >> 8) & 1) as u8,
        address: word as u8,
    };
    let transfer = |to, from| Ok(Instruction::Transfer { to, from });
    match word & 0xfe00 {
        0x0000 if word & 0x0100 == 0 => {
            return transfer(
                Target::Register(register),
                Source::Register((word & 15) as u8),
            );
        }
        0x0200 => return transfer(Target::Register(register), Source::Indirect(pointer)),
        0x0400 => return transfer(Target::Indirect(pointer), Source::Register(register)),
        0x0600 => return transfer(Target::Register(3), direct),
        0x0800 if word & 0x010f == 0 => {
            return transfer(Target::Register(register), Source::Immediate);
        }
        0x0a00 => return transfer(Target::Register(register), Source::Double(pointer)),
        0x0c00 if word & 0x00f0 == 0 => {
            return transfer(Target::Indirect(pointer), Source::Immediate);
        }
        0x0e00 => {
            return transfer(
                Target::Direct {
                    bank: ((word >> 8) & 1) as u8,
                    address: word as u8,
                },
                Source::Register(3),
            );
        }
        0x1200 if word & 0x000c == 0 => {
            return transfer(Target::Register(register), Source::Pointer(pointer.index));
        }
        0x1400 if word & 0x000c == 0 => {
            return transfer(Target::Pointer(pointer.index), Source::Register(register));
        }
        0x4800 | 0x4c00 if word & 15 == 0 => {
            let (condition, expected) = condition(word)?;
            return Ok(Instruction::Branch {
                call: word & 0x0400 == 0,
                condition,
                expected,
            });
        }
        0x4a00 if word & 0x010f == 0 => {
            return transfer(Target::Register(register), Source::ProgramA);
        }
        0x9000 if word & 8 == 0 && matches!(word & 7, 2 | 3 | 6 | 7) => {
            let (condition, expected) = condition(word)?;
            return Ok(Instruction::Modify {
                operation: (word & 7) as u8,
                condition,
                expected,
            });
        }
        _ => {}
    }
    if word & 0xf800 == 0x1800 {
        return transfer(
            Target::Pointer(((word >> 8) & 7) as u8),
            Source::Short(word as u8),
        );
    }
    let multiply = match word & 0xff00 {
        0x3700 => Some(Multiply::Sub),
        0x9700 => Some(Multiply::Add),
        0xb700 => Some(Multiply::Load),
        _ => None,
    };
    if let Some(operation) = multiply {
        return Ok(Instruction::Multiply {
            operation,
            x: Pointer {
                index: (word & 3) as u8,
                modifier: ((word >> 2) & 3) as u8,
            },
            y: Pointer {
                index: 4 | ((word >> 4) & 3) as u8,
                modifier: ((word >> 6) & 3) as u8,
            },
        });
    }
    let operation = match word >> 13 {
        1 => Alu::Sub,
        3 => Alu::Compare,
        4 => Alu::Add,
        5 => Alu::And,
        6 => Alu::Or,
        7 => Alu::Xor,
        _ => return Err(Fault::Opcode(word)),
    };
    let operand = word & 0x1fff;
    let from = match operand & 0x1e00 {
        0x0000 if operand & 0x01f0 == 0 => Source::Register((word & 15) as u8),
        0x0200 if operand & 0x00f0 == 0 => Source::Indirect(pointer),
        0x0600 => direct,
        0x0800 if operand & 0x01ff == 0 => Source::Immediate,
        0x0a00 if operand & 0x00f0 == 0 => Source::Double(pointer),
        0x1200 if operand & 0x00fc == 0 => Source::Pointer(pointer.index),
        0x1800 if operand & 0x0100 == 0 => Source::Short(word as u8),
        _ => return Err(Fault::Opcode(word)),
    };
    Ok(Instruction::Alu { operation, from })
}
