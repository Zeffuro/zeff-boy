use super::*;
use crate::hardware::sound::SoundFault;
use zeff_m68k::timing::physical::{EntryBus, EntryOutcome, EntryReport};

pub(crate) mod admission;
pub(crate) mod scope;
#[cfg(test)]
mod sound_tests;
#[cfg(test)]
mod tests;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ResetBusError {
    Bus(BusError),
    Sound(SoundFault),
    Ownership,
    UnsupportedWrite,
    ElapsedBudget { completed: u64, requested: u32 },
    ClockGeometry,
}

impl From<BusError> for ResetBusError {
    fn from(value: BusError) -> Self {
        Self::Bus(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct IdleFailure {
    start: CpuClocks,
    requested: u32,
    completed: u64,
    source: ResetBusError,
}

enum Target<'a, C = NoSoundCapture> {
    Direct(&'a mut Bus),
    Scheduled(ScheduledBus<'a, C>),
}

impl<C: SoundCapture> Target<'_, C> {
    fn bus(&self) -> &Bus {
        match self {
            Self::Direct(bus) => bus,
            Self::Scheduled(target) => target.bus,
        }
    }

    fn bus_mut(&mut self) -> &mut Bus {
        match self {
            Self::Direct(bus) => bus,
            Self::Scheduled(target) => target.bus,
        }
    }

    fn check(&self, start: CpuClocks) -> Result<(), ResetBusError> {
        self.bus().check_main_clock(start)?;
        if self.bus().rmw.is_some() || self.bus().reset_output {
            return Err(ResetBusError::Ownership);
        }
        if let Self::Scheduled(target) = self {
            if target.schedule.rmw_checkpoint.is_some() {
                return Err(ResetBusError::Ownership);
            }
            sound_ok(target)?;
        }
        Ok(())
    }

    fn read(&mut self, access: BusAccess, origin: CpuClocks) -> Result<u16, ResetBusError> {
        self.check(origin)?;
        if self
            .bus()
            .cartridge
            .bounded_switched_guarded(access.bus_address)
            || self
                .bus()
                .cartridge
                .bounded_switched_guarded(access.logical_address & BUS_ADDRESS_MASK)
            || self.bus().cartridge.bounded_odd_guarded(access.bus_address)
            || self
                .bus()
                .cartridge
                .bounded_odd_guarded(access.logical_address & BUS_ADDRESS_MASK)
        {
            return Err(BusError::UnsupportedAccess {
                address: access.bus_address,
                direction: Direction::Read,
            }
            .into());
        }
        match self {
            Self::Direct(bus) => {
                if let Some((clocks, _)) = bus.external_dma_wait()? {
                    budget(origin, bus.cpu_clock, clocks)?;
                    bus.wait_external_dma(&mut 0)?;
                }
                budget(origin, bus.cpu_clock, TRANSFER_CLOCKS)?;
                bus.advance_main(bus.cpu_clock, TRANSFER_CLOCKS)?;
                while let Some(clocks) = bus.vdp_read_wait(access)? {
                    budget(origin, bus.cpu_clock, clocks)?;
                    bus.advance_main(bus.cpu_clock, clocks)?;
                }
                Ok(bus.read(access)?)
            }
            Self::Scheduled(target) => {
                let previous = target.begin_epoch_access(false);
                let result = scheduled_read(target, access, origin);
                target.end_epoch_access(previous);
                result
            }
        }
    }

    fn idle(&mut self, start: CpuClocks, clocks: u32) -> Result<(), ResetBusError> {
        self.check(start)?;
        match self {
            Self::Direct(bus) => bus.advance_main(start, clocks)?,
            Self::Scheduled(target) => {
                let previous = target.begin_epoch_access(false);
                let result = target.advance_main(start, clocks, false);
                target.end_epoch_access(previous);
                result?;
                sound_ok(target)?;
            }
        }
        if clocks != 0 {
            self.bus_mut().prefetch = Prefetch::Empty;
        }
        Ok(())
    }
}

fn sound_ok<C: SoundCapture>(target: &ScheduledBus<'_, C>) -> Result<(), ResetBusError> {
    target
        .schedule
        .fault
        .map_or(Ok(()), |fault| Err(ResetBusError::Sound(fault)))
}

fn budget(origin: CpuClocks, now: CpuClocks, requested: u32) -> Result<(), ResetBusError> {
    let completed = now
        .0
        .checked_sub(origin.0)
        .ok_or(ResetBusError::ClockGeometry)?;
    if completed
        .checked_add(u64::from(requested))
        .is_none_or(|n| n > u64::from(u32::MAX))
    {
        return Err(ResetBusError::ElapsedBudget {
            completed,
            requested,
        });
    }
    Ok(())
}

fn scheduled_read<C: SoundCapture>(
    target: &mut ScheduledBus<'_, C>,
    access: BusAccess,
    origin: CpuClocks,
) -> Result<u16, ResetBusError> {
    if let Some((clocks, _)) = target.bus.external_dma_wait()? {
        budget(origin, target.bus.cpu_clock, clocks)?;
        target.wait_external_dma(&mut 0)?;
        sound_ok(target)?;
    }
    let begin = target.bus.now.get();
    let release = target
        .schedule
        .pending
        .as_ref()
        .and_then(|pending| pending.reservation())
        .filter(|slot| slot.requester != Requester::MAIN)
        .map_or(begin, |slot| slot.ticks.end.max(begin));
    let delay = u32::try_from((release - begin).div_ceil(MAIN_CPU_DIVISOR))
        .map_err(|_| BusError::ClockOverflow)?;
    let clocks = u64::from(delay) + u64::from(TRANSFER_CLOCKS);
    target
        .bus
        .cpu_clock
        .0
        .checked_add(clocks)
        .ok_or(BusError::ClockOverflow)?;
    let end = begin
        .checked_add(clocks * MAIN_CPU_DIVISOR)
        .ok_or(BusError::ClockOverflow)?;
    if delay != 0 {
        budget(origin, target.bus.cpu_clock, delay)?;
        let sound_irq = target.bus.sound_interrupt();
        target
            .bus
            .advance_main_cycle(target.bus.cpu_clock, delay, false)?;
        target.bus.prefetch = Prefetch::Empty;
        target.schedule.advance_owned(
            target.cpu,
            &mut target.bus.sound,
            &mut target.bus.cartridge,
            &mut target.bus.ram,
            (
                SoundInterval {
                    sound_irq: Some(&sound_irq),
                    audio: &mut target.bus.audio,
                    ticks: begin..target.bus.now.get(),
                    occupied: &[Reservation {
                        requester: Requester::MAIN,
                        ticks: release..end,
                    }],
                },
                &mut target.capture,
            ),
        );
        sound_ok(target)?;
    }
    budget(origin, target.bus.cpu_clock, TRANSFER_CLOCKS)?;
    target.advance_main_cycle_observed(
        target.bus.cpu_clock,
        TRANSFER_CLOCKS,
        true,
        false,
        target.bus.closed_sound_read(access) || Bus::reset_observation(access),
    )?;
    sound_ok(target)?;
    if (SOUND_APERTURE_START..=SOUND_APERTURE_END).contains(&access.bus_address)
        && target.bus.sound.requested
        && !target.bus.sound.reset_asserted
        && !target.bus.sound.granted
        && let Some(pending) = &target.schedule.pending
    {
        let remaining = pending.deadline.saturating_sub(target.bus.now.get());
        let extra = u32::try_from(remaining.div_ceil(MAIN_CPU_DIVISOR))
            .map_err(|_| BusError::ClockOverflow)?;
        budget(origin, target.bus.cpu_clock, extra)?;
        target.advance_main(target.bus.cpu_clock, extra, false)?;
        sound_ok(target)?;
    }
    while let Some(clocks) = target.bus.vdp_read_wait(access)? {
        budget(origin, target.bus.cpu_clock, clocks)?;
        target.advance_main(target.bus.cpu_clock, clocks, true)?;
        sound_ok(target)?;
    }
    Ok(target.bus.read(access)?)
}

struct ResetEntry<'a, C = NoSoundCapture> {
    target: Target<'a, C>,
    failure: Option<ResetBusError>,
    idle_failure: Option<IdleFailure>,
}

impl<'a, C: SoundCapture> ResetEntry<'a, C> {
    fn new(target: Target<'a, C>) -> Result<Self, ResetBusError> {
        target.check(target.bus().cpu_clock)?;
        Ok(Self {
            target,
            failure: None,
            idle_failure: None,
        })
    }
}

impl<C: SoundCapture> EntryBus for ResetEntry<'_, C> {
    type Error = ResetBusError;

    fn read_entry(&mut self, access: BusAccess, start: CpuClocks) -> EntryReport<u16, Self::Error> {
        if let Some(error) = &self.failure {
            return EntryReport {
                elapsed: 0,
                outcome: EntryOutcome::HostFailure(error.clone()),
            };
        }
        let origin = self.target.bus().cpu_clock;
        let master = self.target.bus().now.get();
        let result = self
            .target
            .check(start)
            .and_then(|()| self.target.read(access, origin));
        let elapsed = self
            .target
            .bus()
            .cpu_clock
            .0
            .checked_sub(origin.0)
            .expect("monotonic MAIN clock");
        let elapsed = u32::try_from(elapsed).expect("pre-advance callback budget");
        let result = result.and_then(|value| {
            if self.target.bus().now.get().checked_sub(master)
                != Some(u64::from(elapsed) * MAIN_CPU_DIVISOR)
            {
                Err(ResetBusError::ClockGeometry)
            } else {
                Ok(value)
            }
        });
        let outcome = match result {
            Ok(value) => EntryOutcome::Complete(value),
            Err(error) => {
                self.failure = Some(error.clone());
                EntryOutcome::HostFailure(error)
            }
        };
        EntryReport { elapsed, outcome }
    }

    fn write_entry(&mut self, _: BusAccess, _: u16, _: CpuClocks) -> EntryReport<(), Self::Error> {
        let error = self
            .failure
            .get_or_insert(ResetBusError::UnsupportedWrite)
            .clone();
        EntryReport {
            elapsed: 0,
            outcome: EntryOutcome::HostFailure(error),
        }
    }

    fn idle_entry(&mut self, start: CpuClocks, clocks: u32) {
        if self.failure.is_some() {
            return;
        }
        let origin = self.target.bus().cpu_clock;
        if let Err(source) = self.target.idle(start, clocks) {
            let completed = self
                .target
                .bus()
                .cpu_clock
                .0
                .checked_sub(origin.0)
                .expect("monotonic MAIN clock");
            self.idle_failure = Some(IdleFailure {
                start,
                requested: clocks,
                completed,
                source: source.clone(),
            });
            self.failure = Some(source);
            self.target.bus_mut().clock_fault = true;
        }
    }

    fn detect_address_error(
        &mut self,
        _: BusAccess,
        _: Direction,
        _: CpuClocks,
    ) -> EntryReport<(), Self::Error> {
        EntryReport {
            elapsed: 0,
            outcome: self
                .failure
                .as_ref()
                .map_or(EntryOutcome::TimingUnavailable, |e| {
                    EntryOutcome::HostFailure(e.clone())
                }),
        }
    }
}
