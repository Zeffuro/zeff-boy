use super::{Emulator, observe};
use crate::hardware::sound::{CandidateBus, SoundFault};
use zeff_m68k::bus::Direction;
use zeff_z80::{BusState, Cpu, CpuState, FetchedInstruction, Registers};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SoundCandidateAccess {
    pub ordinal: u32,
    pub local_address: u16,
    pub direction: Direction,
    pub write_value: Option<u8>,
    pub bank: u16,
    pub translated_main: Option<u32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SoundTerminalObservation {
    pub kind: &'static str,
    pub master_ticks: u64,
    pub vdp_ticks: u64,
    pub command: (u32, u8),
    pub cache_ready: bool,
    pub buffered_word: Option<u16>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SoundPreparationContext {
    pub now: u64,
    pub bounded: bool,
    pub irq_asserted: bool,
    pub opn2_status: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SoundPreparationFault {
    pub kind: &'static str,
    pub boundary: u64,
    pub main_callback_sequence: Option<u64>,
    pub fault: SoundFault,
    pub access: SoundCandidateAccess,
    pub candidate_accesses: u32,
    pub preparation: SoundPreparationContext,
    pub before_cpu_state: CpuState,
    pub before_registers: Registers,
    pub after_registers: Registers,
    pub before_cycles: u64,
    pub after_cycles: u64,
    pub before_state: BusState,
    pub after_state: BusState,
    pub next_memory_cycle: Option<u32>,
    pub returned_segment: FetchedInstruction,
    pub terminal: Option<SoundTerminalObservation>,
}

impl SoundPreparationFault {
    pub(super) fn finish(mut self, machine: &Emulator) -> Self {
        let snapshot = machine.diagnostic_snapshot();
        let vdp = machine.vdp();
        self.terminal = Some(SoundTerminalObservation {
            kind: "CAPTURE_FINISH_OBSERVATION",
            master_ticks: snapshot.context.bus.master_ticks,
            vdp_ticks: snapshot.context.bus.vdp_ticks,
            command: snapshot.context.bus.vdp_command,
            cache_ready: vdp.read_cache().is_ready(),
            buffered_word: vdp.read_cache().buffered_word(),
        });
        self
    }
}

pub(crate) fn preparation_failed(
    before: &Cpu,
    after: &Cpu,
    states: (BusState, BusState),
    instruction: FetchedInstruction,
    candidate: &CandidateBus<'_>,
    preparation: SoundPreparationContext,
) {
    let Some(fault) = candidate.fault.get() else {
        return;
    };
    observe(|recorder| {
        if recorder.first_sound_failure.is_some() {
            return;
        }
        let Some(access) = candidate.diagnostic_access.get() else {
            recorder.complete = false;
            return;
        };
        if fault
            != (SoundFault::UnsupportedAccess {
                address: access.local_address,
                direction: access.direction,
            })
        {
            recorder.complete = false;
            return;
        }
        recorder.first_sound_failure = Some(SoundPreparationFault {
            kind: "SPECULATIVE_PREPARATION",
            boundary: recorder.boundary,
            main_callback_sequence: recorder.in_flight.map(|record| record.sequence),
            fault,
            access,
            candidate_accesses: candidate.accesses.get(),
            preparation,
            before_cpu_state: before.state(),
            before_registers: before.regs(),
            after_registers: after.regs(),
            before_cycles: before.cycles(),
            after_cycles: after.cycles(),
            before_state: states.0,
            after_state: states.1,
            next_memory_cycle: preparation
                .bounded
                .then(|| states.0.next_memory_cycle())
                .flatten(),
            returned_segment: instruction,
            terminal: None,
        });
    });
}
