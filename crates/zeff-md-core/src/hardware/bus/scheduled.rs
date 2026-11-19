use super::ownership::{Requester, Reservation};
use super::sound::Control;
use super::*;
use crate::hardware::sound::schedule::{SoundInterval, SoundSchedule};
use crate::sound_trace::{NoSoundCapture, SoundCapture};
use zeff_z80::Cpu;

mod clocked;
mod compact;
mod read;
pub(crate) mod reset_entry;
mod rmw;
mod svp;
mod write;
pub(crate) use compact::{CompactBus, expand_fault};

#[cfg(test)]
mod reference;

pub(crate) struct ScheduledBus<'a, C = NoSoundCapture> {
    pub capture: C,
    pub bus: &'a mut Bus,
    pub cpu: &'a mut Cpu,
    pub schedule: &'a mut SoundSchedule,
}

#[derive(Default)]
struct TransferPorts {
    vdp: VdpTransfer,
    receipt: Option<ZeroFillWrite>,
}

#[cfg(test)]
impl<'a> ScheduledBus<'a> {
    pub(crate) fn new(bus: &'a mut Bus, cpu: &'a mut Cpu, schedule: &'a mut SoundSchedule) -> Self {
        Self {
            bus,
            cpu,
            schedule,
            capture: NoSoundCapture,
        }
    }
}

impl<C: SoundCapture> ScheduledBus<'_, C> {
    fn begin_epoch_access(&mut self, independent: bool) -> Option<bool> {
        (!independent).then(|| {
            self.schedule
                .block_epoch(self.cpu, &self.bus.sound, &mut self.capture)
        })
    }

    fn end_epoch_access(&mut self, previous: Option<bool>) {
        if let Some(previous) = previous {
            self.schedule.restore_epoch(previous);
        }
    }

    fn wait_external_dma(&mut self, waits: &mut u32) -> Result<(), BusError> {
        if let Some((clocks, release)) = self.bus.external_dma_wait()? {
            let total = waits.checked_add(clocks).ok_or(BusError::ClockOverflow)?;
            let begin = self.bus.now.get();
            let sound_irq = self.bus.sound_interrupt();
            self.bus
                .advance_main_cycle(self.bus.cpu_clock, clocks, false)?;
            self.bus.prefetch = Prefetch::Empty;
            self.schedule.advance_owned(
                self.cpu,
                &mut self.bus.sound,
                &mut self.bus.cartridge,
                &mut self.bus.ram,
                (
                    SoundInterval {
                        sound_irq: Some(&sound_irq),
                        audio: &mut self.bus.audio,
                        ticks: begin..self.bus.now.get(),
                        occupied: &[Reservation {
                            requester: Requester::VDP,
                            ticks: begin..release,
                        }],
                    },
                    &mut self.capture,
                ),
            );
            *waits = total;
        }
        Ok(())
    }
    pub fn advance_machine(&mut self, ticks: u64) -> Result<(), BusError> {
        let epoch_access = self.begin_epoch_access(false);
        let result = (|| {
            let begin = self.bus.now.get();
            let sound_irq = self.bus.sound_interrupt();
            let external = self.bus.external_dma_wait()?.map(|(_, release)| release);
            self.bus.advance_machine_cycle(ticks, false)?;
            if ticks != 0 {
                self.bus.prefetch = Prefetch::Empty;
            }
            self.advance_sound(begin, false, external, &sound_irq);
            Ok(())
        })();
        self.end_epoch_access(epoch_access);
        result
    }

    fn advance_main(
        &mut self,
        start: CpuClocks,
        clocks: u32,
        occupied: bool,
    ) -> Result<(), BusError> {
        self.advance_main_cycle(start, clocks, occupied, false)
    }

    fn advance_main_cycle(
        &mut self,
        start: CpuClocks,
        clocks: u32,
        occupied: bool,
        acknowledge: bool,
    ) -> Result<(), BusError> {
        self.advance_main_cycle_observed(start, clocks, occupied, acknowledge, false)
    }

    fn advance_main_cycle_observed(
        &mut self,
        start: CpuClocks,
        clocks: u32,
        occupied: bool,
        acknowledge: bool,
        retain_prefetch: bool,
    ) -> Result<(), BusError> {
        let begin = self.bus.now.get();
        let external = self.bus.external_dma_wait()?.map(|(_, release)| release);
        let local_pending = external.is_none()
            && !self.bus.audio.trace.is_enabled()
            && begin
                .checked_add(u64::from(clocks) * MAIN_CPU_DIVISOR)
                .is_some_and(|end| begin < end && self.schedule.local_pending_after(end));
        let sound_irq = (!local_pending).then(|| self.bus.sound_interrupt());
        let sound_active = !self.bus.sound.reset_asserted && !self.bus.sound.granted;
        self.bus.advance_main_cycle(start, clocks, acknowledge)?;
        if clocks != 0 && sound_active && !retain_prefetch {
            self.bus.prefetch = Prefetch::Empty;
        }
        let local = if let Some(sound_irq) = sound_irq.as_ref() {
            self.advance_sound(begin, occupied, external, sound_irq)
        } else {
            self.bus.audio.request_advance_to(self.bus.now.get());
            true
        };
        if retain_prefetch && (!local || external.is_some()) {
            self.bus.prefetch = Prefetch::Empty;
        }
        Ok(())
    }

    fn advance_sound(
        &mut self,
        begin: u64,
        occupied: bool,
        external: Option<u64>,
        sound_irq: &crate::hardware::vdp::SoundInterrupt,
    ) -> bool {
        let reservation = [
            Reservation {
                requester: Requester::MAIN,
                ticks: begin..self.bus.now.get(),
            },
            Reservation {
                requester: Requester::VDP,
                ticks: begin..external.unwrap_or(begin),
            },
        ];
        let occupied = match (occupied, external.is_some()) {
            (true, true) => &reservation[..],
            (true, false) => &reservation[..1],
            (false, true) => &reservation[1..],
            (false, false) => &[],
        };
        let mut interval = SoundInterval::new(begin..self.bus.now.get(), &mut self.bus.audio);
        interval.occupied = occupied;
        interval.sound_irq = Some(sound_irq);
        self.schedule.advance_owned(
            self.cpu,
            &mut self.bus.sound,
            &mut self.bus.cartridge,
            &mut self.bus.ram,
            (interval, &mut self.capture),
        )
    }

    fn transfer(
        &mut self,
        access: BusAccess,
        start: CpuClocks,
        request: VdpTransferRequest,
        ports: &mut TransferPorts,
    ) -> CycleResult<(), BusError> {
        self.transfer_observed(access, start, false, request, ports)
    }

    fn transfer_observed(
        &mut self,
        access: BusAccess,
        start: CpuClocks,
        retain_prefetch: bool,
        request: VdpTransferRequest,
        ports: &mut TransferPorts,
    ) -> CycleResult<(), BusError> {
        ports.receipt = None;
        ports.vdp = VdpTransfer::default();
        let direction = request.direction();
        let mut waits = 0;
        let result = (|| {
            if start != self.bus.cpu_clock {
                return Err(BusError::ClockMismatch {
                    expected: self.bus.cpu_clock,
                    actual: start,
                });
            }
            if self.bus.clock_fault {
                return Err(BusError::ClockOverflow);
            }
            self.wait_external_dma(&mut waits)?;
            self.wait_svp_main(&mut waits)?;
            let begin = self.bus.now.get();
            let release = self
                .schedule
                .pending
                .as_ref()
                .and_then(|pending| pending.reservation())
                .filter(|slot| slot.requester != Requester::MAIN)
                .map_or(begin, |slot| slot.ticks.end.max(begin));
            let delay = u32::try_from((release - begin).div_ceil(MAIN_CPU_DIVISOR))
                .map_err(|_| BusError::ClockOverflow)?;
            let clocks = u64::from(delay) + u64::from(TRANSFER_CLOCKS);
            self.bus
                .cpu_clock
                .0
                .checked_add(clocks)
                .ok_or(BusError::ClockOverflow)?;
            let end = begin
                .checked_add(clocks * MAIN_CPU_DIVISOR)
                .ok_or(BusError::ClockOverflow)?;
            if delay != 0 {
                let sound_irq = self.bus.sound_interrupt();
                let total = waits.checked_add(delay).ok_or(BusError::ClockOverflow)?;
                self.bus
                    .advance_main_cycle(self.bus.cpu_clock, delay, false)?;
                waits = total;
                self.bus.prefetch = Prefetch::Empty;
                self.schedule.advance_owned(
                    self.cpu,
                    &mut self.bus.sound,
                    &mut self.bus.cartridge,
                    &mut self.bus.ram,
                    (
                        SoundInterval {
                            sound_irq: Some(&sound_irq),
                            audio: &mut self.bus.audio,
                            ticks: begin..self.bus.now.get(),
                            occupied: &[Reservation {
                                requester: Requester::MAIN,
                                ticks: release..end,
                            }],
                        },
                        &mut self.capture,
                    ),
                );
            }
            if direction == Direction::Write {
                ports.receipt = self.bus.prepare_zero_fill_transfer(access)?;
            }
            ports.vdp = self.bus.begin_vdp_transfer(access, request)?;
            self.advance_main_cycle_observed(
                self.bus.cpu_clock,
                TRANSFER_CLOCKS,
                true,
                access.kind == AccessKind::InterruptAcknowledge,
                retain_prefetch,
            )?;
            if (SOUND_APERTURE_START..=SOUND_APERTURE_END).contains(&access.bus_address)
                && self.bus.sound.requested
                && !self.bus.sound.reset_asserted
                && !self.bus.sound.granted
                && let Some(pending) = &self.schedule.pending
            {
                let remaining = pending.deadline.saturating_sub(self.bus.now.get());
                let extra = u32::try_from(remaining.div_ceil(MAIN_CPU_DIVISOR))
                    .map_err(|_| BusError::ClockOverflow)?;
                let total = waits.checked_add(extra).ok_or(BusError::ClockOverflow)?;
                self.advance_main(self.bus.cpu_clock, extra, false)?;
                ports.receipt = None;
                waits = total;
            }
            Ok(())
        })();
        if result.is_err() {
            ports.receipt = None;
        }
        CycleResult {
            result,
            wait_clocks: waits,
        }
    }

    fn read_discarded_cycle_with<E>(
        &mut self,
        access: BusAccess,
        start: CpuClocks,
        convert: impl FnOnce(BusError) -> E,
    ) -> CycleResult<(), E> {
        if !self.bus.discarded_read(access) || self.bus.rmw.is_some() {
            let cycle = self.read_cycle_with(access, start, convert);
            return CycleResult {
                result: cycle.result.map(|_| ()),
                wait_clocks: cycle.wait_clocks,
            };
        }
        let epoch_access = self.begin_epoch_access(false);
        let transfer = self.transfer(
            access,
            start,
            VdpTransferRequest::Read,
            &mut TransferPorts::default(),
        );
        let result = transfer
            .result
            .map(|()| self.bus.complete_discarded_read(access));
        self.end_epoch_access(epoch_access);
        CycleResult {
            result: result.map_err(convert),
            wait_clocks: transfer.wait_clocks,
        }
    }

    pub(super) fn read_cycle_with<E>(
        &mut self,
        access: BusAccess,
        start: CpuClocks,
        convert: impl FnOnce(BusError) -> E,
    ) -> CycleResult<u16, E> {
        instruction_input::value(self.read_input_cycle_with(access, start, convert))
    }

    fn acknowledge_cycle_with<E>(
        &mut self,
        access: BusAccess,
        start: CpuClocks,
        convert: impl FnOnce(BusError) -> E,
    ) -> CycleResult<BusResponse<InterruptVector>, E> {
        if let Err(error) = self.bus.reject_rmw_transfer(access) {
            return CycleResult {
                result: Err(convert(error)),
                wait_clocks: 0,
            };
        }
        let epoch_access = self.begin_epoch_access(false);
        let transfer = self.transfer(
            access,
            start,
            VdpTransferRequest::Read,
            &mut TransferPorts::default(),
        );
        self.end_epoch_access(epoch_access);
        CycleResult {
            result: transfer
                .result
                .map(|_| {
                    self.bus.prefetch = Prefetch::Empty;
                    BusResponse::Complete(InterruptVector::Autovector)
                })
                .map_err(convert),
            wait_clocks: transfer.wait_clocks,
        }
    }

    fn controls_changed(&mut self) {
        if self.bus.sound.reset_asserted {
            self.schedule
                .reset_cpu(self.cpu, &mut self.bus.sound, &mut self.capture);
        } else if self.schedule.pending.is_none() {
            self.bus.sound.complete_boundary();
        }
    }
}
