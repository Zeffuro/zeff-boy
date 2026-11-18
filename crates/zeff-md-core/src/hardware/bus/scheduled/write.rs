use super::*;

impl<C: SoundCapture> ScheduledBus<'_, C> {
    pub(in crate::hardware::bus) fn write_cycle_with<E>(
        &mut self,
        access: BusAccess,
        value: u16,
        start: CpuClocks,
        convert: impl FnOnce(BusError) -> E,
    ) -> CycleResult<(), E> {
        self.write_cycle_capturing(access, value, start, convert, &mut ())
    }

    pub(in crate::hardware::bus) fn write_cycle_capturing<E, S: previous::Capture>(
        &mut self,
        access: BusAccess,
        value: u16,
        start: CpuClocks,
        convert: impl FnOnce(BusError) -> E,
        previous: &mut S,
    ) -> CycleResult<(), E> {
        if let Err(error) = self
            .bus
            .preflight_svp_write(access, value)
            .and_then(|()| self.bus.preflight_bounded_odd(access, Direction::Write))
            .and_then(|()| self.bus.preflight_bounded_switched(access, Some(value)))
            .and_then(|()| self.bus.preflight_save_latch(access, Direction::Write))
            .and_then(|()| self.bus.preflight_download(access, Direction::Write))
        {
            return CycleResult {
                result: Err(convert(error)),
                wait_clocks: 0,
            };
        }
        if self.bus.rmw.is_some() {
            let result = self.write_ram_rmw(access, value, start);
            return CycleResult {
                result: result.result.map_err(convert),
                wait_clocks: result.wait_clocks,
            };
        }
        if let Err(error) = self
            .bus
            .check_main_clock(start)
            .and_then(|()| self.bus.preflight_vdp_transfer(access, Direction::Write))
        {
            return CycleResult {
                result: Err(convert(error)),
                wait_clocks: 0,
            };
        }
        if self
            .bus
            .cartridge
            .registers
            .as_ref()
            .is_some_and(|board| board.is_banked())
            && let Err(error) = self
                .bus
                .check_main_clock(start)
                .and_then(|()| self.bus.preflight_banked_register_write(access))
        {
            return CycleResult {
                result: Err(convert(error)),
                wait_clocks: 0,
            };
        }
        if self.bus.cartridge.parallel_adapter.is_some()
            && let Err(error) = self
                .bus
                .check_main_clock(start)
                .and_then(|()| self.bus.preflight_parallel_adapter_write(access, value))
        {
            return CycleResult {
                result: Err(convert(error)),
                wait_clocks: 0,
            };
        }
        if self.bus.cartridge.realtec.is_some()
            && let Err(error) = self
                .bus
                .check_main_clock(start)
                .and_then(|()| self.bus.preflight_realtec_write(access))
        {
            return CycleResult {
                result: Err(convert(error)),
                wait_clocks: 0,
            };
        }
        if self.bus.cartridge.extended_ssf.is_some()
            && let Err(error) = self
                .bus
                .check_main_clock(start)
                .and_then(|()| self.bus.preflight_extended_ssf_write(access))
        {
            return CycleResult {
                result: Err(convert(error)),
                wait_clocks: 0,
            };
        }
        if self.bus.cartridge.sf004.is_some()
            && let Err(error) = self
                .bus
                .check_main_clock(start)
                .and_then(|()| self.bus.preflight_sf004_write(access, value))
        {
            return CycleResult {
                result: Err(convert(error)),
                wait_clocks: 0,
            };
        }
        if self.bus.cartridge.sf002.is_some() {
            let check = (|| {
                self.bus.check_main_clock(start)?;
                let owned = self.bus.cartridge.preflight_sf002_write(access, value);
                if owned.is_none() {
                    return Ok(());
                }
                let pending = self
                    .schedule
                    .pending
                    .as_ref()
                    .filter(|pending| !pending.sf002_writes.is_empty());
                let accepted = if let Some(pending) = pending {
                    let mut projected = self.bus.cartridge.clone();
                    pending.sf002_writes.project(&mut projected)?;
                    projected.preflight_sf002_write(access, value)
                } else {
                    owned
                };
                if accepted == Some(false) {
                    return Err(BusError::UnsupportedAccess {
                        address: access.bus_address,
                        direction: Direction::Write,
                    });
                }
                Ok(())
            })();
            if let Err(error) = check {
                return CycleResult {
                    result: Err(convert(error)),
                    wait_clocks: 0,
                };
            }
        }
        let input_begin = self.bus.now.get();
        let epoch_access = self
            .begin_epoch_access((WORK_RAM_START..=BUS_ADDRESS_MASK).contains(&access.bus_address));
        let held = self.bus.hold_svp_main(access, Direction::Write);
        let mut ports = TransferPorts::default();
        let mut transfer =
            self.transfer(access, start, VdpTransferRequest::Write(value), &mut ports);
        let result = transfer.result.and_then(|()| {
            self.bus.prefetch = Prefetch::Empty;
            while let Some(clocks) = self.bus.vdp_data_wait(access, ports.receipt.as_ref())? {
                let total = transfer
                    .wait_clocks
                    .checked_add(clocks)
                    .ok_or(BusError::ClockOverflow)?;
                self.advance_main(self.bus.cpu_clock, clocks, true)?;
                transfer.wait_clocks = total;
            }
            let result = self.bus.write_zero_fill_timed_capturing(
                (access, value),
                input_begin,
                ports.receipt.take(),
                &mut ports.vdp,
                previous,
            );
            if result.is_ok()
                && access.lanes != ByteLanes::Lower
                && Control::decode(access.bus_address).is_some()
            {
                self.controls_changed();
            }
            result?;
            self.wait_external_dma(&mut transfer.wait_clocks)
        });
        self.bus.release_svp_main(held);
        if result.is_err() && ports.vdp.data_started {
            self.bus.vdp.abort_main_data();
        }
        self.end_epoch_access(epoch_access);
        CycleResult {
            result: result.map_err(convert),
            wait_clocks: transfer.wait_clocks,
        }
    }
}
