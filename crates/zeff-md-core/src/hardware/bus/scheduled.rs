use super::ownership::{Requester, Reservation};
use super::sound::Control;
use super::*;
use crate::hardware::sound::schedule::{SoundInterval, SoundSchedule};
use zeff_z80::Cpu;

pub(crate) struct ScheduledBus<'a> {
    pub bus: &'a mut Bus,
    pub cpu: &'a mut Cpu,
    pub schedule: &'a mut SoundSchedule,
}

impl ScheduledBus<'_> {
    fn wait_external_dma(&mut self, waits: &mut u32) -> Result<(), BusError> {
        if let Some((clocks, release)) = self.bus.external_dma_wait()? {
            let total = waits.checked_add(clocks).ok_or(BusError::ClockOverflow)?;
            let begin = self.bus.now.get();
            let sound_irq = self.bus.sound_interrupt();
            self.bus
                .advance_main_cycle(self.bus.cpu_clock, clocks, false)?;
            self.bus.prefetch_upper = None;
            self.schedule.advance(
                self.cpu,
                &mut self.bus.sound,
                &self.bus.cartridge,
                &mut self.bus.ram,
                SoundInterval {
                    sound_irq: Some(sound_irq),
                    audio: &mut self.bus.audio,
                    ticks: begin..self.bus.now.get(),
                    occupied: &[Reservation {
                        requester: Requester::VDP,
                        ticks: begin..release,
                    }],
                },
            );
            *waits = total;
        }
        Ok(())
    }
    pub fn advance_machine(&mut self, ticks: u64) -> Result<(), BusError> {
        let begin = self.bus.now.get();
        let sound_irq = self.bus.sound_interrupt();
        let external = self.bus.external_dma_wait()?.map(|(_, release)| release);
        self.bus.advance_machine_cycle(ticks, false)?;
        if ticks != 0 {
            self.bus.prefetch_upper = None;
        }
        self.advance_sound(begin, false, external, sound_irq);
        Ok(())
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
        let begin = self.bus.now.get();
        let sound_irq = self.bus.sound_interrupt();
        let external = self.bus.external_dma_wait()?.map(|(_, release)| release);
        let sound_active = !self.bus.sound.reset_asserted && !self.bus.sound.granted;
        self.bus.advance_main_cycle(start, clocks, acknowledge)?;
        if clocks != 0 && sound_active {
            self.bus.prefetch_upper = None;
        }
        self.advance_sound(begin, occupied, external, sound_irq);
        Ok(())
    }

    fn advance_sound(
        &mut self,
        begin: u64,
        occupied: bool,
        external: Option<u64>,
        sound_irq: crate::hardware::vdp::SoundInterrupt,
    ) {
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
        self.schedule.advance(
            self.cpu,
            &mut self.bus.sound,
            &self.bus.cartridge,
            &mut self.bus.ram,
            interval,
        );
    }

    fn transfer(&mut self, access: BusAccess, start: CpuClocks) -> CycleResult<(), BusError> {
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
                self.bus.prefetch_upper = None;
                self.schedule.advance(
                    self.cpu,
                    &mut self.bus.sound,
                    &self.bus.cartridge,
                    &mut self.bus.ram,
                    SoundInterval {
                        sound_irq: Some(sound_irq),
                        audio: &mut self.bus.audio,
                        ticks: begin..self.bus.now.get(),
                        occupied: &[Reservation {
                            requester: Requester::MAIN,
                            ticks: release..end,
                        }],
                    },
                );
            }
            self.advance_main_cycle(
                self.bus.cpu_clock,
                TRANSFER_CLOCKS,
                true,
                access.kind == AccessKind::InterruptAcknowledge,
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
                waits = total;
            }
            Ok(())
        })();
        CycleResult {
            result,
            wait_clocks: waits,
        }
    }

    fn controls_changed(&mut self) {
        if self.bus.sound.reset_asserted {
            self.schedule.reset_cpu(self.cpu, &mut self.bus.sound);
        } else if self.schedule.pending.is_none() {
            self.bus.sound.complete_boundary();
        }
    }
}

impl ClockedBus for ScheduledBus<'_> {
    type Error = BusError;
    fn read_cycle(&mut self, access: BusAccess, start: CpuClocks) -> CycleResult<u16, BusError> {
        let mut transfer = self.transfer(access, start);
        let result = transfer.result.and_then(|()| {
            while let Some(clocks) = self.bus.vdp_read_wait(access)? {
                let total = transfer
                    .wait_clocks
                    .checked_add(clocks)
                    .ok_or(BusError::ClockOverflow)?;
                self.advance_main(self.bus.cpu_clock, clocks, true)?;
                transfer.wait_clocks = total;
            }
            self.bus.read(access)
        });
        CycleResult {
            result,
            wait_clocks: transfer.wait_clocks,
        }
    }
    fn write_cycle(
        &mut self,
        access: BusAccess,
        value: u16,
        start: CpuClocks,
    ) -> CycleResult<(), BusError> {
        let mut transfer = self.transfer(access, start);
        let result = transfer.result.and_then(|()| {
            self.bus.prefetch_upper = None;
            while let Some(clocks) = self.bus.vdp_data_wait(access)? {
                let total = transfer
                    .wait_clocks
                    .checked_add(clocks)
                    .ok_or(BusError::ClockOverflow)?;
                self.advance_main(self.bus.cpu_clock, clocks, true)?;
                transfer.wait_clocks = total;
            }
            let result = self.bus.write(access, value);
            if result.is_ok()
                && access.lanes != ByteLanes::Lower
                && Control::decode(access.bus_address).is_some()
            {
                self.controls_changed();
            }
            result?;
            self.wait_external_dma(&mut transfer.wait_clocks)
        });
        CycleResult {
            result,
            wait_clocks: transfer.wait_clocks,
        }
    }
    fn idle(&mut self, start: CpuClocks, clocks: u32) {
        if self.advance_main(start, clocks, false).is_err() {
            self.bus.clock_fault = true;
        } else if clocks != 0 {
            self.bus.prefetch_upper = None;
        }
    }
    fn acknowledge_cycle(
        &mut self,
        access: BusAccess,
        start: CpuClocks,
    ) -> CycleResult<BusResponse<InterruptVector>, BusError> {
        let transfer = self.transfer(access, start);
        CycleResult {
            result: transfer.result.map(|_| {
                self.bus.prefetch_upper = None;
                BusResponse::Complete(InterruptVector::Autovector)
            }),
            wait_clocks: transfer.wait_clocks,
        }
    }
    fn begin_reset_output(&mut self, start: CpuClocks) -> Result<BusResponse<()>, BusError> {
        self.bus.begin_reset_output(start)
    }
    fn end_reset_output(&mut self, end: CpuClocks) {
        self.bus.end_reset_output(end);
    }
}
