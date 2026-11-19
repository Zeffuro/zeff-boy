use super::*;

impl<C: SoundCapture> ScheduledBus<'_, C> {
    pub(in crate::hardware::bus) fn read_input_cycle_with<E>(
        &mut self,
        access: BusAccess,
        start: CpuClocks,
        convert: impl FnOnce(BusError) -> E,
    ) -> CycleResult<InstructionInput, E> {
        if let Err(error) = self
            .bus
            .preflight_svp(access, Direction::Read)
            .and_then(|()| self.bus.preflight_bounded_odd(access, Direction::Read))
            .and_then(|()| self.bus.preflight_bounded_switched(access, None))
            .and_then(|()| self.bus.preflight_save_latch(access, Direction::Read))
            .and_then(|()| self.bus.preflight_download(access, Direction::Read))
        {
            return CycleResult {
                result: Err(convert(error)),
                wait_clocks: 0,
            };
        }
        if self.bus.rmw.is_some() {
            let result = self.read_ram_rmw(access, start);
            return CycleResult {
                result: result
                    .result
                    .map(InstructionInput::unknown)
                    .map_err(convert),
                wait_clocks: result.wait_clocks,
            };
        }
        if let Err(error) = self
            .bus
            .check_main_clock(start)
            .and_then(|()| self.bus.preflight_vdp_read(access))
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
                .and_then(|()| self.bus.preflight_realtec_read(access))
        {
            return CycleResult {
                result: Err(convert(error)),
                wait_clocks: 0,
            };
        }
        let input_begin = self.bus.now.get();
        let epoch_access = self.begin_epoch_access(
            (access.bus_address <= CARTRIDGE_END
                && self.bus.cartridge.sf001.is_none()
                && self.bus.cartridge.download.is_none())
                || (WORK_RAM_START..=BUS_ADDRESS_MASK).contains(&access.bus_address)
                || (access.bus_address & VDP_DECODE_MASK == VDP_DECODE_VALUE
                    && access.bus_address & VDP_PORT_MASK <= VDP_HV_END),
        );
        let held = self.bus.hold_svp_main(access, Direction::Read);
        let mut ports = TransferPorts::default();
        let mut transfer = self.transfer_observed(
            access,
            start,
            self.bus.closed_sound_read(access)
                || Bus::reset_observation(access)
                || Bus::request_observation(access)
                || Bus::unused_input_observation(access)
                || self.bus.unused_cartridge_observation(access),
            VdpTransferRequest::Read,
            &mut ports,
        );
        let result = transfer.result.and_then(|()| {
            while let Some(clocks) = self.bus.vdp_read_wait(access)? {
                let total = transfer
                    .wait_clocks
                    .checked_add(clocks)
                    .ok_or(BusError::ClockOverflow)?;
                self.advance_main(self.bus.cpu_clock, clocks, true)?;
                transfer.wait_clocks = total;
            }
            self.bus.read_timed(access, input_begin)
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
