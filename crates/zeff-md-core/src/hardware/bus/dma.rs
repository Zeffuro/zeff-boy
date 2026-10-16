use super::*;

impl Bus {
    pub(super) fn external_dma_wait(&self) -> Result<Option<(u32, u64)>, BusError> {
        let release = self
            .vdp
            .external_dma_release(
                self.region,
                VdpDmaSource::from_cartridge(&self.cartridge, &self.ram),
            )
            .map_err(BusError::Vdp)?;
        release
            .map(|at| {
                let clocks = u32::try_from((at.get() - self.now.get()).div_ceil(MAIN_CPU_DIVISOR))
                    .map_err(|_| BusError::ClockOverflow)?;
                Ok((clocks, at.get()))
            })
            .transpose()
    }

    pub(super) fn wait_external_dma(&mut self, waits: &mut u32) -> Result<(), BusError> {
        if let Some((clocks, _)) = self.external_dma_wait()? {
            let total = waits.checked_add(clocks).ok_or(BusError::ClockOverflow)?;
            self.advance_main(self.cpu_clock, clocks)?;
            *waits = total;
        }
        Ok(())
    }
}
