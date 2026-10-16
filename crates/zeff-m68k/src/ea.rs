use crate::Registers;
use crate::bus::{
    AccessKind, BusFault, FunctionCode, M68kBus, read_byte, read_long, read_word, write_byte,
    write_long, write_word,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Size {
    Byte,
    Word,
    Long,
}

impl Size {
    pub const fn bytes(self) -> u32 {
        match self {
            Self::Byte => 1,
            Self::Word => 2,
            Self::Long => 4,
        }
    }

    pub const fn mask(self) -> u32 {
        match self {
            Self::Byte => 0xff,
            Self::Word => 0xffff,
            Self::Long => u32::MAX,
        }
    }

    pub const fn sign_bit(self) -> u32 {
        match self {
            Self::Byte => 0x80,
            Self::Word => 0x8000,
            Self::Long => 0x8000_0000,
        }
    }

    pub const fn truncate(self, value: u32) -> u32 {
        value & self.mask()
    }

    pub const fn sign_extend(self, value: u32) -> u32 {
        match self {
            Self::Byte => value as u8 as i8 as i32 as u32,
            Self::Word => value as u16 as i16 as i32 as u32,
            Self::Long => value,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Operand {
    DataRegister(usize),
    AddressRegister(usize),
    Memory {
        address: u32,
        function_code: FunctionCode,
        predecrement: bool,
    },
    Immediate(u32),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EaError<E> {
    Bus(BusFault<E>),
    InvalidEncoding,
}

pub fn program_space(regs: &Registers) -> FunctionCode {
    if regs.supervisor() {
        FunctionCode::SupervisorProgram
    } else {
        FunctionCode::UserProgram
    }
}

pub fn data_space(regs: &Registers) -> FunctionCode {
    if regs.supervisor() {
        FunctionCode::SupervisorData
    } else {
        FunctionCode::UserData
    }
}

pub fn fetch_word<B: M68kBus>(
    bus: &mut B,
    cursor: &mut u32,
    program: FunctionCode,
) -> Result<u16, EaError<B::Error>> {
    let word = read_word(bus, *cursor, program, AccessKind::Extension).map_err(EaError::Bus)?;
    *cursor = cursor.wrapping_add(2);
    Ok(word)
}

fn fetch_long<B: M68kBus>(
    bus: &mut B,
    cursor: &mut u32,
    program: FunctionCode,
) -> Result<u32, EaError<B::Error>> {
    let high = fetch_word(bus, cursor, program)?;
    let low = fetch_word(bus, cursor, program)?;
    Ok((u32::from(high) << 16) | u32::from(low))
}

fn indexed<E>(regs: &Registers, base: u32, extension: u16) -> Result<u32, EaError<E>> {
    // The 68000 ignores extension bits 10..8.
    let index = usize::from((extension >> 12) & 7);
    let value = if extension & 0x8000 == 0 {
        regs.d[index]
    } else {
        regs.address_register(index)
            .ok_or(EaError::InvalidEncoding)?
    };
    let value = if extension & 0x0800 == 0 {
        Size::Word.sign_extend(value)
    } else {
        value
    };
    Ok(base
        .wrapping_add(value)
        .wrapping_add_signed(i32::from(extension as u8 as i8)))
}

pub fn resolve<B: M68kBus>(
    regs: &mut Registers,
    bus: &mut B,
    cursor: &mut u32,
    mode: u16,
    reg: u16,
    size: Size,
) -> Result<Operand, EaError<B::Error>> {
    if mode > 7 || reg > 7 {
        return Err(EaError::InvalidEncoding);
    }
    let index = usize::from(reg);
    match mode {
        0 => return Ok(Operand::DataRegister(index)),
        1 => return Ok(Operand::AddressRegister(index)),
        _ => {}
    }
    let program = program_space(regs);
    let mut function_code = data_space(regs);
    let address = if mode == 7 {
        match reg {
            0 => Size::Word.sign_extend(u32::from(fetch_word(bus, cursor, program)?)),
            1 => fetch_long(bus, cursor, program)?,
            2 => {
                function_code = program;
                let base = *cursor;
                let displacement = fetch_word(bus, cursor, program)? as i16;
                base.wrapping_add_signed(i32::from(displacement))
            }
            3 => {
                function_code = program;
                let base = *cursor;
                indexed(regs, base, fetch_word(bus, cursor, program)?)?
            }
            4 => {
                let value = if size == Size::Long {
                    fetch_long(bus, cursor, program)?
                } else {
                    u32::from(fetch_word(bus, cursor, program)?)
                };
                return Ok(Operand::Immediate(size.truncate(value)));
            }
            _ => return Err(EaError::InvalidEncoding),
        }
    } else {
        let base = regs
            .address_register(index)
            .ok_or(EaError::InvalidEncoding)?;
        let step = if index == 7 && size == Size::Byte {
            2
        } else {
            size.bytes()
        };
        match mode {
            2 => base,
            3 => {
                regs.set_address_register(index, base.wrapping_add(step));
                base
            }
            4 => {
                let address = base.wrapping_sub(step);
                regs.set_address_register(index, address);
                address
            }
            5 => {
                let displacement = fetch_word(bus, cursor, program)? as i16;
                base.wrapping_add_signed(i32::from(displacement))
            }
            6 => indexed(regs, base, fetch_word(bus, cursor, program)?)?,
            _ => return Err(EaError::InvalidEncoding),
        }
    };
    Ok(Operand::Memory {
        address,
        function_code,
        predecrement: mode == 4,
    })
}

pub fn read<B: M68kBus>(
    regs: &Registers,
    bus: &mut B,
    operand: Operand,
    size: Size,
) -> Result<u32, EaError<B::Error>> {
    let value = match operand {
        Operand::DataRegister(index) => *regs.d.get(index).ok_or(EaError::InvalidEncoding)?,
        Operand::AddressRegister(index) => regs
            .address_register(index)
            .ok_or(EaError::InvalidEncoding)?,
        Operand::Immediate(value) => value,
        Operand::Memory {
            address,
            function_code,
            ..
        } => match size {
            Size::Byte => u32::from(
                read_byte(bus, address, function_code, AccessKind::Data).map_err(EaError::Bus)?,
            ),
            Size::Word => u32::from(
                read_word(bus, address, function_code, AccessKind::Data).map_err(EaError::Bus)?,
            ),
            Size::Long => {
                read_long(bus, address, function_code, AccessKind::Data).map_err(EaError::Bus)?
            }
        },
    };
    Ok(size.truncate(value))
}

pub fn write<B: M68kBus>(
    regs: &mut Registers,
    bus: &mut B,
    operand: Operand,
    size: Size,
    value: u32,
) -> Result<(), EaError<B::Error>> {
    match operand {
        Operand::DataRegister(index) => {
            let register = regs.d.get_mut(index).ok_or(EaError::InvalidEncoding)?;
            *register = (*register & !size.mask()) | size.truncate(value);
            Ok(())
        }
        Operand::AddressRegister(index) => {
            if size == Size::Byte || !regs.set_address_register(index, size.sign_extend(value)) {
                return Err(EaError::InvalidEncoding);
            }
            Ok(())
        }
        Operand::Immediate(_) => Err(EaError::InvalidEncoding),
        Operand::Memory {
            address,
            function_code,
            predecrement,
        } => {
            let result = match size {
                Size::Byte => {
                    write_byte(bus, address, value as u8, function_code, AccessKind::Data)
                }
                Size::Word => {
                    write_word(bus, address, value as u16, function_code, AccessKind::Data)
                }
                Size::Long if predecrement => {
                    // The original 68000 stores the low word first for predecrement.
                    write_word(
                        bus,
                        address.wrapping_add(2),
                        value as u16,
                        function_code,
                        AccessKind::Data,
                    )
                    .map_err(EaError::Bus)?;
                    write_word(
                        bus,
                        address,
                        (value >> 16) as u16,
                        function_code,
                        AccessKind::Data,
                    )
                }
                Size::Long => write_long(bus, address, value, function_code, AccessKind::Data),
            };
            result.map_err(EaError::Bus)
        }
    }
}

/// Address calculation and operand-read clocks.
pub fn read_cycles(mode: u16, reg: u16, size: Size) -> Option<u32> {
    if reg > 7 {
        return None;
    }
    let word_cycles = match (mode, reg) {
        (0 | 1, _) => return Some(0),
        (2 | 3, _) => 4,
        (4, _) => 6,
        (5, _) | (7, 0 | 2) => 8,
        (6, _) | (7, 3) => 10,
        (7, 1) => 12,
        (7, 4) => 4,
        _ => return None,
    };
    Some(word_cycles + if size == Size::Long { 4 } else { 0 })
}
