use super::*;

impl Bus {
    pub(in super::super) fn advance_functional_svp(
        &mut self,
        ticks: u64,
        acknowledge: bool,
    ) -> Result<(), BusError> {
        let target = self
            .now
            .get()
            .checked_add(ticks)
            .ok_or(BusError::ClockOverflow)?;
        let pixels_may_change = ticks != 0
            && (self.vdp.registers().pending().is_some()
                || self
                    .vdp
                    .next_row_deadline(self.region)
                    .map_or(true, |row| row.is_some_and(|at| at.get() <= target)));
        let checkpoint = (
            self.now,
            self.cartridge.svp.clone(),
            self.vdp.checkpoint_for_advance(),
            self.video.checkpoint_for_advance(pixels_may_change),
        );
        let result = self.advance_functional_svp_interval(ticks, acknowledge);
        if result.is_err() {
            let (now, model, vdp, video) = checkpoint;
            self.now = now;
            self.cartridge.svp = model;
            self.vdp.restore(vdp);
            self.video.restore(video);
        }
        result
    }

    fn advance_functional_svp_interval(
        &mut self,
        ticks: u64,
        acknowledge: bool,
    ) -> Result<(), BusError> {
        let target = self
            .now
            .get()
            .checked_add(ticks)
            .ok_or(BusError::ClockOverflow)?;
        while self.now.get() < target {
            let model = self.cartridge.svp.as_ref().unwrap();
            if model.functional_timing().unwrap().master_tick != self.now.get() {
                return Err(BusError::Owned(
                    "functional SVP/master clock mismatch".into(),
                ));
            }
            let end = model
                .functional_event_tick()
                .map_err(clock_error)?
                .map_or(target, |tick| tick.min(target));
            let dram = model.dram_read_view();
            let rom =
                crate::svp::CurrentRom::new(&self.cartridge.bytes, &self.cartridge.rom_overlay);
            self.cartridge
                .svp
                .as_mut()
                .unwrap()
                .advance_functional_current(end, Some(rom))
                .map_err(clock_error)?;
            // Equal-time VDP DMA observes DRAM from before this DSP grant.
            self.advance_video_to_with_svp_dram(MasterTicks::new(end), Some(&dram))?;
            self.now = MasterTicks::new(end);
        }
        if acknowledge && self.irq_level == 0 {
            self.vdp.acknowledge_cpu();
        }
        Ok(())
    }

    pub(in super::super) fn hold_svp_main(
        &mut self,
        access: BusAccess,
        direction: Direction,
    ) -> bool {
        if mutable(access, direction)
            && let Some(model) = &mut self.cartridge.svp
        {
            return model.hold_functional_main(true);
        }
        false
    }

    pub(in super::super) fn release_svp_main(&mut self, held: bool) {
        if held {
            self.cartridge
                .svp
                .as_mut()
                .unwrap()
                .hold_functional_main(false);
        }
    }

    pub(in super::super) fn svp_main_wait(&self) -> Result<Option<u32>, BusError> {
        let ticks = self
            .cartridge
            .svp
            .as_ref()
            .map(|model| model.functional_main_wait())
            .transpose()
            .map_err(clock_error)?
            .flatten();
        ticks
            .map(|ticks| {
                u32::try_from(ticks.div_ceil(MAIN_CPU_DIVISOR)).map_err(|_| BusError::ClockOverflow)
            })
            .transpose()
    }

    pub(in super::super) fn wait_svp_main(&mut self, waits: &mut u32) -> Result<(), BusError> {
        while let Some(clocks) = self.svp_main_wait()? {
            let total = waits.checked_add(clocks).ok_or(BusError::ClockOverflow)?;
            self.advance_main(self.cpu_clock, clocks)?;
            *waits = total;
        }
        Ok(())
    }
}

fn clock_error(cause: crate::svp::Error) -> BusError {
    BusError::Owned(format!("functional SVP: {cause}"))
}

fn mutable(access: BusAccess, direction: Direction) -> bool {
    matches!(
        (access.bus_address, direction),
        (
            0x300000..=0x31fffe | 0xa15000 | 0xa15002 | 0xa15006,
            Direction::Write
        ) | (0xa15004, Direction::Read)
    )
}
