use super::*;
use crate::constants::SOUND_CPU_DIVISOR;
use crate::hardware::bus::ownership::{Requester, Reservation};
use crate::hardware::vdp::SoundInterrupt;
use zeff_z80::{BusState, Cpu, FetchedInstruction, PrefixState};
#[cfg(test)]
mod irq_tests;
mod state;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PendingSound {
    pub cpu: Cpu,
    pub memory: SoundMemory,
    pub instruction: FetchedInstruction,
    pub deadline: u64,
    pub state: BusState,
    pub writes: WorkWrites,
    pub psg_writes: PsgWrites,
    pub opn2_writes: Opn2Writes,
    pub bank_access: Option<BankAccess>,
    pub any_bank_access: bool,
    pub start: u64,
    replay: Option<Opn2Replay>,
    pub trace_origin: Option<zeff_emu_common::audio_trace::MdTraceOrigin>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Preparation {
    now: u64,
    state: BusState,
    bounded: bool,
    opn2_status: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Opn2Replay {
    preparation: Preparation,
    bank: u16,
    irq_asserted: bool,
    cycles: u32,
    accesses: u32,
    bank_identity: Option<(u32, Direction)>,
}

impl PendingSound {
    pub fn prepare(
        cpu: &Cpu,
        memory: &SoundMemory,
        cartridge: &Cartridge,
        now: u64,
    ) -> Result<Self, SoundFault> {
        Self::prepare_with(
            cpu,
            memory,
            cartridge,
            Preparation {
                now,
                state: BusState::default(),
                bounded: false,
                opn2_status: 0,
            },
            memory.irq_asserted,
        )
    }

    fn prepare_with(
        cpu: &Cpu,
        memory: &SoundMemory,
        cartridge: &Cartridge,
        preparation: Preparation,
        irq_asserted: bool,
    ) -> Result<Self, SoundFault> {
        let mut pending = None;
        Self::prepare_into(
            &mut pending,
            cpu,
            memory,
            cartridge,
            preparation,
            irq_asserted,
        )?;
        Ok(pending.unwrap())
    }

    fn prepare_into(
        destination: &mut Option<Self>,
        cpu: &Cpu,
        memory: &SoundMemory,
        cartridge: &Cartridge,
        preparation: Preparation,
        irq_asserted: bool,
    ) -> Result<(), SoundFault> {
        let Preparation {
            now,
            mut state,
            bounded,
            opn2_status,
        } = preparation;
        // Prepare in the final slot to keep the RAM snapshot from moving.
        *destination = Some(Self {
            cpu: cpu.clone(),
            memory: memory.snapshot(),
            instruction: FetchedInstruction {
                pc: 0,
                opcode: 0,
                cycles: 0,
            },
            deadline: 0,
            state,
            writes: WorkWrites::default(),
            psg_writes: PsgWrites::default(),
            opn2_writes: Opn2Writes::default(),
            bank_access: None,
            any_bank_access: false,
            start: now / SOUND_CPU_DIVISOR * SOUND_CPU_DIVISOR,
            replay: None,
            trace_origin: None,
        });
        let pending = destination.as_mut().unwrap();
        let memory_cycle = bounded.then(|| state.next_memory_cycle()).flatten();
        let mut candidate = CandidateBus::new(&mut pending.memory, cartridge);
        candidate.memory.irq_asserted = irq_asserted;
        candidate.opn2_status = opn2_status;
        let instruction = if bounded {
            pending
                .cpu
                .step_bus_segment(&mut candidate, &mut state)
                .expect("owned sound CPU cannot be suspended")
                .instruction
        } else {
            let step = pending
                .cpu
                .step_bounded(&mut candidate, &mut PrefixState::default())
                .expect("owned sound CPU cannot be suspended");
            if !step.complete {
                return Err(SoundFault::UnsupportedOpcode {
                    pc: step.instruction.pc,
                    opcode: step.instruction.opcode,
                });
            }
            step.instruction
        };
        if let Some(fault) = candidate.fault.get() {
            return Err(fault);
        }
        if let Some(trap) = pending.cpu.trap() {
            return Err(SoundFault::CpuTrap(trap));
        }
        let expected = cpu
            .cycles()
            .checked_add(u64::from(instruction.cycles))
            .ok_or(SoundFault::ClockOverflow)?;
        if pending.cpu.cycles() != expected {
            return Err(SoundFault::ClockOverflow);
        }
        let deadline = (now / SOUND_CPU_DIVISOR)
            .checked_add(u64::from(instruction.cycles))
            .and_then(|cycles| cycles.checked_mul(SOUND_CPU_DIVISOR))
            .ok_or(SoundFault::ClockOverflow)?;
        let CandidateBus {
            writes,
            psg_writes,
            opn2_writes,
            bank_access,
            accesses,
            opn2_read,
            ..
        } = candidate;
        let bank_access = bank_access.get();
        let accesses = accesses.get();
        pending.instruction = instruction;
        pending.deadline = deadline;
        pending.state = state;
        pending.writes = writes;
        pending.psg_writes = psg_writes;
        pending.opn2_writes = opn2_writes;
        pending.bank_access = memory_cycle
            .filter(|cycles| *cycles == instruction.cycles && accesses == 1)
            .and(bank_access);
        pending.any_bank_access = bank_access.is_some();
        pending.replay = opn2_read.get().then_some(Opn2Replay {
            preparation,
            bank: memory.bank,
            irq_asserted,
            cycles: instruction.cycles,
            accesses,
            bank_identity: bank_access.map(|access| (access.address, access.direction)),
        });
        pending.extend_bus_segment(0)?;
        Ok(())
    }

    pub fn refresh_opn2(
        &mut self,
        cpu: &Cpu,
        memory: &SoundMemory,
        cartridge: &Cartridge,
        audio: &Audio,
    ) -> Result<(), SoundFault> {
        let Some(replay) = self.replay else {
            return Ok(());
        };
        // CPU and RAM remain unpublished; bank and interrupt inputs were sampled at preparation.
        let mut source = memory.snapshot();
        source.bank = replay.bank;
        source.irq_asserted = replay.irq_asserted;
        let mut refreshed = Self::prepare_with(
            cpu,
            &source,
            cartridge,
            Preparation {
                opn2_status: audio.opn2_status_at(self.deadline),
                ..replay.preparation
            },
            replay.irq_asserted,
        )?;
        let stable = refreshed.replay.is_some_and(|next| {
            next.cycles == replay.cycles
                && next.accesses == replay.accesses
                && next.bank_identity == replay.bank_identity
        }) && refreshed.instruction.pc == self.instruction.pc
            && refreshed.instruction.opcode == self.instruction.opcode
            && refreshed
                .bank_access
                .map(|access| (access.address, access.direction))
                == self
                    .bank_access
                    .map(|access| (access.address, access.direction));
        if !stable {
            return Err(SoundFault::UnstableOpn2Read {
                pc: self.instruction.pc,
            });
        }
        let waits = self
            .instruction
            .cycles
            .checked_sub(replay.cycles)
            .ok_or(SoundFault::ClockOverflow)?;
        refreshed.extend_bus_segment(u64::from(waits))?;
        if refreshed.deadline != self.deadline {
            return Err(SoundFault::ClockOverflow);
        }
        refreshed.start = self.start;
        refreshed.trace_origin = self.trace_origin;
        refreshed.replay = None;
        *self = refreshed;
        Ok(())
    }

    pub fn reservation(&self) -> Option<Reservation> {
        self.bank_access.map(|_| Reservation {
            requester: Requester::SOUND,
            ticks: self.start..self.deadline,
        })
    }

    pub fn reserve_after(&mut self, occupied: &[Reservation]) -> Result<(), SoundFault> {
        if self.bank_access.is_none() && !self.any_bank_access {
            return Ok(());
        }
        let waits = if let Some(reservation) = self.reservation() {
            reservation.wait_after(occupied, SOUND_CPU_DIVISOR)
        } else {
            self.vdp_waits(occupied)
        }
        .ok_or(SoundFault::ClockOverflow)?;
        let start = self
            .start
            .checked_add(
                waits
                    .checked_mul(SOUND_CPU_DIVISOR)
                    .ok_or(SoundFault::ClockOverflow)?,
            )
            .ok_or(SoundFault::ClockOverflow)?;
        self.extend_bus_segment(waits)?;
        self.start = start;
        Ok(())
    }

    fn vdp_waits(&self, occupied: &[Reservation]) -> Option<u64> {
        let mut reservation = Reservation {
            requester: Requester::SOUND,
            ticks: self.start..self.deadline,
        };
        loop {
            let before = reservation.ticks.start;
            for slot in occupied
                .iter()
                .filter(|slot| slot.requester == Requester::VDP)
            {
                let elapsed = reservation
                    .wait_after(std::slice::from_ref(slot), SOUND_CPU_DIVISOR)?
                    .checked_mul(SOUND_CPU_DIVISOR)?;
                reservation.ticks.start = reservation.ticks.start.checked_add(elapsed)?;
                reservation.ticks.end = reservation.ticks.end.checked_add(elapsed)?;
            }
            if reservation.ticks.start == before {
                return Some((before - self.start) / SOUND_CPU_DIVISOR);
            }
        }
    }

    pub fn extend_bus_segment(&mut self, waits: u64) -> Result<(), SoundFault> {
        let waits = u32::try_from(waits).map_err(|_| SoundFault::ClockOverflow)?;
        let deadline = self
            .deadline
            .checked_add(u64::from(waits) * SOUND_CPU_DIVISOR)
            .ok_or(SoundFault::ClockOverflow)?;
        if !self.cpu.extend_bus_segment(&mut self.instruction, waits) {
            return Err(SoundFault::ClockOverflow);
        }
        self.deadline = deadline;
        Ok(())
    }

    pub fn publish(
        &self,
        cpu: &mut Cpu,
        memory: &mut SoundMemory,
        ram: &mut [u8],
        audio: &mut Audio,
    ) {
        assert!(
            self.replay.is_none(),
            "OPN2 reads refreshed before publication"
        );
        audio.advance_to(self.deadline);
        let trace_origin = audio.trace_origin;
        if audio.trace.is_enabled() {
            audio.trace_origin = self.trace_origin;
        }
        self.psg_writes.publish(audio, self.deadline);
        self.opn2_writes.publish(audio, self.deadline);
        audio.trace_origin = trace_origin;
        *cpu = self.cpu.clone();
        memory.ram = self.memory.ram.snapshot();
        memory.bank = self.memory.bank;
        self.writes.publish(ram);
        memory.complete_boundary();
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct SoundSchedule {
    pub pending: Option<PendingSound>,
    pub fault: Option<SoundFault>,
    pub state: BusState,
}

pub(crate) struct SoundInterval<'a> {
    pub ticks: std::ops::Range<u64>,
    pub occupied: &'a [Reservation],
    pub audio: &'a mut Audio,
    pub sound_irq: Option<SoundInterrupt>,
}

impl<'a> SoundInterval<'a> {
    pub fn new(ticks: std::ops::Range<u64>, audio: &'a mut Audio) -> Self {
        Self {
            ticks,
            occupied: &[],
            audio,
            sound_irq: None,
        }
    }
}

impl SoundSchedule {
    pub fn advance(
        &mut self,
        cpu: &mut Cpu,
        memory: &mut SoundMemory,
        cartridge: &Cartridge,
        ram: &mut [u8],
        interval: SoundInterval<'_>,
    ) {
        let SoundInterval {
            ticks,
            occupied,
            audio,
            sound_irq,
        } = interval;
        if self.fault.is_some() {
            audio.advance_to(ticks.end);
            return;
        }
        let mut cursor = ticks.start;
        while cursor < ticks.end {
            if self.pending.is_none() {
                if memory.reset_asserted || memory.requested {
                    memory.complete_boundary();
                    break;
                }
                let irq = match sound_irq.map(|irq| irq.asserted_at(cursor)).transpose() {
                    Ok(irq) => memory.irq_asserted || irq.unwrap_or(false),
                    Err(error) => {
                        self.fault = Some(SoundFault::VdpTiming(error));
                        break;
                    }
                };
                if let Err(fault) = PendingSound::prepare_into(
                    &mut self.pending,
                    cpu,
                    memory,
                    cartridge,
                    Preparation {
                        now: cursor,
                        state: self.state,
                        bounded: true,
                        opn2_status: 0,
                    },
                    irq,
                ) {
                    self.pending = None;
                    self.fault = Some(fault);
                    break;
                }
                if audio.trace.is_enabled() {
                    self.pending.as_mut().unwrap().trace_origin =
                        Some(crate::hardware::audio::trace::sound_origin(
                            cpu.regs().pc,
                            memory.bank,
                            cartridge,
                        ));
                }
            }
            if let Err(fault) = self.pending.as_mut().unwrap().reserve_after(occupied) {
                self.pending = None;
                self.fault = Some(fault);
                break;
            }
            let deadline = self.pending.as_ref().unwrap().deadline;
            if deadline > ticks.end {
                break;
            }
            let completed = self.pending.as_mut().unwrap();
            if let Err(fault) = completed.refresh_opn2(cpu, memory, cartridge, audio) {
                self.pending = None;
                self.fault = Some(fault);
                break;
            }
            self.state = completed.state;
            completed.publish(cpu, memory, ram, audio);
            self.pending = None;
            cursor = deadline;
        }
        audio.advance_to(ticks.end);
    }

    pub fn reset_cpu(&mut self, cpu: &mut Cpu, memory: &mut SoundMemory) {
        self.pending = None;
        self.fault = None;
        self.state = BusState::default();
        cpu.reset();
        memory.complete_boundary();
    }
}
