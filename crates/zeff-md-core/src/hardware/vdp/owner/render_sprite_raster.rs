#[cfg(test)]
mod contract;
mod line_access;
mod line_port;
mod state_tail;

use super::render_timing::RenderTiming;
use super::{domain::RenderContext, internal_ram::PalettePixel};
use super::{render_sprite_control::RasterControl, render_sprite_pixels::SpritePixels};
use crate::hardware::vdp::control_core::RegisterStage;
use crate::hardware::vdp::state::{StateReader, StateWriter};
use anyhow::{Result, ensure};
use line_port::LinePort;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Position {
    reload: bool,
    idle: bool,
    origin: u8,
    column: u8,
    flip: bool,
    valid: bool,
    inverse_idle: bool,
    previous_inverse_idle: bool,
    writing: bool,
    offset: u8,
    address: u8,
}

impl Position {
    fn write(self, w: &mut StateWriter) {
        for v in [
            self.reload,
            self.idle,
            self.flip,
            self.valid,
            self.inverse_idle,
            self.previous_inverse_idle,
            self.writing,
        ] {
            w.write_bool(v);
        }
        for v in [self.origin, self.column, self.offset, self.address] {
            w.write_u8(v);
        }
    }
    fn read(r: &mut StateReader<'_>) -> Result<Self> {
        let reload = r.read_bool()?;
        let idle = r.read_bool()?;
        let flip = r.read_bool()?;
        let valid = r.read_bool()?;
        let inverse_idle = r.read_bool()?;
        let previous_inverse_idle = r.read_bool()?;
        let writing = r.read_bool()?;
        let origin = r.read_u8()?;
        let column = r.read_u8()?;
        let offset = r.read_u8()?;
        let address = r.read_u8()?;
        ensure!(
            origin < 64 && column < 64 && offset < 8 && address < 64,
            "invalid sprite line position"
        );
        Ok(Self {
            reload,
            idle,
            origin,
            column,
            flip,
            valid,
            inverse_idle,
            previous_inverse_idle,
            writing,
            offset,
            address,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct SpriteRaster {
    descriptors: [u16; 11],
    width_counter: [u8; 2],
    horizontal: [u16; 2],
    display: [u8; 2],
    read_delay: [bool; 2],
    read_latch: bool,
    positions: [Position; 2],
    idle: bool,
    flip: bool,
    attributes: u8,
    active_attributes: u8,
    offset: u8,
    edge_split: bool,
    write_disabled: bool,
    clear: bool,
    thresholds: [bool; 7],
    writes: [bool; 8],
    pixels: SpritePixels,
    lines: [[u8; 8]; 40],
    output: [u8; 8],
    port: LinePort,
    shifters: [[u8; 8]; 2],
    colors: [[u8; 3]; 2],
    priority: bool,
    collision: [bool; 2],
}

impl SpriteRaster {
    pub fn software_origin() -> Self {
        Self {
            descriptors: [0; 11],
            width_counter: [0; 2],
            horizontal: [0; 2],
            display: [0; 2],
            read_delay: [false; 2],
            read_latch: false,
            positions: [Position::default(); 2],
            idle: false,
            flip: false,
            attributes: 0,
            active_attributes: 0,
            offset: 0,
            edge_split: false,
            write_disabled: false,
            clear: false,
            thresholds: [false; 7],
            writes: [false; 8],
            pixels: SpritePixels::default(),
            lines: [[0; 8]; 40],
            output: [0; 8],
            port: LinePort::legacy_unknown(),
            shifters: [[0; 8]; 2],
            colors: [[0; 3]; 2],
            priority: false,
            collision: [false; 2],
        }
    }
    pub fn collision(&self) -> bool {
        self.collision[1]
    }
    pub fn pixel(&self) -> PalettePixel {
        let v = self.colors[1][2];
        PalettePixel {
            index: v & 15,
            palette: (v >> 4) & 3,
            priority: v & 64 != 0,
        }
    }
    pub fn priority(&self) -> bool {
        self.priority
    }
    pub fn early_pixel(&self) -> u8 {
        self.colors[1][1]
    }

    pub fn clock(
        &mut self,
        c: RenderContext,
        t: RenderTiming,
        control: RasterControl,
        serial: u8,
    ) -> Result<()> {
        let p = c.phases;
        let regs = c.registers.at(RegisterStage::Control);
        let wide = regs[12] & 1 != 0;
        let m5 = regs[1] & 4 != 0;
        if p.horizontal_master {
            self.horizontal[0] = (c.horizontal + if m5 { 494 } else { 501 }) & 511;
            self.display[0] = ((self.display[1] << 1) | u8::from(t.memory_pixels())) & 3;
        }
        if p.horizontal_slave {
            self.horizontal[1] = self.horizontal[0];
            self.display[1] = self.display[0];
            self.read_delay[1] = self.read_delay[0];
        }
        if p.horizontal_master {
            for i in 0..5 {
                self.descriptors[2 * i + 1] = self.descriptors[2 * i];
            }
        }
        if p.horizontal_slave {
            for i in 1..6 {
                self.descriptors[2 * i] = self.descriptors[2 * i - 1];
            }
            let s = control.selected;
            self.descriptors[0] = s.x
                | u16::from(s.flip) << 9
                | u16::from(s.palette) << 10
                | u16::from(s.priority) << 12
                | u16::from(s.width) << 13;
        }
        let descriptor = self.descriptors[10];
        let flip = m5 && descriptor & 512 != 0;
        let x = if m5 {
            descriptor & 511
        } else {
            u16::from(control.mode4_x)
        };
        let width = ((descriptor >> 13) & 3) as u8;
        let new_word =
            control.fetch_pipe & 4 != 0 && self.width_counter[1] == 0 && p.horizontal_master;
        let writing =
            self.positions[1].valid && (!self.idle || self.positions[1].previous_inverse_idle);
        if p.data_master {
            let old = self.positions[1];
            let stepping = !(old.reload || old.idle);
            let origin = ((x >> 3)
                + u16::from(flip)
                + if flip { u16::from(width) } else { 0 }
                + if m5 {
                    48
                } else if regs[0] & 8 != 0 {
                    63
                } else {
                    0
                })
                & 63;
            let column = ((if old.reload { old.origin } else { old.column })
                + if stepping {
                    if old.flip { 63 } else { 1 }
                } else {
                    0
                })
                & 63;
            self.positions[0] = Position {
                reload: new_word,
                idle: self.idle,
                origin: origin as u8,
                column,
                flip: self.flip,
                valid: !(column & 32 != 0 && (!wide || column & 24 != 0)),
                inverse_idle: !self.idle,
                previous_inverse_idle: old.inverse_idle,
                writing,
                offset: self.offset,
                address: column,
            };
            self.edge_split = old.previous_inverse_idle ^ old.flip;
            self.write_disabled = !writing;
            self.clear = p.horizontal_slave && self.read_latch;
        }
        if p.data_slave {
            self.positions[1] = self.positions[0];
        }
        if new_word {
            self.flip = flip;
            self.attributes = ((descriptor >> 10) & 7) as u8;
        }
        if p.data_slave {
            if self.pixels.active() {
                self.active_attributes = self.attributes;
            }
            self.idle = !self.pixels.active();
        }
        if p.horizontal_master {
            let starting = control.fetch_pipe & 4 != 0 && self.width_counter[1] == 0;
            self.width_counter[0] = if t.scan_end() {
                0
            } else {
                ((if starting {
                    if m5 { width ^ 3 } else { 3 }
                } else {
                    self.width_counter[1]
                }) + u8::from(control.fetch_pipe & 4 != 0))
                    & 3
            };
        }
        if p.horizontal_slave {
            self.width_counter[1] = self.width_counter[0];
        }
        let displaying = if m5 {
            self.display[1] & 2 != 0
        } else {
            t.memory_pixels()
        };
        let read = displaying && self.horizontal[1] & 7 == 0;
        if p.horizontal_master {
            self.read_latch = read;
            self.read_delay[0] = read;
        }
        if new_word {
            self.offset = (x & 7) as u8;
        }
        self.pixels.clock(
            p,
            m5,
            control.ready,
            control.fetch_pipe & 1 != 0,
            t.memory_pixels(),
            flip,
            x,
            serial,
        );
        let incoming = self.pixels.row();
        if p.data_master {
            for (i, v) in self.thresholds.iter_mut().enumerate() {
                *v = usize::from(self.positions[1].offset) < i + 1;
            }
        }
        let enabled = self.line_pins(m5, t, p.data_master).enabled;
        (self.output, self.port.output_known) = self.port.bus(enabled).lanes();
        if p.data_slave {
            self.prepare_writes(incoming, m5, t, p)?;
        }
        if p.horizontal_master {
            for i in 0..8 {
                self.shifters[0][i] = if i == 7 { 0 } else { self.shifters[1][i + 1] };
                if read {
                    self.shifters[0][i] = self.load_output(i, self.shifters[0][i], m5, t, p)?;
                }
            }
        }
        if p.horizontal_slave {
            self.shifters[1] = self.shifters[0];
        }
        self.priority = m5 && self.colors[1][1] & 64 != 0;
        if p.horizontal_master {
            let first = self.shifters[1][0];
            let second = if m5 { self.colors[1][0] } else { first };
            let last = self.colors[1][1] & 15
                | if m5 { self.colors[1][1] & 48 } else { 16 }
                | u8::from(self.priority) << 6;
            self.colors[0] = [first, second, last];
        }
        if p.horizontal_slave {
            self.colors[1] = self.colors[0];
        }
        self.pixels.finish(p);
        if p.data_master {
            self.collision[1] = self.collision[0];
        }
        Ok(())
    }

    pub fn write_state(&self, w: &mut StateWriter) {
        w.write_u8(1);
        for v in self.descriptors {
            w.write_u16(v);
        }
        w.write_bytes(&self.width_counter);
        for v in self.horizontal {
            w.write_u16(v);
        }
        w.write_bytes(&self.display);
        for v in self.read_delay {
            w.write_bool(v);
        }
        w.write_bool(self.read_latch);
        for v in self.positions {
            v.write(w);
        }
        w.write_bool(self.idle);
        w.write_bool(self.flip);
        w.write_u8(self.attributes);
        w.write_u8(self.active_attributes);
        w.write_u8(self.offset);
        for v in [self.edge_split, self.write_disabled, self.clear] {
            w.write_bool(v);
        }
        for v in self.thresholds {
            w.write_bool(v);
        }
        for v in self.writes {
            w.write_bool(v);
        }
        self.pixels.write_state(w);
        for row in self.lines {
            w.write_bytes(&row);
        }
        w.write_bytes(&self.output);
        for row in self.shifters {
            w.write_bytes(&row);
        }
        for row in self.colors {
            w.write_bytes(&row);
        }
        w.write_bool(self.priority);
        for v in self.collision {
            w.write_bool(v);
        }
    }
    pub fn read_state(r: &mut StateReader<'_>) -> Result<Self> {
        ensure!(r.read_u8()? == 1, "unsupported sprite raster state");
        let mut s = Self::software_origin();
        for v in &mut s.descriptors {
            *v = r.read_u16()?;
        }
        r.read_exact(&mut s.width_counter)?;
        for v in &mut s.horizontal {
            *v = r.read_u16()?;
        }
        r.read_exact(&mut s.display)?;
        for v in &mut s.read_delay {
            *v = r.read_bool()?;
        }
        s.read_latch = r.read_bool()?;
        for v in &mut s.positions {
            *v = Position::read(r)?;
        }
        s.idle = r.read_bool()?;
        s.flip = r.read_bool()?;
        s.attributes = r.read_u8()?;
        s.active_attributes = r.read_u8()?;
        s.offset = r.read_u8()?;
        s.edge_split = r.read_bool()?;
        s.write_disabled = r.read_bool()?;
        s.clear = r.read_bool()?;
        for v in &mut s.thresholds {
            *v = r.read_bool()?;
        }
        for v in &mut s.writes {
            *v = r.read_bool()?;
        }
        s.pixels = SpritePixels::read_state(r)?;
        for row in &mut s.lines {
            r.read_exact(row)?;
        }
        r.read_exact(&mut s.output)?;
        for row in &mut s.shifters {
            r.read_exact(row)?;
        }
        for row in &mut s.colors {
            r.read_exact(row)?;
        }
        s.priority = r.read_bool()?;
        for v in &mut s.collision {
            *v = r.read_bool()?;
        }
        ensure!(
            s.descriptors.into_iter().all(|v| v < 32768)
                && s.width_counter.into_iter().all(|v| v < 4)
                && s.horizontal.into_iter().all(|v| v < 512)
                && s.display.into_iter().all(|v| v < 4)
                && s.attributes < 8
                && s.active_attributes < 8
                && s.offset < 8
                && s.lines
                    .iter()
                    .flatten()
                    .chain(s.output.iter())
                    .chain(s.shifters.iter().flatten())
                    .chain(s.colors.iter().flatten())
                    .all(|v| *v < 128),
            "invalid sprite raster captures"
        );
        Ok(s)
    }
}
