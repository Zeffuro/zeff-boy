use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Operand {
    opcode: u8,
    action: Action,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Action {
    ImmediateAlu(u8),
    ReadRegister(u8, u16),
    ReadAlu(u8, u16),
    Write(u16, u8),
}

impl Operand {
    pub(super) fn decode(cpu: &Cpu, opcode: u8) -> Option<Self> {
        let action = match opcode {
            0x02 => Action::Write(cpu.regs.bc(), cpu.regs.a),
            0x12 => Action::Write(cpu.regs.de(), cpu.regs.a),
            0x0a => Action::ReadRegister(REGISTER_A_INDEX, cpu.regs.bc()),
            0x1a => Action::ReadRegister(REGISTER_A_INDEX, cpu.regs.de()),
            0x46 | 0x4e | 0x56 | 0x5e | 0x66 | 0x6e | 0x7e => {
                Action::ReadRegister((opcode >> 3) & 7, cpu.regs.hl())
            }
            0x70..=0x75 | 0x77 => Action::Write(cpu.regs.hl(), cpu.read_register8(opcode & 7)),
            0x86 | 0x8e | 0x96 | 0x9e | 0xa6 | 0xae | 0xb6 | 0xbe => {
                Action::ReadAlu((opcode >> 3) & 7, cpu.regs.hl())
            }
            0xc6 | 0xce | 0xd6 | 0xde | 0xe6 | 0xee | 0xf6 | 0xfe => {
                Action::ImmediateAlu((opcode >> 3) & 7)
            }
            _ => return None,
        };
        Some(Self { opcode, action })
    }

    fn valid(self, opcode: u8) -> bool {
        if self.opcode != opcode {
            return false;
        }
        match (opcode, self.action) {
            (0x02 | 0x12 | 0x70..=0x75 | 0x77, Action::Write(_, _)) => true,
            (0x0a | 0x1a, Action::ReadRegister(REGISTER_A_INDEX, _)) => true,
            (0x46 | 0x4e | 0x56 | 0x5e | 0x66 | 0x6e | 0x7e, Action::ReadRegister(register, _)) => {
                register == ((opcode >> 3) & 7)
            }
            (0x86 | 0x8e | 0x96 | 0x9e | 0xa6 | 0xae | 0xb6 | 0xbe, Action::ReadAlu(group, _))
            | (
                0xc6 | 0xce | 0xd6 | 0xde | 0xe6 | 0xee | 0xf6 | 0xfe,
                Action::ImmediateAlu(group),
            ) => group == ((opcode >> 3) & 7),
            _ => false,
        }
    }

    pub(super) fn physical_cycle(self, cpu: &Cpu, opcode: u8) -> Option<load::LoadCycle> {
        if !self.valid(opcode) {
            return None;
        }
        Some(match self.action {
            Action::ImmediateAlu(_) => load::LoadCycle::Fetch {
                address: cpu.regs.pc,
            },
            Action::ReadRegister(_, address) | Action::ReadAlu(_, address) => {
                load::LoadCycle::Read { address }
            }
            Action::Write(address, value) => load::LoadCycle::Write { address, value },
        })
    }

    pub(super) fn complete<C: retirement_evidence::Capture>(
        self,
        cpu: &mut Cpu,
        sample: load::LoadSample,
        capture: &C,
    ) -> Result<(), ()> {
        match (self.action, sample) {
            (Action::ImmediateAlu(group), load::LoadSample::Read(value)) => {
                cpu.accept_instruction_input(value, InstructionByteClass::Operand, capture);
                cpu.execute_alu_group(group, value);
            }
            (Action::ReadAlu(group, _), load::LoadSample::Read(value)) => {
                cpu.execute_alu_group(group, value);
            }
            (Action::ReadRegister(register, _), load::LoadSample::Read(value)) => match register {
                0 => cpu.regs.b = value,
                1 => cpu.regs.c = value,
                2 => cpu.regs.d = value,
                3 => cpu.regs.e = value,
                4 => cpu.regs.h = value,
                5 => cpu.regs.l = value,
                REGISTER_A_INDEX => cpu.regs.a = value,
                _ => return Err(()),
            },
            (Action::Write(_, _), load::LoadSample::Written) => {}
            _ => return Err(()),
        }
        match (self.opcode, self.action) {
            (0x02 | 0x12, Action::Write(address, value)) => {
                cpu.memptr_accumulator_store(address, value);
            }
            (0x0a | 0x1a, Action::ReadRegister(_, address)) => {
                cpu.set_memptr(address.wrapping_add(1));
            }
            _ => {}
        }
        Ok(())
    }

    pub(super) fn step<B: Z80Bus>(self, cpu: &mut Cpu, bus: &mut B) {
        let sample = match self
            .physical_cycle(cpu, self.opcode)
            .expect("valid operand cycle")
        {
            load::LoadCycle::Fetch { address } => {
                load::LoadSample::Read(bus.cpu_read_instruction(address))
            }
            load::LoadCycle::Read { address } => load::LoadSample::Read(bus.cpu_read(address)),
            load::LoadCycle::Write { address, value } => {
                bus.cpu_write(address, value);
                load::LoadSample::Written
            }
        };
        self.complete(cpu, sample, &retirement_evidence::NoCapture)
            .expect("matching operand sample");
    }
}
