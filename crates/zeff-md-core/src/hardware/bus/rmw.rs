use super::ownership::{Requester, Reservation};
use super::*;
use zeff_m68k::bus::FunctionCode;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Phase {
    Armed,
    ReadComplete,
    IdleComplete,
    WriteComplete,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RamRmw {
    pub(super) access: BusAccess,
    pub(super) next: CpuClocks,
    pub(super) phase: Phase,
    pub(crate) reservation: Option<Reservation>,
}

impl Bus {
    pub(super) fn begin_ram_rmw(
        &mut self,
        access: BusAccess,
        start: CpuClocks,
    ) -> Result<BusResponse<()>, BusError> {
        self.check_main_clock(start)?;
        let lane = if access.logical_address & 1 == 0 {
            ByteLanes::Upper
        } else {
            ByteLanes::Lower
        };
        if self.rmw.is_some()
            || self.reset_output
            || access.kind != AccessKind::Data
            || !matches!(
                access.function_code,
                FunctionCode::UserData | FunctionCode::SupervisorData
            )
            || access.lanes != lane
            || access.bus_address != access.logical_address & BUS_ADDRESS_MASK & !1
            || !(WORK_RAM_START..BUS_ADDRESS_MASK).contains(&access.bus_address)
        {
            return Ok(BusResponse::Unsupported);
        }
        self.rmw_headroom(0)?;
        self.rmw = Some(RamRmw {
            access,
            next: start,
            phase: Phase::Armed,
            reservation: None,
        });
        Ok(BusResponse::Complete(()))
    }

    pub(super) fn rmw_headroom(&self, delay: u32) -> Result<(), BusError> {
        let clocks = u64::from(delay) + u64::from(RAM_TAS_CLOCKS);
        self.cpu_clock
            .0
            .checked_add(clocks)
            .ok_or(BusError::ClockOverflow)?;
        self.now
            .get()
            .checked_add(clocks * MAIN_CPU_DIVISOR)
            .ok_or(BusError::ClockOverflow)?;
        Ok(())
    }

    pub(super) fn arm_rmw_interval(&mut self, delay: u32) -> Result<(), BusError> {
        self.rmw_headroom(delay)?;
        let start = self.now.get() + u64::from(delay) * MAIN_CPU_DIVISOR;
        let scope = self.rmw.as_mut().unwrap();
        scope.next = CpuClocks(self.cpu_clock.0 + u64::from(delay));
        scope.reservation = Some(Reservation {
            requester: Requester::MAIN_RMW,
            ticks: start..start + u64::from(RAM_TAS_CLOCKS) * MAIN_CPU_DIVISOR,
        });
        Ok(())
    }

    pub(super) fn check_rmw_phase(
        &self,
        access: BusAccess,
        start: CpuClocks,
        phase: Phase,
        direction: Direction,
    ) -> Result<(), BusError> {
        self.check_main_clock(start)?;
        if self.rmw.as_ref().is_some_and(|scope| {
            scope.access == access && scope.next == start && scope.phase == phase
        }) {
            Ok(())
        } else {
            Err(BusError::UnsupportedAccess {
                address: access.bus_address,
                direction,
            })
        }
    }

    pub(super) fn check_rmw_idle(&self, start: CpuClocks, clocks: u32) -> Result<(), BusError> {
        let scope = self.rmw.as_ref().unwrap();
        self.check_rmw_phase(scope.access, start, Phase::ReadComplete, Direction::Write)?;
        if clocks != RAM_TAS_MODIFY_CLOCKS {
            return Err(BusError::UnsupportedAccess {
                address: scope.access.bus_address,
                direction: Direction::Write,
            });
        }
        Ok(())
    }

    pub(super) fn complete_rmw_phase(&mut self, phase: Phase) {
        let scope = self.rmw.as_mut().unwrap();
        scope.phase = phase;
        scope.next = self.cpu_clock;
    }

    pub(super) fn suppress_rmw_write(&self, access: BusAccess) -> bool {
        self.rmw
            .as_ref()
            .is_some_and(|scope| scope.access == access && scope.phase == Phase::IdleComplete)
    }

    pub(super) fn reject_rmw_transfer(&self, access: BusAccess) -> Result<(), BusError> {
        if self.rmw.is_some() {
            return Err(BusError::UnsupportedAccess {
                address: access.bus_address,
                direction: Direction::Read,
            });
        }
        Ok(())
    }

    pub(super) fn read_ram_rmw(
        &mut self,
        access: BusAccess,
        start: CpuClocks,
    ) -> CycleResult<u16, BusError> {
        let mut waits = 0;
        let result = (|| {
            self.check_rmw_phase(access, start, Phase::Armed, Direction::Read)?;
            self.wait_external_dma(&mut waits)?;
            self.arm_rmw_interval(0)?;
            self.advance_main(self.cpu_clock, TRANSFER_CLOCKS)?;
            let value = self.read(access)?;
            self.complete_rmw_phase(Phase::ReadComplete);
            Ok(value)
        })();
        CycleResult {
            result,
            wait_clocks: waits,
        }
    }

    pub(super) fn idle_ram_rmw(&mut self, start: CpuClocks, clocks: u32) -> Result<(), BusError> {
        self.check_rmw_idle(start, clocks)?;
        self.advance_main(start, clocks)?;
        self.prefetch = Prefetch::Empty;
        self.complete_rmw_phase(Phase::IdleComplete);
        Ok(())
    }

    pub(super) fn write_ram_rmw(
        &mut self,
        access: BusAccess,
        value: u16,
        start: CpuClocks,
    ) -> CycleResult<(), BusError> {
        let result = (|| {
            self.check_rmw_phase(access, start, Phase::IdleComplete, Direction::Write)?;
            self.advance_main(start, TRANSFER_CLOCKS)?;
            self.write(access, value)?;
            self.complete_rmw_phase(Phase::WriteComplete);
            Ok(())
        })();
        CycleResult {
            result,
            wait_clocks: 0,
        }
    }
}
