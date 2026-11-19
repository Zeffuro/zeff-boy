use super::decode::{Source, Target};
use super::{ADMITTED_ST, Core, FLAGS, Fault, INPUTS, Ports, ProgramRole, Step};

impl Core {
    pub(super) fn source<P: Ports>(
        &mut self,
        source: Source,
        blind: bool,
        program: &impl Fn(u16, ProgramRole) -> Result<u16, Fault>,
        ports: &mut P,
        step: &mut Step,
    ) -> Result<u16, Fault> {
        match source {
            Source::Register(index) => self.read_register(index, blind, ports),
            Source::Indirect(pointer) => {
                let (bank, address) = self.pointer_address(pointer, false)?;
                Ok(self.ram[bank][address])
            }
            Source::Double(pointer) => {
                let (bank, address) = self.double_address(pointer)?;
                let program_address = self.ram[bank][address];
                let value = Self::program_data(program_address, program, step)?;
                self.ram[bank][address] = program_address.wrapping_add(1);
                Ok(value)
            }
            Source::Direct { bank, address } => {
                Ok(self.ram[usize::from(bank)][usize::from(address)])
            }
            Source::Pointer(index) => self.pointer_read(index),
            Source::Immediate => self.fetch(program, step),
            Source::Short(value) => Ok(u16::from(value)),
            Source::ProgramA => {
                self.require_a(0xffff0000)?;
                Self::program_data((self.a >> 16) as u16, program, step)
            }
        }
    }

    pub(super) fn transfer<P: Ports>(
        &mut self,
        to: Target,
        from: Source,
        program: &impl Fn(u16, ProgramRole) -> Result<u16, Fault>,
        ports: &mut P,
        step: &mut Step,
    ) -> Result<(), Fault> {
        if matches!((to, from), (Target::Register(3), Source::Register(7))) {
            self.assign_a(self.p, u32::MAX);
            return Ok(());
        }
        if matches!((to, from), (Target::Register(3), Source::Register(4))) {
            let status = self.st | Self::inputs(ports)?;
            let known = u32::from(!FLAGS | self.flags_known) << 16;
            self.assign_a(
                (self.a & 0xffff) | (u32::from(status) << 16),
                (self.a_known & 0xffff) | known,
            );
            return Ok(());
        }
        if matches!((to, from), (Target::Register(4), Source::Register(3))) {
            self.require_a(u32::from(!FLAGS) << 16)?;
            let value = (self.a >> 16) as u16;
            if value & !ADMITTED_ST != 0 {
                return Err(Fault::StatusMode(value));
            }
            self.st = value;
            self.flags_known = (self.a_known >> 16) as u16 & FLAGS;
            return Ok(());
        }
        let blind = matches!(to, Target::Register(0));
        let value = self.source(from, blind, program, ports, step)?;
        match to {
            Target::Register(index) => {
                self.write_register(index, matches!(from, Source::Register(0)), value, ports)
            }
            Target::Indirect(pointer) => {
                let (bank, address) = self.pointer_address(pointer, true)?;
                self.ram[bank][address] = value;
                Ok(())
            }
            Target::Direct { bank, address } => {
                self.ram[usize::from(bank)][usize::from(address)] = value;
                Ok(())
            }
            Target::Pointer(index) => self.pointer_write(index, value),
        }
    }

    fn read_register<P: Ports>(
        &mut self,
        index: u8,
        blind: bool,
        ports: &mut P,
    ) -> Result<u16, Fault> {
        match index {
            0 => Ok(0xffff),
            1 => Ok(self.x),
            2 => Ok(self.y),
            3 => {
                self.require_a(0xffff0000)?;
                Ok((self.a >> 16) as u16)
            }
            4 => {
                if self.flags_known != FLAGS {
                    return Err(Fault::UnknownFlags(FLAGS & !self.flags_known));
                }
                Ok(self.st | Self::inputs(ports)?)
            }
            5 => self.pop(),
            6 => Err(Fault::RegisterRead(index)),
            7 => Ok((self.p >> 16) as u16),
            8..=14 => ports.read(index - 8, self.st, blind),
            15 if blind => ports.read(7, self.st, true),
            15 => {
                self.require_a(0x0000ffff)?;
                Ok(self.a as u16)
            }
            _ => Err(Fault::RegisterRead(index)),
        }
    }

    fn write_register<P: Ports>(
        &mut self,
        index: u8,
        blind: bool,
        value: u16,
        ports: &mut P,
    ) -> Result<(), Fault> {
        match index {
            0 => {}
            1 => {
                self.x = value;
                self.p = Self::product(self.x, self.y);
            }
            2 => {
                self.y = value;
                self.p = Self::product(self.x, self.y);
            }
            3 => self.assign_a(
                (self.a & 0xffff) | (u32::from(value) << 16),
                self.a_known | 0xffff0000,
            ),
            4 => self.set_st(value)?,
            5 => self.push(value)?,
            6 => self.pc = value,
            7 => return Err(Fault::RegisterWrite(index)),
            8..=14 => ports.write(index - 8, self.st, blind, value)?,
            15 if blind => ports.write(7, self.st, true, value)?,
            15 => self.assign_a(
                (self.a & 0xffff0000) | u32::from(value),
                self.a_known | 0x0000ffff,
            ),
            _ => return Err(Fault::RegisterWrite(index)),
        }
        Ok(())
    }

    pub(super) fn inputs<P: Ports>(ports: &mut P) -> Result<u16, Fault> {
        let inputs = ports.status_inputs()?;
        if inputs & !INPUTS != 0 {
            return Err(Fault::StatusInputs);
        }
        Ok(inputs)
    }
}
