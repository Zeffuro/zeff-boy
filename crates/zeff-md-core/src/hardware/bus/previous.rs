use super::*;

pub(super) trait Capture {
    fn capture(&mut self, ram: &[u8], index: usize, lanes: ByteLanes);
}

impl Capture for () {
    #[inline]
    fn capture(&mut self, _: &[u8], _: usize, _: ByteLanes) {}
}

#[derive(Default)]
pub(super) struct Previous(pub(super) [Option<u8>; 2]);

impl Previous {
    pub(super) fn finish(self, succeeded: bool) -> [Option<u8>; 2] {
        if succeeded { self.0 } else { [None, None] }
    }
}

impl Capture for Previous {
    fn capture(&mut self, ram: &[u8], index: usize, lanes: ByteLanes) {
        // Only the accepted storage assignment owns this passive receipt.
        if lanes != ByteLanes::Lower {
            self.0[0] = Some(ram[index]);
        }
        if lanes != ByteLanes::Upper {
            self.0[1] = Some(ram[(index + 1) & WORK_RAM_MASK]);
        }
    }
}

impl Bus {
    pub(super) fn write_cycle_capturing<S: previous::Capture>(
        &mut self,
        access: BusAccess,
        value: u16,
        start: CpuClocks,
        previous: &mut S,
    ) -> CycleResult<(), BusError> {
        if let Err(error) = self
            .preflight_svp_write(access, value)
            .and_then(|()| self.preflight_bounded_odd(access, Direction::Write))
            .and_then(|()| self.preflight_bounded_switched(access, Some(value)))
            .and_then(|()| self.preflight_save_latch(access, Direction::Write))
            .and_then(|()| self.preflight_download(access, Direction::Write))
        {
            return CycleResult {
                result: Err(error),
                wait_clocks: 0,
            };
        }
        if self.rmw.is_some() {
            return self.write_ram_rmw(access, value, start);
        }
        let held = self.hold_svp_main(access, Direction::Write);
        let mut waits = 0u32;
        let mut ports = VdpTransfer::default();
        let result = (|| {
            self.check_main_clock(start)?;
            self.preflight_banked_register_write(access)?;
            self.preflight_protection_write(access)?;
            self.preflight_parallel_adapter_write(access, value)?;
            self.preflight_sf002_write(access, value)?;
            self.preflight_sf004_write(access, value)?;
            self.preflight_extended_ssf_write(access)?;
            self.preflight_realtec_write(access)?;
            self.preflight_vdp_transfer(access, Direction::Write)?;
            let input_begin = self.now.get();
            self.wait_external_dma(&mut waits)?;
            self.wait_svp_main(&mut waits)?;
            let receipt = self.prepare_zero_fill_transfer(access)?;
            ports = self.begin_vdp_transfer(access, VdpTransferRequest::Write(value))?;
            self.advance_main(self.cpu_clock, TRANSFER_CLOCKS)?;
            self.prefetch = Prefetch::Empty;
            while let Some(clocks) = self.vdp_data_wait(access, receipt.as_ref())? {
                let total = waits.checked_add(clocks).ok_or(BusError::ClockOverflow)?;
                self.advance_main(self.cpu_clock, clocks)?;
                waits = total;
            }
            self.write_zero_fill_timed_capturing(
                (access, value),
                input_begin,
                receipt,
                &mut ports,
                previous,
            )?;
            self.wait_external_dma(&mut waits)
        })();
        self.release_svp_main(held);
        if result.is_err() && ports.data_started {
            self.vdp.abort_main_data();
        }
        CycleResult {
            result,
            wait_clocks: waits,
        }
    }
}

#[cfg(test)]
mod tests;
