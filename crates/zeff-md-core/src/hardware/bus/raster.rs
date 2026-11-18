use super::*;

impl Bus {
    fn raster_clock_publication(&self) -> Option<MasterTicks> {
        self.vdp
            .registers()
            .pending()
            .filter(|write| matches!(write.index, 1 | 12))
            .map(|write| write.publish_at)
    }

    pub(super) fn advance_video_to(&mut self, next: MasterTicks) -> Result<(), BusError> {
        self.advance_video_to_with_svp_dram(next, None)
    }

    pub(super) fn advance_video_to_with_svp_dram(
        &mut self,
        next: MasterTicks,
        dram: Option<&crate::svp::DramReadView>,
    ) -> Result<(), BusError> {
        let row = self
            .vdp
            .next_row_deadline(self.region)
            .map_err(BusError::Vdp)?;
        let fetch = self
            .vdp
            .next_scroll_deadline(self.region)
            .map_err(BusError::Vdp)?;
        let due = [
            row,
            fetch,
            self.video.deadline(),
            self.raster_clock_publication(),
        ]
        .into_iter()
        .flatten()
        .any(|at| at <= next);
        if due {
            self.vdp
                .preflight_with_source(
                    next,
                    self.region,
                    VdpDmaSource::from_cartridge(&self.cartridge, &self.ram)
                        .with_tmss(self.tmss.as_ref())
                        .with_svp_dram(dram),
                )
                .map_err(BusError::Vdp)?;
            self.skip_raster_fields(next, dram)?;
            loop {
                let row = self
                    .vdp
                    .next_row_deadline(self.region)
                    .map_err(BusError::Vdp)?;
                let fetch = self
                    .vdp
                    .next_scroll_deadline(self.region)
                    .map_err(BusError::Vdp)?;
                let Some(at) = [
                    row,
                    fetch,
                    self.video.deadline(),
                    self.raster_clock_publication(),
                ]
                .into_iter()
                .flatten()
                .min() else {
                    break;
                };
                if at >= next {
                    break;
                }
                self.advance_raster_deadline(at, row == Some(at), true, dram)
                    .expect("validated raster prefix");
            }
        }
        let row = self
            .vdp
            .next_row_deadline(self.region)
            .map_err(BusError::Vdp)?
            == Some(next);
        self.advance_raster_deadline(next, row, true, dram)
    }

    fn skip_raster_fields(
        &mut self,
        next: MasterTicks,
        dram: Option<&crate::svp::DramReadView>,
    ) -> Result<(), BusError> {
        if !self.vdp.progressive_skip_allowed() {
            return Ok(());
        }
        let field = self
            .vdp
            .field_position(self.region)
            .map_err(BusError::Vdp)?;
        let period = self.region.frame_lines() * VDP_LINE_TICKS;
        let skip = MasterTicks::new(
            field.start.get()
                + ((next.get().saturating_sub(field.start.get()) / period).saturating_sub(1))
                    * period,
        );
        if skip <= self.vdp.now() {
            return Ok(());
        }
        let fetch = self
            .vdp
            .next_scroll_deadline(self.region)
            .map_err(BusError::Vdp)?
            .map(|at| {
                let phase = (at.get() - field.line_start.get()) % field.line_ticks;
                MasterTicks::new(skip.get() - field.line_ticks + phase)
            })
            .filter(|at| *at > self.vdp.now());
        let capture = self.video.deadline().filter(|at| *at < skip);
        self.video.skip_scroll_rows();
        let mut events = [fetch, capture];
        events.sort();
        let mut previous = None;
        for at in events.into_iter().flatten() {
            if previous != Some(at) {
                self.advance_raster_deadline(at, false, fetch == Some(at), dram)
                    .expect("validated skipped raster prefix");
                previous = Some(at);
            }
        }
        #[cfg(feature = "first-failure-diagnostic")]
        let observed = crate::diagnostic::field::enabled()
            .then(|| crate::diagnostic::field::Raster::read(&self.vdp, self.region));
        self.vdp
            .advance_with_source(
                skip,
                self.region,
                VdpDmaSource::from_cartridge(&self.cartridge, &self.ram)
                    .with_tmss(self.tmss.as_ref())
                    .with_svp_dram(dram),
            )
            .expect("validated skipped fields");
        #[cfg(feature = "first-failure-diagnostic")]
        if let Some(before) = observed {
            crate::diagnostic::field::skip(
                before,
                crate::diagnostic::field::Raster::read(&self.vdp, self.region),
                skip.get(),
                next.get(),
            );
        }
        Ok(())
    }

    fn advance_raster_deadline(
        &mut self,
        at: MasterTicks,
        row: bool,
        fetch: bool,
        dram: Option<&crate::svp::DramReadView>,
    ) -> Result<(), BusError> {
        #[cfg(feature = "first-failure-diagnostic")]
        let observed = crate::diagnostic::field::enabled()
            .then(|| crate::diagnostic::field::Raster::read(&self.vdp, self.region));
        let completed = if row {
            self.vdp
                .completed_row(at, self.region)
                .map_err(BusError::Vdp)?
        } else {
            None
        };
        if self.video.deadline() == Some(at) {
            let captured = self
                .vdp
                .advance_with_line_from(
                    at,
                    at,
                    false,
                    self.region,
                    Some(
                        VdpDmaSource::from_cartridge(&self.cartridge, &self.ram)
                            .with_tmss(self.tmss.as_ref())
                            .with_svp_dram(dram),
                    ),
                )
                .map_err(BusError::Vdp)?;
            self.video.complete(captured);
        } else {
            self.vdp
                .advance_with_source(
                    at,
                    self.region,
                    VdpDmaSource::from_cartridge(&self.cartridge, &self.ram)
                        .with_tmss(self.tmss.as_ref())
                        .with_svp_dram(dram),
                )
                .map_err(BusError::Vdp)?;
        }
        #[cfg(feature = "first-failure-diagnostic")]
        if let Some(before) = observed {
            crate::diagnostic::field::raster_advance(
                before,
                crate::diagnostic::field::Raster::read(&self.vdp, self.region),
                at.get(),
            );
        }
        let presentation = self.vdp.field_presentation_supported();
        if !presentation {
            self.video.invalidate_field(self.vdp.field_count());
        }
        if row && presentation {
            self.publish_raster_row(completed);
        }
        if fetch && presentation && self.vdp.is_scroll_fetch(self.region) {
            self.fetch_raster_scroll();
        }
        Ok(())
    }

    fn publish_raster_row(
        &mut self,
        completed: Option<(u16, crate::hardware::vdp::FieldPosition)>,
    ) {
        let Some((row, field)) = completed else {
            return;
        };
        let height = self.vdp.display_height(self.region);
        #[cfg(feature = "first-failure-diagnostic")]
        let _scope = if crate::diagnostic::field::enabled() {
            let registers = self.vdp.registers().registers();
            crate::diagnostic::field::RasterScope::begin(crate::diagnostic::field::RowContext {
                at: self.vdp.now().get(),
                completed_field: field.into(),
                row,
                raw: [registers[0], registers[1], registers[12]],
                height,
            })
        } else {
            None
        };
        if usize::from(row) < height {
            let horizontal = self.video.horizontal_scroll(field.id, row);
            let line = self
                .vdp
                .prepare_raster_with_horizontal(row, self.region, horizontal);
            self.video.publish_field_row(
                usize::from(row),
                row_delivery::RasterField {
                    id: field.id,
                    height,
                    mode: field.active_mode,
                    odd: field.active_odd,
                },
                line,
            );
        } else {
            #[cfg(feature = "first-failure-diagnostic")]
            if crate::diagnostic::field::enabled() {
                crate::diagnostic::field::outside_height();
            }
        }
    }

    fn fetch_raster_scroll(&mut self) {
        if self.vdp.scroll_preparation(self.region) {
            self.video.hold_scroll(
                self.vdp.display_enabled(),
                self.vdp.fetched_horizontal_scroll(255),
            );
        }
        if let Some((field, row)) = self.vdp.scroll_fetch_target(self.region) {
            self.video.fetch_scroll(
                field,
                row,
                self.vdp.display_enabled()
                    && usize::from(row) < self.vdp.display_height(self.region),
                self.vdp.fetched_horizontal_scroll(row),
            );
        }
    }
}
