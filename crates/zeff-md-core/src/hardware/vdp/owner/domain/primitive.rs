use super::*;

pub(super) struct Input {
    pub board: BoardInput,
    pub levels: [bool; 3],
    pub pass: u8,
    pub ordinal: u8,
}
impl<R: RenderBus> Domain<R> {
    pub(super) fn primitive<B: BoardBoundary>(
        &mut self,
        input: Input,
        boundary: &mut B,
        storage: &impl Storage,
        effects: &mut Vec<Effect>,
        dma_activity: &mut bool,
    ) -> Result<()> {
        let Input {
            board,
            levels: [d1, d2, cpu_clock1],
            pass,
            ordinal,
        } = input;
        self.clocks.eval_dclk(d1, d2);
        let phases = PhaseLevels {
            data_master: d1,
            data_slave: d2,
            horizontal_master: self.clocks.h1(),
            horizontal_slave: self.clocks.h2(),
        };
        let before = self.registers();
        self.mode128 = before[1] & 0x84 == 0x84;
        self.hv.sample_mode(before[1] & 4 != 0, before[1] & 8 != 0);
        input::profile(before, self.mode128)?;
        let old_hv = self.hv.irq_inputs();
        let old_vint = self.irq.vp;
        let sprite_signals = self.render.sprite_signals();
        let old_status = self.view(0)?.drives.levels.status_read;
        self.irq.eval(
            d1,
            d2,
            phases.horizontal_master,
            phases.horizontal_slave,
            board.main.address_strobe && board.main.interrupt_acknowledge,
            before[1] & 4 != 0,
            old_status,
            before[0] & 16 != 0,
            before[1] & 32 != 0,
            self.control.registers().hint(),
            old_hv,
        );
        self.status.sample(old_vint, sprite_signals, self.irq.clear);
        let (opportunity, data_window) = self.hv.memory_gates();
        let early = self.wiring.early(wiring::Inputs {
            phases,
            cpu_clock1,
            refresh: self.hv.refresh(),
            memory_beat: data_window,
            fifo_full: self.frontend.fifo_full(false),
            old_external: self.frontend.external_feedback(),
            handshake: self.control.handshake(),
            register11_before: before[11],
            reset_comb: false,
        })?;
        #[cfg(test)]
        if !board.bus_grant_acknowledge_level
            && self.frontend.external_feedback().bus_grant_acknowledge
            && before[23] >> 6 == 0
            && let Some(source) = early.dma_pressure
        {
            boundary.observe_dma_pressure(self.half, pass, source);
        }
        let output = self.frontend.evaluate(
            &mut self.control,
            frontend::Inputs {
                phases,
                main: early.main,
                upper_strobe: early.upper_strobe,
                lower_strobe: early.lower_strobe,
                hv_read: early.hv_read,
                reset_comb: false,
                external: frontend::ExternalPins {
                    cpu_clock1,
                    address_strobe: board.main.address_strobe,
                    bus_grant: board.bus_grant,
                    bus_grant_acknowledge_level: board.bus_grant_acknowledge_level,
                    data_acknowledge_level: board.data_acknowledge_level,
                },
                external_address_cycle: early.external_address_cycle,
                memory: frontend::MemoryInputs {
                    opportunity,
                    data_window,
                    address: self.shared.address,
                    data: self.shared.data,
                },
            },
        )?;
        input::profile(
            output.control.registers.at(RegisterStage::Control),
            self.mode128,
        )?;
        let _ = pass;
        if B::VDP_CAPTURE {
            let receipt = crate::sound_trace::physical::vdp::Primitive {
                ordinal,
                address: early.address,
                bus_address: board.main.address,
                word: early.main.data_bus,
                lanes: [early.upper_strobe, early.lower_strobe],
                main: [early.main.write, early.main.read, early.main.control_select],
                intake: [
                    output.control.intake.data_write,
                    output.control.intake.first,
                    output.control.intake.second,
                ],
                ack: [
                    output.data_ack,
                    output.control.intake.control_ack,
                    output.read_ack,
                ],
                psg: [early.psg_acknowledge, early.psg_write],
                load: output.bus_load,
            };
            boundary.vdp(crate::sound_trace::physical::vdp::Event::Primitive(receipt));
        }
        #[cfg(test)]
        boundary.vdp_raw(crate::sound_trace::physical::vdp::raw::Event::Primitive(
            crate::sound_trace::physical::vdp::Primitive {
                ordinal,
                address: early.address,
                bus_address: board.main.address,
                word: early.main.data_bus,
                lanes: [early.upper_strobe, early.lower_strobe],
                main: [early.main.write, early.main.read, early.main.control_select],
                intake: [
                    output.control.intake.data_write,
                    output.control.intake.first,
                    output.control.intake.second,
                ],
                ack: [
                    output.data_ack,
                    output.control.intake.control_ack,
                    output.read_ack,
                ],
                psg: [early.psg_acknowledge, early.psg_write],
                load: output.bus_load,
            },
        ));
        *dma_activity |= output.dma_activity;
        self.fifo_full = output.fifo_full;
        self.wiring.finish(early, &output)?;
        self.shared = SharedBus {
            address: output.memory.address,
            data: output.memory.data,
        };
        self.counter.evaluate(
            output.control.registers.at(RegisterStage::Control),
            old_hv.h as u16,
            old_hv.v as u16,
            [false; 2],
            false,
        )?;
        let memory_active_before_hv = self.memory_active;
        self.hv.eval(
            phases.horizontal_master,
            phases.horizontal_slave,
            output.control.registers.at(RegisterStage::Control),
            self.memory_active,
        );
        let vertical_display_before_hv = self.irq.active;
        let vertical_active = self.irq.post_hv(
            phases.horizontal_master,
            phases.horizontal_slave,
            old_hv,
            self.hv.irq_inputs().height,
        );
        self.memory_active =
            output.control.registers.at(RegisterStage::Control)[1] & 64 != 0 && vertical_active;
        let context = RenderContext {
            mode128: self.mode128,
            height: self.hv.sampled_height(),
            phases,
            registers: output.control.registers,
            before_hv: old_hv,
            memory_active_before_hv,
            frontend: output.memory,
            horizontal: self.hv.counters().0,
            vertical: self.hv.counters().1,
            vertical_active,
            vertical_display_before_hv,
            vertical_display: self.irq.active,
            vertical_last: self.irq.vertical_last[1],
            memory_active: self.memory_active,
            input_bus: self.shared,
        };
        let planes = self.internal.clock_planes(
            internal_ram::PlanesInputs {
                profile: input::profile(
                    output.control.registers.at(RegisterStage::ScrollY),
                    self.mode128,
                )?,
                phases,
                frontend: output.memory,
                horizontal_counter: context.horizontal,
            },
            |index| Overlay { storage, effects }.vsram(index),
        )?;
        if let Some(update) = planes.bus {
            self.shared.data = update.apply(self.shared.data);
        }
        self.shared =
            self.render
                .planes(context, planes, self.shared, &Overlay { storage, effects })?;
        if let Some(write) = planes.write {
            effects.push(Effect::Internal(write));
        }
        self.shared = self
            .render
            .sprites(context, self.shared, &Overlay { storage, effects })?;
        self.shared.validate()?;
        let mut memory = output.memory;
        memory.address = self.shared.address;
        memory.data = self.shared.data;
        let (opportunity, memory_window) = self.hv.memory_gates();
        let ram = self.ram.clock(ram::Inputs {
            phases,
            profile: input::profile(
                output.control.registers.at(RegisterStage::Sprite),
                self.mode128,
            )?
            .memory,
            frontend: memory,
            opportunity,
            memory_window,
            refresh: self.hv.refresh(),
            copy_read_address_selected: self.frontend.copy_read_address_selected(output.dma),
            bus: self.ram_bus,
        })?;
        if let Some(data) = ram.returned_data {
            self.shared.data = data;
        }
        memory.data = self.shared.data;
        let video = self.internal.clock_video(
            internal_ram::VideoInputs {
                profile: input::profile(
                    output.control.registers.at(RegisterStage::Backdrop),
                    self.mode128,
                )?,
                phases,
                frontend: memory,
                palette: self.render.palette_signals(),
                visible_gate: self.hv.visible_gate(),
                geometry: self.render.geometry(),
            },
            |index| Overlay { storage, effects }.cram(index),
        )?;
        if let Some(update) = video.bus {
            self.shared.data = update.apply(self.shared.data);
        }
        if let Some(write) = video.write {
            effects.push(Effect::Internal(write));
        }
        ensure!(
            effects.len() <= storage::MAX_EFFECTS,
            "VDP half-cycle effect capacity"
        );
        Ok(())
    }
}

#[cfg(test)]
impl<R: RenderBus> Domain<R> {
    pub(in crate::hardware::vdp::owner) fn with_render_half_schedule(
        render: R,
        mut observe: impl FnMut(&mut R, u64, bool),
    ) -> Result<[bool; 3]> {
        let mut domain = Self::software_origin(false);
        domain.render = render;
        let mut dirty = [false; 3];
        for half in 2..=4 {
            domain.half = half;
            domain.render_dirty = false;
            domain.input_cache.levels[0] = half < 4;
            domain.input_cache.levels[1] = half == 4;
            let descriptor_sample = domain.begin_render_half()?;
            observe(&mut domain.render, half, false);
            domain.finish_render_half(descriptor_sample)?;
            observe(&mut domain.render, half, true);
            dirty[(half - 2) as usize] = domain.render_dirty;
        }
        Ok(dirty)
    }
}
