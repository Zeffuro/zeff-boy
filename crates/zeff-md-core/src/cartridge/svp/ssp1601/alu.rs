use super::decode::{Alu, Instruction, Multiply, Source};
use super::{Core, Fault, Ports, ProgramRole, Step};

impl Core {
    pub(super) fn execute<P: Ports>(
        &mut self,
        instruction: Instruction,
        program: &impl Fn(u16, ProgramRole) -> Result<u16, Fault>,
        ports: &mut P,
        step: &mut Step,
    ) -> Result<(), Fault> {
        match instruction {
            Instruction::Transfer { to, from } => self.transfer(to, from, program, ports, step),
            Instruction::Alu { operation, from } => {
                if matches!(from, Source::Register(3))
                    && matches!(operation, Alu::And | Alu::Or | Alu::Xor)
                {
                    if matches!(operation, Alu::Xor) {
                        self.assign_a(0, u32::MAX);
                    }
                    self.result_flags(self.a, self.a_known);
                    return Ok(());
                }
                if matches!(operation, Alu::Sub | Alu::Compare | Alu::Add) {
                    self.require_a(u32::MAX)?;
                }
                let operand = match from {
                    Source::Register(3) => self.a,
                    Source::Register(7) => self.p,
                    _ => u32::from(self.source(from, false, program, ports, step)?) << 16,
                };
                let result = match operation {
                    Alu::Sub | Alu::Compare => self.a.wrapping_sub(operand),
                    Alu::Add => self.a.wrapping_add(operand),
                    Alu::And => self.a & operand,
                    Alu::Or => self.a | operand,
                    Alu::Xor => self.a ^ operand,
                };
                let known = match operation {
                    Alu::And => self.a_known | !operand,
                    Alu::Or => self.a_known | operand,
                    Alu::Xor => self.a_known,
                    _ => u32::MAX,
                };
                if !matches!(operation, Alu::Compare) {
                    self.assign_a(result, known);
                }
                self.result_flags(result & known, known);
                Ok(())
            }
            Instruction::Branch {
                call,
                condition,
                expected,
            } => {
                let target = self.fetch(program, step)?;
                if self.test_condition(condition, expected, ports)? {
                    if call {
                        self.push(self.pc)?;
                    }
                    self.pc = target;
                }
                Ok(())
            }
            Instruction::Modify {
                operation,
                condition,
                expected,
            } => {
                if self.test_condition(condition, expected, ports)? {
                    self.require_a(u32::MAX)?;
                    self.a = match operation {
                        2 => ((self.a as i32) >> 1) as u32,
                        3 => self.a.wrapping_shl(1),
                        6 => self.a.wrapping_neg(),
                        7 if self.a & 0x80000000 != 0 => self.a.wrapping_neg(),
                        7 => self.a,
                        _ => return Err(Fault::Opcode(step.opcode)),
                    };
                    self.result_flags(self.a, self.a_known);
                }
                Ok(())
            }
            Instruction::Multiply { operation, x, y } => {
                if !matches!(operation, Multiply::Load) {
                    self.require_a(u32::MAX)?;
                }
                let result = match operation {
                    Multiply::Load => 0,
                    Multiply::Add => self.a.wrapping_add(self.p),
                    Multiply::Sub => self.a.wrapping_sub(self.p),
                };
                let (x_bank, x_address) = self.pointer_address(x, false)?;
                let (y_bank, y_address) = self.pointer_address(y, false)?;
                self.x = self.ram[x_bank][x_address];
                self.y = self.ram[y_bank][y_address];
                self.p = Self::product(self.x, self.y);
                self.assign_a(result, u32::MAX);
                self.result_flags(result, u32::MAX);
                Ok(())
            }
        }
    }

    fn test_condition<P: Ports>(
        &self,
        code: u8,
        expected: bool,
        ports: &mut P,
    ) -> Result<bool, Fault> {
        if code == 2 || code == 3 {
            let mask = if code == 2 { 0x0400 } else { 0x0800 };
            return Ok((Self::inputs(ports)? & mask != 0) == expected);
        }
        self.condition(code, expected)
    }
}
