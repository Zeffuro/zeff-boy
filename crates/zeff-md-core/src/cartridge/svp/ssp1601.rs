mod alu;
mod decode;
mod known;
mod pointers;
mod state;
mod transfer;

#[cfg(test)]
mod tests;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ProgramRole {
    Opcode,
    InstructionWord,
    ProgramOperand,
}

pub(crate) const FLAG_N: u16 = 0x8000;
pub(crate) const FLAG_OV: u16 = 0x4000;
pub(crate) const FLAG_Z: u16 = 0x2000;
pub(crate) const FLAG_L: u16 = 0x1000;
const FLAGS: u16 = 0xf000;
#[cfg(test)]
const NZ: u16 = FLAG_N | FLAG_Z;
const INPUTS: u16 = 0x0c00;
const ADMITTED_ST: u16 = FLAGS | 0x0067;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Fault {
    ServicePending,
    Opcode(u16),
    StatusMode(u16),
    UnknownFlags(u16),
    UnknownAccumulator(u32),
    StatusInputs,
    RegisterRead(u8),
    RegisterWrite(u8),
    PointerMode { index: u8, modifier: u8 },
    StackUnderflow,
    StackOverflow,
    ProgramRead(u16),
    Port { index: u8, reason: &'static str },
    InvalidState(&'static str),
}

impl std::fmt::Display for Fault {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "SSP1601 semantic fault: {self:?}")
    }
}

impl std::error::Error for Fault {}

// Clones must own their mutations; callbacks must not mutate shared host state.
pub(crate) trait Ports: Clone {
    fn read(&mut self, index: u8, st: u16, blind: bool) -> Result<u16, Fault>;
    fn write(&mut self, index: u8, st: u16, blind: bool, value: u16) -> Result<(), Fault>;

    fn status_inputs(&mut self) -> Result<u16, Fault> {
        Err(Fault::StatusInputs)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Step {
    pub pc: u16,
    pub opcode: u16,
    pub instruction_words: u8,
    pub program_reads: u8,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Core {
    pub x: u16,
    pub y: u16,
    pub a: u32,
    pub a_known: u32,
    pub p: u32,
    pub pc: u16,
    pub st: u16,
    pub flags_known: u16,
    pub stack: [u16; 6],
    pub stack_len: u8,
    pub pointers: [u8; 8],
    pub ram: [[u16; 256]; 2],
}

impl Core {
    pub(crate) fn synthetic(entry: u16) -> Self {
        Self {
            x: 0,
            y: 0,
            a: 0,
            a_known: u32::MAX,
            p: 0,
            pc: entry,
            st: 0,
            flags_known: FLAGS,
            stack: [0; 6],
            stack_len: 0,
            pointers: [0; 8],
            ram: [[0; 256]; 2],
        }
    }

    pub(crate) fn step<P: Ports>(
        &mut self,
        program: impl Fn(u16) -> Result<u16, Fault>,
        ports: &mut P,
    ) -> Result<Step, Fault> {
        let (candidate, candidate_ports, step) = self.prepare(program, ports)?;
        *self = candidate;
        *ports = candidate_ports;
        Ok(step)
    }

    pub(super) fn prepare<P: Ports>(
        &self,
        program: impl Fn(u16) -> Result<u16, Fault>,
        ports: &P,
    ) -> Result<(Self, P, Step), Fault> {
        self.prepare_serviced(|address, _| program(address), ports)
    }

    pub(super) fn prepare_serviced<P: Ports>(
        &self,
        program: impl Fn(u16, ProgramRole) -> Result<u16, Fault>,
        ports: &P,
    ) -> Result<(Self, P, Step), Fault> {
        self.validate()?;
        let mut candidate = self.clone();
        let mut candidate_ports = ports.clone();
        let pc = candidate.pc;
        let opcode = program(pc, ProgramRole::Opcode)?;
        let instruction = decode::decode(opcode)?;
        candidate.pc = pc.wrapping_add(1);
        let mut step = Step {
            pc,
            opcode,
            instruction_words: 1,
            program_reads: 1,
        };
        candidate.execute(instruction, &program, &mut candidate_ports, &mut step)?;
        candidate.validate()?;
        Ok((candidate, candidate_ports, step))
    }

    fn fetch(
        &mut self,
        program: &impl Fn(u16, ProgramRole) -> Result<u16, Fault>,
        step: &mut Step,
    ) -> Result<u16, Fault> {
        let word = program(self.pc, ProgramRole::InstructionWord)?;
        self.pc = self.pc.wrapping_add(1);
        step.instruction_words += 1;
        step.program_reads += 1;
        Ok(word)
    }

    fn program_data(
        address: u16,
        program: &impl Fn(u16, ProgramRole) -> Result<u16, Fault>,
        step: &mut Step,
    ) -> Result<u16, Fault> {
        let value = program(address, ProgramRole::ProgramOperand)?;
        step.program_reads += 1;
        Ok(value)
    }

    fn product(x: u16, y: u16) -> u32 {
        (i32::from(x as i16) * i32::from(y as i16)).wrapping_mul(2) as u32
    }

    fn push(&mut self, value: u16) -> Result<(), Fault> {
        if self.stack_len == 6 {
            return Err(Fault::StackOverflow);
        }
        self.stack[usize::from(self.stack_len)] = value;
        self.stack_len += 1;
        Ok(())
    }

    fn pop(&mut self) -> Result<u16, Fault> {
        if self.stack_len == 0 {
            return Err(Fault::StackUnderflow);
        }
        self.stack_len -= 1;
        Ok(self.stack[usize::from(self.stack_len)])
    }

    fn set_st(&mut self, value: u16) -> Result<(), Fault> {
        if value & !ADMITTED_ST != 0 {
            return Err(Fault::StatusMode(value));
        }
        self.st = value;
        self.flags_known = FLAGS;
        Ok(())
    }

    fn condition(&self, code: u8, expected: bool) -> Result<bool, Fault> {
        let mask = match code {
            0 => return Ok(true),
            4 => FLAG_L,
            5 => FLAG_Z,
            6 => FLAG_OV,
            7 => FLAG_N,
            _ => return Err(Fault::InvalidState("condition decoder")),
        };
        if self.flags_known & mask == 0 {
            return Err(Fault::UnknownFlags(mask));
        }
        Ok((self.st & mask != 0) == expected)
    }
}
