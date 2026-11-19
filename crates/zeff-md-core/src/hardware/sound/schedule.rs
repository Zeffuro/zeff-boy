mod rollback_memory;
use super::*;
use crate::constants::SOUND_CPU_DIVISOR;
use crate::hardware::bus::ownership::{Requester, Reservation};
use crate::hardware::vdp::SoundInterrupt;
use zeff_z80::{BusState, Cpu, FetchedInstruction, PrefixState};
mod advance;
#[cfg(test)]
mod banked_reads;
#[cfg(test)]
mod banked_writes;
mod download;
mod epoch;
#[cfg(test)]
mod irq_tests;
#[cfg(test)]
mod mixed_reservation_tests;
#[cfg(feature = "sound-shadow-observer")]
mod observer_traits;
#[cfg(test)]
mod or_bank_tests;
mod prepare;
#[cfg(test)]
mod register_tests;
mod replay;
mod rmw;
#[cfg(test)]
mod sf001_fetch_tests;
#[cfg(test)]
mod sf001_tests;
#[cfg(test)]
mod sf002_tests;
#[cfg(test)]
mod sf004_tests;
#[cfg(test)]
mod split_serial_reads;
#[cfg(test)]
mod ssf2_tests;
mod state;
mod svp;
mod svp_history;

const CARTRIDGE_READ_CYCLES: u32 = 3;

#[cfg_attr(
    not(feature = "sound-shadow-observer"),
    derive(Clone, Debug, PartialEq, Eq)
)]
pub(crate) struct PendingSound {
    pub cpu: Cpu,
    pub memory: SoundMemory,
    pub instruction: FetchedInstruction,
    pub deadline: u64,
    pub state: BusState,
    pub writes: WorkWrites,
    pub register_writes: RegisterWrites,
    pub sf001_writes: Sf001Writes,
    pub sf002_writes: Sf002Writes,
    pub psg_writes: PsgWrites,
    pub opn2_writes: Opn2Writes,
    pub bank_access: Option<BankAccess>,
    observed_bank_access: Option<BankAccess>,
    svp_history: Option<Box<svp_history::SoundHistory>>,
    pub any_bank_access: bool,
    pub start: u64,
    replay: Option<ReadReplay>,
    pub trace_origin: Option<zeff_emu_common::audio_trace::MdTraceOrigin>,
    #[cfg(feature = "sound-shadow-observer")]
    observer: observer::Token,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Preparation {
    pub(crate) now: u64,
    pub(crate) state: BusState,
    pub(crate) bounded: bool,
    pub(crate) opn2_status: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ReadReplay {
    preparation: Preparation,
    bank: u16,
    irq_asserted: bool,
    cycles: u32,
    accesses: u32,
    bank_identity: Option<(u32, Direction)>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RelativeReplay {
    Jr,
    Djnz,
}

pub(crate) struct PreparationPolicy {
    pub(crate) irq_asserted: bool,
    pub(crate) legacy_relative: Option<RelativeReplay>,
}

impl PendingSound {
    #[cfg(test)]
    pub(crate) fn has_read_replay(&self) -> bool {
        self.replay.is_some()
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
            self.exclusive_waits(occupied)
        }
        .ok_or(SoundFault::ClockOverflow)?;
        self.apply_reservation_waits(waits)
    }

    fn apply_reservation_waits(&mut self, waits: u64) -> Result<(), SoundFault> {
        let next_waits = self
            .svp_history
            .as_ref()
            .map(|history| {
                history
                    .reservation_waits
                    .checked_add(waits)
                    .ok_or(SoundFault::ClockOverflow)
            })
            .transpose()?;
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
        if let Some(waits) = next_waits {
            self.svp_history.as_mut().unwrap().reservation_waits = waits;
        }
        Ok(())
    }

    fn exclusive_waits(&self, occupied: &[Reservation]) -> Option<u64> {
        let mut reservation = Reservation {
            requester: Requester::SOUND,
            ticks: self.start..self.deadline,
        };
        loop {
            let before = reservation.ticks.start;
            for slot in occupied
                .iter()
                .filter(|slot| matches!(slot.requester, Requester::VDP | Requester::MAIN_RMW))
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
        cartridge: &mut Cartridge,
    ) {
        assert!(
            self.replay.is_none(),
            "timed reads refreshed before publication"
        );
        audio.request_advance_to(self.deadline);
        #[cfg(feature = "sound-shadow-observer")]
        observer::publication(&self.observer);
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
        self.register_writes.publish(cartridge);
        self.sf001_writes.publish(cartridge);
        self.sf002_writes.publish(cartridge);
        if let Some(board) = &mut cartridge.download
            && let Some(access) = self.bank_access
            && access.direction == Direction::Write
            && crate::cartridge::download::memory(access.address)
        {
            assert!(
                board.write(access.address, access.value),
                "qualified Download store"
            );
        }
        memory.complete_boundary();
    }
}

#[derive(Clone, Default, PartialEq, Eq)]
pub(crate) struct SoundSchedule {
    pub pending: Option<PendingSound>,
    pub(crate) rmw_checkpoint: Option<Box<PendingSound>>,
    pub fault: Option<SoundFault>,
    pub state: BusState,
    pub epoch: Option<epoch::LocalEpoch>,
}

pub(crate) struct SoundInterval<'a> {
    pub ticks: std::ops::Range<u64>,
    pub occupied: &'a [Reservation],
    pub audio: &'a mut Audio,
    pub sound_irq: Option<&'a SoundInterrupt>,
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
    pub(crate) fn rollback_boundary_ready(&self) -> bool {
        let Self {
            pending,
            rmw_checkpoint,
            fault,
            state: _,
            epoch,
        } = self;
        rmw_checkpoint.is_none()
            && fault.is_none()
            && epoch.is_none()
            && pending.as_ref().is_none_or(|pending| {
                let PendingSound {
                    cpu: _,
                    memory: _,
                    instruction: _,
                    deadline: _,
                    state: _,
                    writes: _,
                    register_writes: _,
                    sf001_writes: _,
                    sf002_writes: _,
                    psg_writes: _,
                    opn2_writes: _,
                    bank_access: _,
                    observed_bank_access: _,
                    svp_history,
                    any_bank_access: _,
                    start: _,
                    replay: _,
                    trace_origin,
                    #[cfg(feature = "sound-shadow-observer")]
                        observer: _,
                } = pending;
                trace_origin.is_none() && svp_history.is_none()
            })
    }

    pub(crate) fn local_pending_after(&self, end: u64) -> bool {
        if self.fault.is_some() {
            return false;
        }
        match (self.epoch_deadline(), self.pending.as_ref()) {
            (Some(deadline), None) => deadline > end,
            (None, Some(pending)) => {
                pending.deadline > end && pending.bank_access.is_none() && !pending.any_bank_access
            }
            _ => false,
        }
    }

    #[cfg(test)]
    pub fn advance(
        &mut self,
        cpu: &mut Cpu,
        memory: &mut SoundMemory,
        cartridge: &mut Cartridge,
        ram: &mut [u8],
        interval: SoundInterval<'_>,
    ) {
        self.advance_observed(cpu, memory, cartridge, ram, interval);
    }

    #[cfg(test)]
    pub(crate) fn advance_observed(
        &mut self,
        cpu: &mut Cpu,
        memory: &mut SoundMemory,
        cartridge: &mut Cartridge,
        ram: &mut [u8],
        interval: SoundInterval<'_>,
    ) -> bool {
        self.advance_owned(
            cpu,
            memory,
            cartridge,
            ram,
            (interval, &mut crate::sound_trace::NoSoundCapture),
        )
    }

    pub(crate) fn reset_cpu<C: crate::sound_trace::SoundCapture>(
        &mut self,
        cpu: &mut Cpu,
        memory: &mut SoundMemory,
        capture: &mut C,
    ) {
        capture.reset();
        {
            self.epoch = None;
        }
        self.pending = None;
        self.rmw_checkpoint = None;
        self.fault = None;
        self.state = BusState::default();
        cpu.reset();
        memory.complete_boundary();
    }
}
