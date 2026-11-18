use super::*;
use crate::hardware::bus::rmw::Phase;

impl<C: SoundCapture> ScheduledBus<'_, C> {
    fn advance_rmw(&mut self, clocks: u32) -> Result<(), BusError> {
        let begin = self.bus.now.get();
        let sound_irq = self.bus.sound_interrupt();
        let reservation = self.bus.rmw.as_ref().unwrap().reservation.clone().unwrap();
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
                    ticks: begin..self.bus.now.get(),
                    occupied: std::slice::from_ref(&reservation),
                    audio: &mut self.bus.audio,
                    sound_irq: Some(&sound_irq),
                },
                &mut self.capture,
            ),
        );
        Ok(())
    }

    pub(super) fn read_ram_rmw(
        &mut self,
        access: BusAccess,
        start: CpuClocks,
    ) -> CycleResult<u16, BusError> {
        let mut waits = 0;
        let result = (|| {
            self.bus
                .check_rmw_phase(access, start, Phase::Armed, Direction::Read)?;
            self.wait_external_dma(&mut waits)?;
            let begin = self.bus.now.get();
            let release = self
                .schedule
                .pending
                .as_ref()
                .filter(|pending| pending.any_bank_access || pending.bank_access.is_some())
                .map_or(begin, |pending| pending.deadline.max(begin));
            let delay = u32::try_from((release - begin).div_ceil(MAIN_CPU_DIVISOR))
                .map_err(|_| BusError::ClockOverflow)?;
            let total = waits.checked_add(delay).ok_or(BusError::ClockOverflow)?;
            self.bus.arm_rmw_interval(delay)?;
            if delay != 0 {
                self.advance_rmw(delay)?;
                waits = total;
            }
            self.advance_rmw(TRANSFER_CLOCKS)?;
            let value = self.bus.read(access)?;
            self.bus.complete_rmw_phase(Phase::ReadComplete);
            Ok(value)
        })();
        CycleResult {
            result,
            wait_clocks: waits,
        }
    }

    pub(super) fn idle_ram_rmw(&mut self, start: CpuClocks, clocks: u32) -> Result<(), BusError> {
        self.bus.check_rmw_idle(start, clocks)?;
        self.advance_rmw(clocks)?;
        self.bus.complete_rmw_phase(Phase::IdleComplete);
        Ok(())
    }

    pub(super) fn write_ram_rmw(
        &mut self,
        access: BusAccess,
        value: u16,
        start: CpuClocks,
    ) -> CycleResult<(), BusError> {
        let result = (|| {
            self.bus
                .check_rmw_phase(access, start, Phase::IdleComplete, Direction::Write)?;
            self.advance_rmw(TRANSFER_CLOCKS)?;
            self.bus.write(access, value)?;
            self.bus.complete_rmw_phase(Phase::WriteComplete);
            Ok(())
        })();
        CycleResult {
            result,
            wait_clocks: 0,
        }
    }

    pub(super) fn end_ram_rmw(&mut self) {
        if let Some(scope) = self.bus.rmw.take()
            && let Some(reservation) = scope.reservation
        {
            self.schedule
                .finish_rmw(&reservation, self.bus.now.get(), &mut self.capture);
        }
    }
}
