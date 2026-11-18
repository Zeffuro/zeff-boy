use super::*;

impl<R: RenderBus> Domain<R> {
    pub fn write_state(&self, writer: &mut StateWriter) -> Result<()> {
        self.validate_state()?;
        ensure!(
            self.pending.is_none(),
            "cannot save unretired VDP physical effects"
        );
        writer.write_u8(7);
        writer.write_bool(self.pal);
        writer.write_u64(self.half);
        writer.write_bool(self.mode128);
        self.clocks.write_state(writer);
        self.hv.write_state(writer);
        self.irq.write_state(writer);
        self.status.write_state(writer);
        self.counter.write_state(writer);
        self.control.write_state(writer);
        self.frontend.write_state(writer);
        self.wiring.write_state(writer);
        self.ram.write_state(writer);
        self.internal.write_state(writer);
        self.render.write_state(writer);
        for value in self
            .old_dclk
            .into_iter()
            .chain(self.feedback)
            .chain(self.input_cache.levels)
        {
            writer.write_bool(value);
        }
        writer.write_bool(self.zclk_feedback);
        for clock in self.board_clocks {
            writer.write_bool(clock);
        }
        writer.write_bool(self.render_dirty);
        writer.write_u8(self.input_cache.ad);
        writer.write_u8(self.input_cache.rd);
        writer.write_u8(self.input_cache.sd);
        writer.write_u32(self.shared.address);
        writer.write_u16(self.shared.data);
        writer.write_u8(self.ram_bus.ad);
        writer.write_u8(self.ram_bus.rd);
        writer.write_u16(self.io_data);
        writer.write_bool(self.memory_active);
        writer.write_bool(self.fifo_full);
        Ok(())
    }

    pub fn read_state(reader: &mut StateReader<'_>, pal: bool) -> Result<Self> {
        ensure!(
            reader.read_u8()? == 7,
            "unsupported VDP domain state version"
        );
        ensure!(reader.read_bool()? == pal, "VDP domain region differs");
        let half = reader.read_u64()?;
        let mode128 = reader.read_bool()?;
        let clocks = ClockCone::read_state(reader)?;
        let hv = HvCone::read_state(reader, pal)?;
        let irq = IrqCone::read_state(reader)?;
        let status = status::Status::read_state(reader)?;
        let counter = Counter::read_state(reader)?;
        let control = ControlCore::read_state(reader)?;
        let frontend = frontend::Frontend::read_state(reader, &control)?;
        let wiring = wiring::Wiring::read_state(reader)?;
        let profile = input::profile(control.registers().logical(), mode128)?;
        let ram = ram::Ram::read_state(reader, profile.memory)?;
        let internal = internal_ram::InternalRam::read_state(reader, profile)?;
        let render = R::read_state(reader, pal)?;
        let mut old_dclk = [false; 3];
        let mut feedback = [false; 3];
        let mut levels = [false; 15];
        for value in old_dclk.iter_mut().chain(&mut feedback).chain(&mut levels) {
            *value = reader.read_bool()?;
        }
        let zclk_feedback = reader.read_bool()?;
        let board_clocks = [reader.read_bool()?, reader.read_bool()?];
        let render_dirty = reader.read_bool()?;
        let input_cache = PinCache {
            levels,
            ad: reader.read_u8()?,
            rd: reader.read_u8()?,
            sd: reader.read_u8()?,
        };
        let shared = SharedBus {
            address: reader.read_u32()?,
            data: reader.read_u16()?,
        };
        let ram_bus = ram::BusInputs {
            ad: reader.read_u8()?,
            rd: reader.read_u8()?,
        };
        let io_data = reader.read_u16()?;
        let memory_active = reader.read_bool()?;
        let fifo_full = reader.read_bool()?;
        let value = Self {
            pal,
            half,
            mode128,
            clocks,
            hv,
            irq,
            status,
            counter,
            control,
            frontend,
            wiring,
            ram,
            internal,
            render,
            old_dclk,
            feedback,
            zclk_feedback,
            board_clocks,
            render_dirty,
            input_cache,
            shared,
            ram_bus,
            io_data,
            memory_active,
            fifo_full,
            pending: None,
        };
        value.validate_state()?;
        Ok(value)
    }

    pub fn validate_state(&self) -> Result<()> {
        ensure!(
            self.half <= u64::MAX - 3,
            "invalid VDP domain phase frontier"
        );
        self.clocks.validate_profile(self.pal)?;
        let profile = input::profile(self.registers(), self.mode128)?;
        let (entering_m5, _, live_m5) = self.hv.mode_samples();
        ensure!(
            !self.mode128 || entering_m5,
            "128K sample lacks entering M5"
        );
        ensure!(
            live_m5 == (self.registers()[1] & 4 != 0),
            "stale live VDP mode"
        );
        self.frontend.validate_state(&self.control)?;
        ensure!(
            self.frontend.launched_write().is_none(),
            "duplicate logical VDP write retirement"
        );
        self.wiring.validate_state()?;
        self.ram.validate_state(profile.memory)?;
        self.internal.validate_state(profile)?;
        self.render.validate_state(self.registers())?;
        self.internal.validate_video_source(
            self.render.geometry(),
            self.render.palette_signals().display_active,
        )?;
        self.render
            .validate_response(self.ram.pins(), self.half == 0)?;
        if self.half != 0 {
            let geometry = self.render.geometry();
            ensure!(
                (geometry.horizontal, geometry.vertical) == self.hv.counters(),
                "stale VDP render geometry"
            );
            ensure!(
                geometry.height == self.hv.sampled_height(),
                "stale sampled render height"
            );
            ensure!(
                self.internal.visible_gate() == self.hv.visible_gate(),
                "stale VDP RGB blanking gate"
            );
        }
        self.shared.validate()?;
        let levels = self.view(0)?.drives.levels;
        if levels.status_read {
            ensure!(
                self.io_data & 0x3ff == self.status_word(),
                "stale VDP status bus output"
            );
        }
        if levels.data_read {
            ensure!(
                self.io_data == self.frontend.read_word(),
                "stale VDP read bus output"
            );
        }
        if levels.hv_read {
            ensure!(
                self.io_data == self.counter.drive(true, true).word,
                "stale VDP counter bus output"
            );
        }
        ensure!(
            !(self.input_cache.levels[0] && self.input_cache.levels[1])
                && self.input_cache.levels[14],
            "invalid VDP primitive input cache"
        );
        if let Some(receipt) = &self.pending {
            ensure!(
                receipt.half == self.half && receipt.effects.len() <= storage::MAX_EFFECTS,
                "invalid pending VDP physical effects"
            );
            ensure!(
                receipt.pixel == self.internal.pixel(),
                "stale physical pixel receipt"
            );
        }
        if self.half == 0 {
            ensure!(
                *self == Self::software_origin(self.pal),
                "VDP origin state differs"
            );
        }
        Ok(())
    }
}

impl<R: RenderBus> Domain<R> {
    pub fn write_render_tail(&self, writer: &mut StateWriter) -> Result<()> {
        self.render.write_state_tail(writer)
    }

    pub fn read_render_tail(&mut self, reader: &mut StateReader<'_>) -> Result<()> {
        self.render.read_state_tail(reader)
    }
}
