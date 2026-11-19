use super::{
    controller::Controller,
    model::{InstructionModel, Phase},
    ssp1601::{Core, Step},
};
use zeff_emu_common::save_state::StateReader;
const MAX_BYTES: usize = 512 * 1024;
impl InstructionModel {
    pub(super) fn decode_historical_candidate(&self, bytes: &[u8]) -> anyhow::Result<Self> {
        anyhow::ensure!(bytes.len() <= MAX_BYTES, "component state bound");
        let mut r = StateReader::new(bytes);
        let mut magic = [0; 8];
        r.read_exact(&mut magic)?;
        anyhow::ensure!(&magic == b"ZSINST02", "component state tag");
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
        let history = Self::read_rom_history(&mut r)?;
        anyhow::ensure!(r.is_exhausted(), "trailing component state");
        let mut candidate = self.clone();
        candidate.core = core;
        candidate.ports = ports;
        candidate.phase = phase;
        candidate.instructions = instructions;
        candidate.boundary_control = boundary_control;
        candidate.services = services;
        candidate.clock = clock;
        candidate.rom_history = history;
        candidate.validate_rom_history()?;
        candidate.validate_functional_clock()?;
        candidate.validate_historical_services()?;
        Ok(candidate)
    }
}
