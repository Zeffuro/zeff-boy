use super::domain::{RenderBus, RenderContext, RenderGeometry, SharedBus, Storage};
use super::{internal_ram, ram, status};
use super::{render_planes::Planes, render_serial::SerialPort, render_timing::RenderTiming};
use super::{render_sprite_control::SpriteControl, render_sprite_raster::SpriteRaster};
use crate::hardware::vdp::control_core::{PhaseLevels, RegisterStage};
use crate::hardware::vdp::state::{StateReader, StateWriter};
use anyhow::{Result, ensure};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::hardware::vdp) struct Renderer {
    pal: bool,
    timing: RenderTiming,
    planes: Planes,
    control: SpriteControl,
    raster: SpriteRaster,
    serial: SerialPort,
    serial_latch: u8,
    sprites_pending: bool,
    geometry: RenderGeometry,
}

impl Renderer {
    pub(super) fn classify_retained_fields(&self) {
        let Self {
            pal: _,
            timing: _,
            planes: _,
            control: _,
            raster: _,
            serial: _,
            serial_latch: _,
            sprites_pending: _,
            geometry: _,
        } = self;
    }
}

impl RenderBus for Renderer {
    fn software_origin(pal: bool) -> Self {
        Self {
            pal,
            timing: RenderTiming::software_origin(),
            planes: Planes::software_origin(),
            control: SpriteControl::software_origin(),
            raster: SpriteRaster::software_origin(),
            serial: SerialPort::software_origin(),
            serial_latch: 0,
            sprites_pending: false,
            geometry: RenderGeometry {
                mode5: true,
                height: 224,
                horizontal: 0,
                vertical: 0,
                horizontal_active: false,
                vertical_active: false,
                display_enabled: false,
                wide: false,
            },
        }
    }
    fn begin_descriptor_edge(&mut self, horizontal_master: bool) -> Result<()> {
        self.control.begin_descriptor_edge(horizontal_master)
    }
    fn finish_descriptor_edge(&mut self) -> Result<bool> {
        self.control.finish_descriptor_edge()
    }
    fn begin_sprite_half(
        &mut self,
        phases: PhaseLevels,
        mode5: bool,
        horizontal: u16,
    ) -> Result<()> {
        self.raster
            .begin_line_edge(phases, mode5, horizontal, self.timing)
    }
    fn finish_sprite_half(&mut self) -> Result<bool> {
        self.raster.finish_line_edge()
    }
    fn write_state_tail(&self, writer: &mut StateWriter) -> Result<()> {
        self.raster.write_line_tail(writer)
    }
    fn read_state_tail(&mut self, reader: &mut StateReader<'_>) -> Result<()> {
        self.raster.read_line_tail(reader)
    }
    fn serial_input(&self) -> u8 {
        self.serial.input()
    }
    fn geometry(&self) -> RenderGeometry {
        self.geometry
    }
    fn sprite_signals(&self) -> status::SpriteSignals {
        status::SpriteSignals {
            overflow: self.control.overflow_output(),
            collision: self.raster.collision(),
        }
    }
    fn palette_signals(&self) -> internal_ram::PaletteSignals {
        let sprite = self.raster.early_pixel();
        internal_ram::PaletteSignals {
            planes: [self.planes.pixel(0), self.planes.pixel(1)],
            sprite: self.raster.pixel(),
            priorities: [
                self.planes.priority(0),
                self.raster.priority(),
                self.planes.priority(1),
            ],
            nonzero: [
                self.planes.nonzero(0),
                sprite & 15 != 0,
                self.planes.nonzero(1),
            ],
            sprite_palette_three: (sprite >> 4) & 3 == 3,
            sprite_index_fourteen: sprite & 15 == 14,
            sprite_index_fifteen: sprite & 15 == 15,
            display_active: self.timing.display_active(),
        }
    }
    fn planes(
        &mut self,
        c: RenderContext,
        vsram: internal_ram::PlanesOutput,
        bus: SharedBus,
        _storage: &impl Storage,
    ) -> Result<SharedBus> {
        ensure!(!self.sprites_pending, "sprite stage has not completed");
        let registers = c.registers.at(RegisterStage::Control);
        validate_profile(registers)?;
        bus.validate()?;
        self.geometry = RenderGeometry {
            mode5: registers[1] & 4 != 0,
            height: c.height,
            horizontal: c.horizontal,
            vertical: c.vertical,
            horizontal_active: self.timing.horizontal_active(),
            vertical_active: c.vertical_display_before_hv,
            display_enabled: registers[1] & 64 != 0,
            wide: registers[12] & 1 != 0,
        };
        self.timing.evaluate(c);
        let bus = self
            .planes
            .clock(c, self.timing, vsram, bus, self.serial_latch);
        self.sprites_pending = true;
        Ok(bus)
    }
    fn sprites(
        &mut self,
        c: RenderContext,
        bus: SharedBus,
        _storage: &impl Storage,
    ) -> Result<SharedBus> {
        ensure!(self.sprites_pending, "sprite stage lacks its plane stage");
        let bus = self.control.clock(
            c,
            self.timing,
            bus,
            &mut self.serial_latch,
            self.serial.input(),
        )?;
        self.raster
            .clock(c, self.timing, self.control.raster(), self.serial_latch)?;
        self.sprites_pending = false;
        Ok(bus)
    }
    fn respond(
        &mut self,
        pins: ram::Pins,
        response: ram::Response,
        storage: &impl Storage,
    ) -> Result<()> {
        ensure!(
            !self.sprites_pending,
            "serial response during unfinished render stage"
        );
        self.serial.respond(pins, response, storage)
    }
    fn write_state(&self, w: &mut StateWriter) {
        w.write_u8(3);
        w.write_bool(self.pal);
        self.timing.write_state(w);
        self.planes.write_state(w);
        self.control.write_state(w);
        self.raster.write_state(w);
        self.serial.write_state(w);
        w.write_u8(self.serial_latch);
        w.write_bool(self.sprites_pending);
        w.write_u16(self.geometry.horizontal);
        w.write_u16(self.geometry.vertical);
        w.write_u16(self.geometry.height);
        for v in [
            self.geometry.horizontal_active,
            self.geometry.vertical_active,
            self.geometry.display_enabled,
            self.geometry.wide,
            self.geometry.mode5,
        ] {
            w.write_bool(v);
        }
    }
    fn read_state(r: &mut StateReader<'_>, pal: bool) -> Result<Self> {
        ensure!(r.read_u8()? == 3, "unsupported concrete render state");
        ensure!(r.read_bool()? == pal, "render region mismatch");
        let mut state = Self {
            pal,
            timing: RenderTiming::read_state(r)?,
            planes: Planes::read_state(r)?,
            control: SpriteControl::read_state(r)?,
            raster: SpriteRaster::read_state(r)?,
            serial: SerialPort::read_state(r)?,
            serial_latch: r.read_u8()?,
            sprites_pending: r.read_bool()?,
            geometry: RenderGeometry {
                mode5: true,
                height: 224,
                horizontal: 0,
                vertical: 0,
                horizontal_active: false,
                vertical_active: false,
                display_enabled: false,
                wide: false,
            },
        };
        state.geometry = RenderGeometry {
            horizontal: r.read_u16()?,
            vertical: r.read_u16()?,
            height: r.read_u16()?,
            horizontal_active: r.read_bool()?,
            vertical_active: r.read_bool()?,
            display_enabled: r.read_bool()?,
            wide: r.read_bool()?,
            mode5: r.read_bool()?,
        };
        ensure!(
            !state.sprites_pending,
            "incomplete render stage in board state"
        );
        state.validate_geometry()?;
        Ok(state)
    }
    fn validate_state(&self, registers: [u8; 24]) -> Result<()> {
        ensure!(!self.sprites_pending, "incomplete render stage");
        self.control.validate_descriptor_frontier()?;
        self.raster.validate_line_frontier()?;
        self.validate_geometry()?;
        ensure!(
            self.geometry.wide == (registers[12] & 1 != 0)
                && self.geometry.mode5 == (registers[1] & 4 != 0)
                && self.geometry.display_enabled == (registers[1] & 64 != 0),
            "render register provenance mismatch"
        );
        validate_profile(registers)
    }
    fn validate_response(&self, pins: ram::Pins, origin: bool) -> Result<()> {
        self.serial.validate_response(pins, origin)
    }
}

impl Renderer {
    fn validate_geometry(&self) -> Result<()> {
        ensure!(
            self.geometry.horizontal < 512
                && self.geometry.vertical < 512
                && matches!(
                    (self.geometry.mode5, self.geometry.height),
                    (false, 192) | (true, 224 | 240)
                )
                && self.timing.display_active()
                    == (self.geometry.horizontal_active
                        && self.geometry.vertical_active
                        && self.geometry.display_enabled),
            "invalid render geometry provenance"
        );
        Ok(())
    }
}

fn validate_profile(registers: [u8; 24]) -> Result<()> {
    ensure!(
        registers[12] & 6 == 0 && registers[0] & 1 == 0,
        "unsupported concrete render profile"
    );
    Ok(())
}

#[cfg(test)]
impl Renderer {
    pub(super) fn fixture_raster(&mut self) -> &mut SpriteRaster {
        &mut self.raster
    }
}
