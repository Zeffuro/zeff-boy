use super::*;

impl Board {
    // The enclosing half-step discards detached candidates on failure.
    pub(crate) fn resolve_inner(&mut self, view: BoardView) -> Result<BoardInput> {
        self.resolve_inner_physical(view, &mut ())
    }
    pub(crate) fn resolve_inner_physical<H: HalfCapture>(
        &mut self,
        view: BoardView,
        physical: &mut H,
    ) -> Result<BoardInput> {
        ensure!(
            self.stage.is_none() && view.pass == self.next_pass,
            "MAIN board pass order"
        );
        let expected_half = if self.next_pass == 0 {
            self.half.checked_add(1)
        } else {
            Some(self.half)
        };
        ensure!(
            Some(view.half) == expected_half,
            "MAIN board clock discontinuity"
        );
        let companion = self
            .companion
            .ok_or_else(|| anyhow::anyhow!("unqualified SOUND companion interval"))?;
        ensure!(
            companion.half == view.half && companion.pass == view.pass,
            "stale SOUND companion interval"
        );
        self.sound_resolve(companion)?;
        let before = self
            .main
            .as_ref()
            .map_or_else(Self::idle_pins, main_bus::MainBus::pins);
        let released = self.grant.tristate() && !before.address_strobe;
        let (as_level, upper, lower, read) = if released {
            (
                self.outputs.strobe_direction || self.outputs.address_strobe_level,
                if self.outputs.strobe_direction {
                    self.held_controls[1]
                } else {
                    self.outputs.upper_level
                },
                if self.outputs.strobe_direction {
                    self.held_controls[2]
                } else {
                    self.outputs.lower_level
                },
                if self.outputs.read_direction {
                    self.held_controls[0]
                } else {
                    !companion.pins.write
                },
            )
        } else {
            (
                !before.address_strobe,
                !before.upper_strobe,
                !before.lower_strobe,
                before.read,
            )
        };
        physical.vdp(VdpEvent::Resolve(VdpMux {
            released,
            held_strobes: self.outputs.strobe_direction,
            held_read: self.outputs.read_direction,
            source: VdpSource::Sound {
                logical: self.sound.address,
                bank: companion.bank,
                write: companion.pins.shared() && companion.pins.write,
            },
        }));
        #[cfg(test)]
        physical.vdp_raw(VdpRaw::Resolve {
            released,
            held_strobes: self.outputs.strobe_direction,
            held_read: self.outputs.read_direction,
            pins: VdpPins {
                address: self.address,
                word: self.data,
                controls: [read, !as_level, !upper, !lower],
            },
        });
        self.held_controls = [read, upper, lower];
        let phase = self
            .main
            .as_ref()
            .map_or(main_bus::Phase::Idle, |bus| bus.snapshot().phase);
        if view.vclk != self.clock_high {
            self.grant.edge(
                view.vclk,
                phase,
                !self.outputs.request_level,
                !self.outputs.grant_acknowledge_level,
            );
        }
        if let Some(bus) = &mut self.main {
            if bus.snapshot().phase == main_bus::Phase::S6
                && self.protocol.is_some()
                && !matches!(
                    bus.snapshot()
                        .pending
                        .map(|pending| pending.request.operation),
                    Some(main_bus::Operation::Write(_))
                )
                && view.vclk != self.clock_high
                && !view.vclk
            {
                ensure!(
                    self.protocol_seen,
                    "MAIN protocol read was not physically driven before sampling"
                );
            }
            let required =
                bus.snapshot()
                    .pending
                    .map_or(0, |pending| match pending.request.access.lanes {
                        ByteLanes::Upper => 0xff00,
                        ByteLanes::Lower => 0x00ff,
                        ByteLanes::Both => 0xffff,
                    });
            let before_sample = bus.snapshot();
            bus.advance(main_bus::Sample {
                half: view.half,
                clock_high: view.vclk,
                dtack: !self.outputs.acknowledge_level,
                vpa: !self.outputs.vpa_level,
                berr: false,
                read_word: (self.data_mask & required == required).then_some(self.data),
            })
            .map_err(|error| anyhow::anyhow!("MAIN physical phase: {error:?}"))?;
            if before_sample.phase == main_bus::Phase::S6
                && bus.snapshot().phase == main_bus::Phase::S7
            {
                self.instruction_origin.0.freeze(
                    before_sample.pending,
                    self.data,
                    self.data_mask,
                    self.protocol.is_none(),
                );
            }
        } else if view.vclk {
            ensure!(!self.clock_high, "MAIN first rise was not observed");
            self.main = Some(
                main_bus::MainBus::at_origin(view.half, FunctionCode::SupervisorProgram)
                    .map_err(|error| anyhow::anyhow!("MAIN origin: {error:?}"))?,
            );
        }
        self.clock_high = view.vclk;
        let pins = self
            .main
            .as_ref()
            .map_or_else(Self::idle_pins, main_bus::MainBus::pins);
        // Retain driven terminal levels before grant ownership floats the controls.
        if before.address_strobe && !pins.address_strobe {
            self.held_controls = [pins.read, !pins.upper_strobe, !pins.lower_strobe];
            physical.vdp(VdpEvent::Terminal);
            #[cfg(test)]
            physical.vdp_raw(VdpRaw::Terminal);
        }
        let fc = pins.function_code as u8;
        let input = BoardInput {
            main: wiring::BoardPins {
                address: self.address,
                data: self.data,
                function_code: pins.function_code,
                address_strobe: !as_level,
                read,
                upper_strobe: !upper,
                lower_strobe: !lower,
                interrupt_acknowledge: pins.interrupt_acknowledge,
                sound_vdp_request: false,
                test_mode: false,
            },
            bus_grant: self.grant.granted(),
            bus_grant_acknowledge_level: self.outputs.grant_acknowledge_level,
            data_acknowledge_level: self.outputs.acknowledge_level,
            reset_released: true,
        };
        self.stage = Some(Stage {
            preview: view,
            pins,
            arbiter: arbiter::Inputs {
                vclk: view.vclk,
                zclk: view.zclk,
                data: self.data,
                address: self.address >> 1,
                bgack: self.outputs.grant_acknowledge_level,
                bg: !self.grant.granted(),
                read,
                upper,
                address_strobe: as_level,
                dtack: self.outputs.acknowledge_level,
                lower,
                cas: !view.drives.levels.column_strobe,
                output_enable: !view.drives.levels.output_enable,
                fc: [fc & 1 != 0, fc & 2 != 0],
                wait_level: self.arbiter.wait_level,
                sound_bus_acknowledge_level: !companion.pins.bus_ack,
                sound_address: self.sound.address,
                sound_bank: companion.bank,
                sound_read_level: !companion.pins.read,
                sound_write_level: !companion.pins.write,
                sound_memory_level: !companion.pins.mreq,
                sound_io_level: !companion.pins.iorq,
                m1: !companion.pins.m1,
                m3: true,
                delayed: [false; 8],
            },
            data_mask: self.data_mask,
            companion,
        });
        self.half = view.half;
        Ok(input)
    }
}
