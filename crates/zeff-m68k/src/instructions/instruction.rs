use crate::bus::{AccessKind, M68kBus, read_long, write_long};
use crate::constants::SHORT_EXCEPTION_CLOCKS;
use crate::ea::{self, EaError, Operand, Size};
use crate::{Fault, Registers};

pub(crate) struct Context<'a, B: M68kBus> {
    pub regs: &'a mut Registers,
    pub bus: &'a mut B,
    pub pc: u32,
    pub opcode: u16,
    pub cursor: u32,
    pub next_pc: Option<u32>,
    pub exception: Option<u8>,
    pub stopped: bool,
}

pub(crate) struct Execution {
    pub nominal_cycles: u32,
    pub exception: Option<u8>,
    pub stopped: bool,
}

impl<B: M68kBus> Context<'_, B> {
    pub fn unsupported(&self) -> Fault<B::Error> {
        Fault::UnsupportedInstruction {
            pc: self.pc,
            opcode: self.opcode,
        }
    }

    pub fn map<T>(&self, result: Result<T, EaError<B::Error>>) -> Result<T, Fault<B::Error>> {
        result.map_err(|error| match error {
            EaError::Bus(error) => Fault::Bus(error),
            EaError::InvalidEncoding => self.unsupported(),
        })
    }

    pub fn fetch(&mut self) -> Result<u16, Fault<B::Error>> {
        let result = ea::fetch_word(self.bus, &mut self.cursor, ea::program_space(self.regs));
        self.map(result)
    }

    pub fn resolve(&mut self, mode: u16, reg: u16, size: Size) -> Result<Operand, Fault<B::Error>> {
        let result = ea::resolve(self.regs, self.bus, &mut self.cursor, mode, reg, size);
        self.map(result)
    }

    pub fn read(&mut self, operand: Operand, size: Size) -> Result<u32, Fault<B::Error>> {
        let result = ea::read(self.regs, self.bus, operand, size);
        self.map(result)
    }

    pub fn write(
        &mut self,
        operand: Operand,
        size: Size,
        value: u32,
    ) -> Result<(), Fault<B::Error>> {
        let result = ea::write(self.regs, self.bus, operand, size, value);
        self.map(result)
    }

    pub fn ea_cycles(&self, mode: u16, reg: u16, size: Size) -> Result<u32, Fault<B::Error>> {
        ea::read_cycles(mode, reg, size).ok_or_else(|| self.unsupported())
    }

    pub fn push(&mut self, value: u32) -> Result<(), Fault<B::Error>> {
        let address = self.regs.stack_pointer().wrapping_sub(4);
        self.regs.set_address_register(7, address);
        write_long(
            self.bus,
            address,
            value,
            ea::data_space(self.regs),
            AccessKind::Stack,
        )
        .map_err(Fault::Bus)
    }

    pub fn pop(&mut self) -> Result<u32, Fault<B::Error>> {
        let address = self.regs.stack_pointer();
        let value = read_long(
            self.bus,
            address,
            ea::data_space(self.regs),
            AccessKind::Stack,
        )
        .map_err(Fault::Bus)?;
        self.regs.set_address_register(7, address.wrapping_add(4));
        Ok(value)
    }

    pub fn exception(&mut self, vector: u8, saved_pc: u32) -> Result<u32, Fault<B::Error>> {
        crate::exception::enter(self.regs, self.bus, vector, saved_pc)?;
        self.next_pc = Some(self.regs.pc);
        self.exception = Some(vector);
        Ok(SHORT_EXCEPTION_CLOCKS)
    }
}

pub(crate) fn size(opcode: u16) -> Option<Size> {
    match (opcode >> 6) & 3 {
        0 => Some(Size::Byte),
        1 => Some(Size::Word),
        2 => Some(Size::Long),
        _ => None,
    }
}

pub(crate) fn data_source(mode: u16, reg: u16) -> bool {
    mode == 0 || (2..=6).contains(&mode) || mode == 7 && reg <= 4
}

pub(crate) fn alterable(mode: u16, reg: u16) -> bool {
    mode == 0 || (2..=6).contains(&mode) || mode == 7 && reg <= 1
}

pub(crate) fn memory_alterable(mode: u16, reg: u16) -> bool {
    mode != 0 && alterable(mode, reg)
}

pub(crate) fn control_address(mode: u16, reg: u16) -> bool {
    matches!(mode, 2 | 5 | 6) || mode == 7 && reg <= 3
}

pub(crate) fn execute<B: M68kBus>(
    regs: &mut Registers,
    bus: &mut B,
    pc: u32,
    opcode: u16,
) -> Result<Execution, Fault<B::Error>> {
    let mut context = Context {
        regs,
        bus,
        pc,
        opcode,
        cursor: pc.wrapping_add(2),
        next_pc: None,
        exception: None,
        stopped: false,
    };
    let cycles = if opcode == 0x4e71 {
        4
    } else if opcode & 0xf100 == 0x7000 {
        let value = i32::from(opcode as u8 as i8) as u32;
        context.regs.d[usize::from((opcode >> 9) & 7)] = value;
        crate::alu::logic(context.regs, Size::Long, value);
        4
    } else if let Some(cycles) = crate::control::execute(&mut context)? {
        cycles
    } else if let Some(cycles) = crate::bus_instruction::execute(&mut context)? {
        cycles
    } else if let Some(cycles) = crate::transfer::execute(&mut context)? {
        cycles
    } else if let Some(cycles) = crate::shift::execute(&mut context)? {
        cycles
    } else if let Some(cycles) = crate::bit::execute(&mut context)? {
        cycles
    } else if let Some(cycles) = crate::multiply::execute(&mut context)? {
        cycles
    } else if let Some(cycles) = crate::data::execute(&mut context)? {
        cycles
    } else {
        return Err(context.unsupported());
    };
    context.regs.pc = context.next_pc.unwrap_or(context.cursor);
    Ok(Execution {
        nominal_cycles: cycles,
        exception: context.exception,
        stopped: context.stopped,
    })
}
