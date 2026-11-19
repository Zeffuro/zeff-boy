use super::{
    controller::Controller,
    model::{Error, InstructionModel, Phase},
    ssp1601::{Core, Step},
};
use zeff_emu_common::save_state::{StateReader, StateWriter};

const MAGIC: &[u8; 8] = b"ZSINST01";
const MAX_BYTES: usize = 512 * 1024;

impl InstructionModel {
    pub fn encode_state(&self) -> Result<Vec<u8>, Error> {
        let mut w = StateWriter::new();
        w.write_bytes(if self.has_rom_history() {
            b"ZSINST02"
        } else {
            MAGIC
        });
        w.write_bytes(&self.identity);
        w.write_u64(self.instructions);
        self.core.write_state(&mut w)?;
        self.ports.write_state(&mut w);
        match &self.phase {
            Phase::Reset => w.write_u8(0),
            Phase::Fetch => w.write_u8(1),
            Phase::Execute { pc, opcode } => {
                w.write_u8(2);
                w.write_u16(*pc);
                w.write_u16(*opcode);
            }
            Phase::Commit { core, ports, step } => {
                w.write_u8(3);
                core.write_state(&mut w)?;
                ports.write_state(&mut w);
                w.write_u16(step.pc);
                w.write_u16(step.opcode);
                w.write_u8(step.instruction_words);
                w.write_u8(step.program_reads);
            }
        }
        if let Some(halted) = self.boundary_control {
            w.write_bytes(b"ZSVPCTL1");
            w.write_bool(halted);
        }
        self.write_services(&mut w);
        self.write_functional_clock(&mut w)?;
        if self.has_rom_history() {
            self.write_rom_history(&mut w);
        }
        let bytes = w.into_bytes();
        if bytes.len() > MAX_BYTES {
            return Err(Error::State("component state bound".into()));
        }
        Ok(bytes)
    }

    pub fn decode_state(&mut self, bytes: &[u8]) -> Result<(), Error> {
        let candidate = match bytes.get(..8) {
            Some(b"ZSINST01") => self.decode_candidate(bytes),
            Some(b"ZSINST02") => self.decode_historical_candidate(bytes),
            _ => Err(anyhow::anyhow!("component state tag")),
        };
        candidate
            .map(|candidate| *self = candidate)
            .map_err(|error| Error::State(format!("{error:#}")))
    }

    fn decode_candidate(&self, bytes: &[u8]) -> anyhow::Result<Self> {
        anyhow::ensure!(bytes.len() <= MAX_BYTES, "component state bound");
        let mut r = StateReader::new(bytes);
        let mut magic = [0; 8];
        r.read_exact(&mut magic)?;
        anyhow::ensure!(&magic == MAGIC, "component state tag");
        let mut identity = [0; 32];
        r.read_exact(&mut identity)?;
        anyhow::ensure!(identity == self.identity, "program/internal ROM identity");
        let instructions = r.read_u64()?;
        let core = Core::read_state(&mut r)?;
        let ports = Controller::read_state(&mut r, self.rom.clone())?;
        let phase = match r.read_u8()? {
            0 => {
                anyhow::ensure!(
                    instructions == 0
                        && core == Core::synthetic(0)
                        && ports == Controller::with_rom(self.rom.clone()),
                    "noncanonical reset context"
                );
                Phase::Reset
            }
            1 => Phase::Fetch,
            2 => {
                let pc = r.read_u16()?;
                let opcode = r.read_u16()?;
                anyhow::ensure!(pc == core.pc, "pending fetch PC");
                Phase::Execute { pc, opcode }
            }
            3 => {
                let next = Core::read_state(&mut r)?;
                let pending = Controller::read_state(&mut r, self.rom.clone())?;
                let step = Step {
                    pc: r.read_u16()?,
                    opcode: r.read_u16()?,
                    instruction_words: r.read_u8()?,
                    program_reads: r.read_u8()?,
                };
                anyhow::ensure!(
                    step.pc == core.pc
                        && (1..=2).contains(&step.instruction_words)
                        && (step.instruction_words..=3).contains(&step.program_reads),
                    "pending instruction metadata"
                );
                Phase::Commit {
                    core: Box::new(next),
                    ports: pending,
                    step,
                }
            }
            _ => anyhow::bail!("component phase"),
        };
        let boundary_control = if self.boundary_control.is_some() {
            let mut tag = [0; 8];
            r.read_exact(&mut tag)?;
            anyhow::ensure!(&tag == b"ZSVPCTL1", "missing boundary-control owner");
            let halted = r.read_bool()?;
            anyhow::ensure!(
                !halted || matches!(phase, Phase::Fetch),
                "halted pending DSP transaction"
            );
            Some(halted)
        } else {
            None
        };
        let services = self.read_services(&mut r)?;
        let clock = self.read_functional_clock(&mut r)?;
        anyhow::ensure!(r.is_exhausted(), "trailing component state");
        let mut candidate = self.clone();
        candidate.core = core;
        candidate.ports = ports;
        candidate.phase = phase;
        candidate.instructions = instructions;
        candidate.boundary_control = boundary_control;
        candidate.services = services;
        candidate.clock = clock;
        candidate.rom_history.clear();
        candidate.validate_functional_clock()?;
        candidate.validate_services()?;
        match &candidate.phase {
            Phase::Execute { pc, opcode } => anyhow::ensure!(
                candidate.program(*pc)? == *opcode,
                "captured opcode differs"
            ),
            Phase::Commit {
                core: pending_core,
                ports: pending_ports,
                step,
            } => {
                let mut expected_core = candidate.core.clone();
                let mut expected_ports = candidate.ports.clone();
                let expected_step = expected_core
                    .step(|address| candidate.program(address), &mut expected_ports)?;
                anyhow::ensure!(
                    expected_step == *step
                        && expected_core == **pending_core
                        && expected_ports == *pending_ports,
                    "pending semantic transaction differs"
                );
            }
            _ => {}
        }
        Ok(candidate)
    }
}
