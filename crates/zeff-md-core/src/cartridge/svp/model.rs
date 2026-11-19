use super::{
    controller::Controller,
    ssp1601::{Core, Fault, Step},
};
use sha2::{Digest, Sha256};
use std::sync::Arc;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    Image(&'static str),
    Busy,
    Cpu(String),
    State(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "SVP instruction model: {self:?}")
    }
}
impl std::error::Error for Error {}
impl From<Fault> for Error {
    fn from(value: Fault) -> Self {
        Self::Cpu(value.to_string())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Progress {
    Waiting,
    ResetVector {
        entry: u16,
    },
    Fetched {
        pc: u16,
        opcode: u16,
    },
    Prepared {
        pc: u16,
        opcode: u16,
    },
    Committed {
        pc: u16,
        opcode: u16,
        instruction_words: u8,
        program_reads: u8,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Registers {
    pub x: u16,
    pub y: u16,
    pub accumulator: u32,
    pub accumulator_known: u32,
    pub product: u32,
    pub pc: u16,
    pub status: u16,
    pub flags_known: u16,
    pub committed_instructions: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[allow(
    clippy::large_enum_variant,
    reason = "Keep the controller inline to avoid another allocation per instruction."
)]
pub(super) enum Phase {
    Reset,
    Fetch,
    Execute {
        pc: u16,
        opcode: u16,
    },
    Commit {
        core: Box<Core>,
        ports: Controller,
        step: Step,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InstructionModel {
    pub(super) rom: Arc<[u16]>,
    pub(super) irom: Arc<[u16]>,
    pub(super) identity: [u8; 32],
    pub(super) core: Core,
    pub(super) ports: Controller,
    pub(super) phase: Phase,
    pub(super) instructions: u64,
    pub(super) boundary_control: Option<bool>,
    pub(super) clock: Option<super::clock::FunctionalClock>,
    pub(super) services: Option<Vec<super::service::Capture>>,
    pub(super) rom_history: Vec<super::history::WordInput>,
}

impl InstructionModel {
    // Explicit software context; grants are semantic phases, not oscillator clocks.
    pub fn new_synthetic(rom: Vec<u16>, irom: Vec<u16>) -> Result<Self, Error> {
        if rom.is_empty() || rom.len() > 1024 * 1024 || irom.len() != 1024 {
            return Err(Error::Image("program or internal ROM word count"));
        }
        let mut digest = Sha256::new();
        digest.update((rom.len() as u32).to_le_bytes());
        for &word in rom.iter().chain(&irom) {
            digest.update(word.to_be_bytes());
        }
        let rom: Arc<[u16]> = rom.into();
        let ports = Controller::with_rom(rom.clone());
        Ok(Self {
            rom,
            irom: irom.into(),
            identity: digest.finalize().into(),
            core: Core::synthetic(0),
            ports,
            phase: Phase::Reset,
            instructions: 0,
            boundary_control: None,
            services: None,
            rom_history: Vec::new(),
            clock: None,
        })
    }

    pub(super) fn program(&self, address: u16) -> Result<u16, Fault> {
        match address {
            0..=0x03ff => Ok(self.ports.iram[usize::from(address)]),
            0xfc00..=0xffff => Ok(self.irom[usize::from(address - 0xfc00)]),
            _ => self
                .rom
                .get(usize::from(address))
                .copied()
                .ok_or(Fault::ProgramRead(address)),
        }
    }

    pub fn advance(&mut self, granted: bool) -> Result<Progress, Error> {
        if self.services.is_some() {
            return Err(Error::Image("use the serviced advance interface"));
        }
        self.advance_phase(granted)
    }

    pub(super) fn advance_phase(&mut self, granted: bool) -> Result<Progress, Error> {
        if !granted || self.boundary_control == Some(true) {
            return Ok(Progress::Waiting);
        }
        match &self.phase {
            Phase::Reset => {
                let entry = self.program(0xfffc)?;
                self.core.pc = entry;
                self.phase = Phase::Fetch;
                Ok(Progress::ResetVector { entry })
            }
            Phase::Fetch => {
                let pc = self.core.pc;
                let opcode = self.program(pc)?;
                self.phase = Phase::Execute { pc, opcode };
                Ok(Progress::Fetched { pc, opcode })
            }
            Phase::Execute { pc, opcode } => {
                let (pc, opcode) = (*pc, *opcode);
                let (core, ports, step) = self.core.prepare(
                    |address| {
                        if address == pc {
                            Ok(opcode)
                        } else {
                            self.program(address)
                        }
                    },
                    &self.ports,
                )?;
                self.phase = Phase::Commit {
                    core: Box::new(core),
                    ports,
                    step,
                };
                Ok(Progress::Prepared { pc, opcode })
            }
            Phase::Commit { step, .. } => {
                let count = self
                    .instructions
                    .checked_add(1)
                    .ok_or(Error::State("instruction count overflow".into()))?;
                let result = Progress::Committed {
                    pc: step.pc,
                    opcode: step.opcode,
                    instruction_words: step.instruction_words,
                    program_reads: step.program_reads,
                };
                let Phase::Commit { core, ports, .. } =
                    std::mem::replace(&mut self.phase, Phase::Fetch)
                else {
                    unreachable!();
                };
                self.core = *core;
                self.ports = ports;
                self.instructions = count;
                self.rom_history.clear();
                if let Some(captures) = &mut self.services {
                    captures.clear();
                }
                Ok(result)
            }
        }
    }

    fn require_boundary(&self) -> Result<(), Error> {
        if matches!(self.phase, Phase::Fetch) {
            Ok(())
        } else {
            Err(Error::Busy)
        }
    }

    pub(crate) fn main_boundary(&self) -> Result<(), Error> {
        self.require_boundary()
    }

    pub(crate) fn enable_boundary_control(&mut self) -> Result<(), Error> {
        if !matches!(self.phase, Phase::Reset) || self.boundary_control.is_some() {
            return Err(Error::Busy);
        }
        self.boundary_control = Some(false);
        Ok(())
    }

    pub(crate) fn boundary_control_enabled(&self) -> bool {
        self.boundary_control.is_some()
    }

    pub(crate) fn preflight_control(&self, value: u16) -> Result<(), Error> {
        self.require_boundary()?;
        self.preflight_control_value(value)
    }

    pub(crate) fn preflight_control_value(&self, value: u16) -> Result<(), Error> {
        if self.boundary_control.is_none() || !matches!(value, 0 | 0x000a) {
            return Err(Error::Image("unqualified boundary control value"));
        }
        Ok(())
    }

    pub(crate) fn main_control(&mut self, value: u16) -> Result<(), Error> {
        self.preflight_control(value)?;
        self.boundary_control = Some(value == 0x000a);
        Ok(())
    }

    pub(crate) fn reset_synthetic(&mut self) {
        self.core = Core::synthetic(0);
        self.ports = Controller::with_rom(self.rom.clone());
        self.phase = Phase::Reset;
        self.instructions = 0;
        self.reset_functional_clock();
        self.rom_history.clear();
        if let Some(captures) = &mut self.services {
            captures.clear();
        }
        self.boundary_control = self.boundary_control.map(|_| false);
    }

    pub(crate) fn main_dram_write(
        &mut self,
        address: u16,
        value: u16,
        mask: u16,
    ) -> Result<(), Error> {
        self.require_boundary()?;
        self.ports.main_dram_write(address, value, mask);
        Ok(())
    }

    pub fn publish_main_command(&mut self, value: u16) -> Result<(), Error> {
        self.require_boundary()?;
        self.ports.publish_main(value);
        Ok(())
    }

    pub fn consume_main_status(&mut self) -> Result<u16, Error> {
        self.require_boundary()?;
        Ok(self.ports.consume_main_status())
    }

    pub fn peek_status(&self) -> u16 {
        self.ports.status()
    }
    pub fn peek_result(&self) -> u16 {
        self.ports.xst()
    }
    pub fn peek_dram(&self, address: u16) -> u16 {
        self.ports.dram(address)
    }

    pub(crate) fn dram_read_view(&self) -> super::DramReadView {
        self.ports.dram_read_view()
    }

    pub fn peek_iram(&self, address: u16) -> Option<u16> {
        self.ports.iram.get(usize::from(address)).copied()
    }

    pub fn set_synthetic_input_pins(&mut self, value: u16) -> Result<(), Error> {
        self.require_boundary()?;
        if value & !0x0c00 != 0 {
            return Err(Error::Image("external status input mask"));
        }
        self.ports.inputs = value;
        Ok(())
    }

    pub fn registers(&self) -> Registers {
        Registers {
            x: self.core.x,
            y: self.core.y,
            accumulator: self.core.a,
            accumulator_known: self.core.a_known,
            product: self.core.p,
            pc: self.core.pc,
            status: self.core.st,
            flags_known: self.core.flags_known,
            committed_instructions: self.instructions,
        }
    }
}
