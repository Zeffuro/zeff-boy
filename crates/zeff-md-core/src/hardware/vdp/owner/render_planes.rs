use super::render_timing::{Event, RenderTiming};
use super::{
    domain::{RenderContext, SharedBus},
    internal_ram,
    render_capture::Capture,
};
use crate::hardware::vdp::control_core::RegisterStage;
use crate::hardware::vdp::state::{StateReader, StateWriter};
use anyhow::{Result, ensure};

macro_rules! cells {
    ($($field:ident: $mask:expr),+ $(,)?) => {
        #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
        struct Cells { $($field: [u16; 2],)+ }
        impl Cells {
            fn commit(&mut self) { $(self.$field[1] = self.$field[0];)+ }
            fn write(self, writer: &mut StateWriter) {
                $(for value in self.$field { writer.write_u16(value); })+
            }
            fn read(reader: &mut StateReader<'_>) -> Result<Self> {
                let mut cells = Self::default();
                $(for value in &mut cells.$field {
                    *value = reader.read_u16()?;
                    ensure!(*value & !$mask == 0, "invalid render plane cell");
                })+
                Ok(cells)
            }
        }
    }
}

cells! { select: 0xfff, names: 7, scroll: 7, fetch_a: 3, fetch_b: 3,
phase_a: 15, color_a: 0x7f7f, color_b: 0x7f7f }

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct Planes {
    cells: Cells,
    phase_b: [u8; 2],
    window: [u8; 2],
    vertical: [u16; 2],
    scroll: Capture,
    names: Capture,
    pixels: [Capture; 4],
    staged: [[[u8; 4]; 2]; 2],
    current: [[[u8; 4]; 2]; 2],
    staged_attributes: [u8; 2],
    captured_attributes_a: u8,
    attributes: [u8; 2],
    fine_a: u8,
    horizontal_scroll: u8,
    copy_a: bool,
    copy_b: bool,
}

impl Planes {
    pub fn software_origin() -> Self {
        Self::default()
    }

    pub fn pixel(&self, plane: usize) -> internal_ram::PalettePixel {
        let word = if plane == 0 {
            self.cells.color_a[1]
        } else {
            self.cells.color_b[1]
        };
        let delayed = (word >> 8) as u8;
        internal_ram::PalettePixel {
            index: delayed & 15,
            palette: (delayed >> 4) & 3,
            priority: delayed & 64 != 0,
        }
    }

    pub fn priority(&self, plane: usize) -> bool {
        (if plane == 0 {
            self.cells.color_a[1]
        } else {
            self.cells.color_b[1]
        }) & 64
            != 0
    }

    pub fn nonzero(&self, plane: usize) -> bool {
        (if plane == 0 {
            self.cells.color_a[1]
        } else {
            self.cells.color_b[1]
        }) & 15
            != 0
    }

    pub fn clock(
        &mut self,
        context: RenderContext,
        timing: RenderTiming,
        vsram: internal_ram::PlanesOutput,
        mut bus: SharedBus,
        serial: u8,
    ) -> SharedBus {
        let p = context.phases;
        let h = context.horizontal;
        let v = context.vertical;
        let early = context.registers.at(RegisterStage::Control);
        let wide = early[12] & 1 != 0;
        let m5 = early[1] & 4 != 0;
        let vertical_window =
            (((v >> 3) & 31) < u16::from(self.window[1] & 31)) ^ (self.window[1] & 128 != 0);
        let horizontal_window =
            (u16::from(self.window[0] & 31) <= ((h >> 4) & 31)) ^ (self.window[0] & 128 == 0);
        let window = m5 && (vertical_window || horizontal_window && h & 384 != 384) && h & 8 == 0;
        let reset_a = timing.gate(Event::PixelResetA);
        if p.horizontal_master {
            let old = self.cells;
            let select = [
                h & 8 != 0,
                !(h & 192 == 192 && early[0] & 128 != 0),
                timing.gate(Event::PatternA) || timing.gate(Event::PatternB),
                h & 8 != 0,
                timing.gate(Event::PatternA) || !m5 && timing.gate(Event::PatternB),
                m5 && timing.gate(Event::NameB),
                timing.gate(Event::NameA) && !window,
                timing.gate(Event::NameA) && window,
                window,
                m5 && timing.gate(Event::PatternB),
                reset_a,
                !(timing.gate(Event::PatternB) && !m5),
            ];
            self.cells.select[0] = select
                .into_iter()
                .enumerate()
                .fold(0, |bits, (index, value)| bits | (u16::from(value) << index));
            self.cells.names[0] = (old.names[1] << 1
                | u16::from(timing.gate(Event::NameA) || timing.gate(Event::NameB)))
                & 7;
            self.cells.scroll[0] = (old.scroll[1] << 1 | u16::from(timing.gate(Event::Scroll))) & 7;
            self.cells.fetch_a[0] =
                (old.fetch_a[1] << 1 | u16::from(timing.gate(Event::PixelsA))) & 3;
            self.cells.fetch_b[0] =
                (old.fetch_b[1] << 1 | u16::from(timing.gate(Event::PixelsB))) & 3;
            self.cells.color_a[0] =
                (old.color_a[1] & 127) << 8 | u16::from(self.sample(0, old.phase_a[1] as u8, m5));
            self.cells.color_b[0] =
                (old.color_b[1] & 127) << 8 | u16::from(self.sample(1, self.phase_b[1], true));
            self.copy_a = (!m5 || old.phase_a[1] & 8 != 0) && old.phase_a[1] & 7 == 7
                || reset_a && self.fine_a == 15;
            self.cells.phase_a[0] = (if reset_a {
                u16::from(self.fine_a)
            } else {
                old.phase_a[1]
            })
            .wrapping_add(1)
                & 15;
        }
        if p.horizontal_slave {
            self.cells.commit();
        }
        let select = self.cells.select[1];
        let selected = |bit: u32| select & (1_u16 << bit) != 0_u16;
        if p.horizontal_master && timing.gate(Event::VerticalStep) && (m5 || context.vertical_last)
        {
            self.vertical[0] = if m5 {
                vsram.before.out
            } else {
                u16::from(context.registers.at(RegisterStage::ScrollY)[9])
            };
        }
        let scroll_read = if early[11] & 4 != 0 {
            timing.gate(Event::Column)
        } else {
            timing.scan_end()
        };
        if p.horizontal_master && scroll_read {
            self.vertical[1] = vsram.before.out;
        }
        let offset = if m5 && (selected(0) || early[11] & 4 != 0) {
            self.vertical[1]
        } else if selected(1) {
            self.vertical[0]
        } else {
            self.vertical[0] & !255
        };
        let y = (offset + (v & 511)) & 2047;
        let scroll_word = if !m5 {
            if early[0] & 64 != 0 && v & 240 == 0 {
                0
            } else {
                u16::from(self.horizontal_scroll)
            }
        } else if window {
            0
        } else if h & 8 == 0 {
            u16::from(self.scroll.bytes[0]) | u16::from(self.scroll.bytes[1] & 3) << 8
        } else {
            u16::from(self.scroll.bytes[2]) | u16::from(self.scroll.bytes[3] & 3) << 8
        };
        let horizontal_offset = scroll_word ^ 1023;
        let geometry = context.registers.at(RegisterStage::PlaneGeometry);
        if selected(5) || selected(6) {
            let base = if selected(5) {
                geometry[4] & 15
            } else {
                (geometry[2] >> 3) & 15
            };
            let outside = if wide {
                h & 256 != 0 && h & 192 != 0
            } else {
                h & 256 != 0
            };
            let column = (h >> 4) & 15 | if outside { 15 } else { 0 };
            let hsize = geometry[16] & 3;
            let vsize = (geometry[16] >> 4) & 3;
            let upper = if outside {
                u16::from(hsize)
            } else {
                (h >> 8) & 1 | u16::from(h & 256 != 0 && h & 192 != 0) << 1
            };
            let scroll_x = (horizontal_offset >> 3) & 127;
            let x = (1
                + (scroll_x | u16::from(m5))
                + (u16::from(!outside && !m5 && h & 8 != 0) | column << 1 | upper << 5))
                & 127;
            let raw_y = (y >> 3) & 127;
            let wrapped_y = (raw_y & 3)
                | (((((raw_y >> 2) & 7)
                    + u16::from(!m5 && (raw_y & 28 == 28 || raw_y & 32 != 0)))
                    << 2)
                    & 28);
            let row = wrapped_y
                | if vsize & 1 != 0 { raw_y & 32 } else { 0 }
                | if vsize & 2 != 0 { raw_y & 64 } else { 0 };
            let address = match hsize {
                0 => row,
                1 => (row << 1) & 126 | (x >> 5) & 1,
                3 => (row << 2) & 124 | (x >> 5) & 3,
                _ => 0,
            };
            if m5 {
                bus.address = u32::from(base & 14) << 13;
            }
            let top = if m5 {
                (address >> 5) & 3
            } else {
                u16::from((geometry[2] >> 1) & 3)
            };
            bus.address = bus.address & !0x3fff
                | u32::from(base & 1) << 13
                | u32::from(top) << 11
                | u32::from(address & 31) << 6
                | u32::from(x & 31) << 1;
        }
        let old_window = context.registers.at(RegisterStage::PlaneGeometry);
        if self.cells.scroll[1] & 1 != 0 {
            let line = (v & 255)
                & if early[11] & 1 != 0 { 255 } else { !7 }
                & if early[11] & 2 != 0 { 255 } else { !248 };
            bus.address = u32::from(old_window[13] & 127) << 10 | u32::from(line) << 2;
        }
        if selected(7) {
            let base = (old_window[3] >> 1) & 63;
            let row = if wide {
                (h >> 8) & 1 | (v >> 2) & 62
            } else {
                (v >> 3) & 31 | u16::from(base & 1) << 5
            };
            bus.address =
                u32::from((h >> 4) & 15) << 2 | u32::from(row) << 6 | u32::from(base >> 1) << 12;
        }
        let window_regs = context.registers.at(RegisterStage::Window);
        if timing.gate(Event::VerticalStep) {
            self.window[0] = window_regs[17];
            if m5 || context.vertical_last {
                self.window[1] = window_regs[18];
            }
        }
        self.scroll.sample(p.data_slave, serial);
        self.scroll.bytes[1] &= 3;
        self.scroll.bytes[3] &= 3;
        if timing.gate(Event::VerticalStep) {
            self.horizontal_scroll = context.registers.at(RegisterStage::ScrollX)[8];
        }
        self.scroll.clock(p, self.cells.scroll[1] & 4 != 0, 3);
        self.names.sample(p.data_slave, serial);
        let names = self.names.bytes;
        let flip = if selected(9) {
            names[3] & 16 != 0
        } else {
            names[1] & if m5 { 16 } else { 4 } != 0
        };
        let row = (if selected(8) { v } else { y }) & 15 ^ if flip { 15 } else { 0 };
        if selected(2) {
            bus.address =
                bus.address & !31 | u32::from(row & 7) << 2 | u32::from(!selected(11)) << 1;
        }
        let bank = if selected(3) {
            window_regs[14] >> 4
        } else {
            window_regs[14]
        } & 1;
        if selected(4) {
            let tile = u16::from(names[0]) | u16::from(names[1] & 1) << 8;
            bus.address = bus.address & !0x3fe0 | u32::from(tile) << 5;
            if m5 {
                bus.address = bus.address & !0x1c000
                    | u32::from((names[1] >> 1) & 3) << 14
                    | u32::from(bank) << 16;
            }
        }
        if selected(9) {
            let tile = u16::from(names[2]) | u16::from(names[3] & 7) << 8;
            bus.address = bus.address & !0x1ffe0 | u32::from(tile) << 5 | u32::from(bank) << 16;
        }
        self.pixels[1].sample(p.data_slave, serial);
        if p.data_slave && self.pixels[1].gate(3) {
            self.captured_attributes_a = attributes(names, m5);
        }
        self.pixels[1].clock(p, self.cells.fetch_a[1] & 2 != 0, 4);
        if p.data_slave
            && if m5 {
                self.pixels[3].complete()
            } else {
                self.pixels[1].complete()
            }
        {
            self.staged[0] = [self.pixels[0].bytes, self.pixels[1].bytes];
            self.staged_attributes[0] = self.captured_attributes_a;
        }
        if selected(10) {
            self.fine_a = (horizontal_offset & 15) as u8;
        }
        if p.horizontal_slave && self.copy_a {
            self.current[0] = self.staged[0];
            self.attributes[0] = self.staged_attributes[0];
        }
        self.pixels[0].sample(p.data_slave, serial);
        self.pixels[0].clock(p, timing.gate(Event::PixelsA), 3);
        self.names.clock(p, self.cells.names[1] & 4 != 0, 3);
        self.pixels[2].sample(p.data_slave, serial);
        self.pixels[2].clock(p, timing.gate(Event::PixelsB), 3);
        if p.horizontal_slave && self.copy_b {
            self.current[1] = self.staged[1];
            self.attributes[1] = self.staged_attributes[1];
        }
        self.pixels[3].sample(p.data_slave, serial);
        self.pixels[3].clock(p, self.cells.fetch_b[1] & 2 != 0, 4);
        if p.data_slave && self.pixels[3].complete() {
            self.staged[1] = [self.pixels[2].bytes, self.pixels[3].bytes];
            self.staged_attributes[1] = attributes(names, m5);
        }
        if p.horizontal_master {
            self.phase_b[0] = (if timing.gate(Event::PixelResetB) {
                (horizontal_offset as u8 & 15) ^ 8
            } else {
                self.phase_b[1]
            })
            .wrapping_add(1)
                & 15;
        }
        if p.horizontal_slave {
            self.phase_b[1] = self.phase_b[0];
        }
        if p.horizontal_master {
            self.copy_b = self.phase_b[1] == 15;
        }
        bus
    }

    fn sample(&self, plane: usize, phase: u8, m5: bool) -> u8 {
        let half = usize::from(m5 && phase & 8 != 0);
        let shift = 1 - half;
        let attr = self.attributes[plane];
        let x = (phase & 7) ^ if m5 { 2 } else { 0 } ^ if attr & (1 << shift) != 0 { 7 } else { 0 };
        let index = if m5 {
            let byte = self.current[plane][half][usize::from(x >> 1)];
            (byte >> if x & 1 == 0 { 4 } else { 0 }) & 15
        } else {
            let banks = self.current[plane];
            [banks[0][0], banks[0][1], banks[1][0], banks[1][1]]
                .into_iter()
                .enumerate()
                .fold(0, |value, (bit, byte)| {
                    value | ((byte >> (x ^ 7)) & 1) << bit
                })
        };
        index
            | ((attr >> (2 + shift)) & 1) << 4
            | ((attr >> (4 + shift)) & 1) << 5
            | ((attr >> (6 + shift)) & 1) << 6
    }

    pub fn write_state(&self, writer: &mut StateWriter) {
        writer.write_u8(2);
        self.cells.write(writer);
        writer.write_bytes(&self.phase_b);
        writer.write_bytes(&self.window);
        for value in self.vertical {
            writer.write_u16(value);
        }
        self.scroll.write_state(writer);
        self.names.write_state(writer);
        for capture in self.pixels {
            capture.write_state(writer);
        }
        for bank in [self.staged, self.current] {
            for plane in bank {
                for row in plane {
                    writer.write_bytes(&row);
                }
            }
        }
        writer.write_bytes(&self.staged_attributes);
        writer.write_bytes(&self.attributes);
        writer.write_u8(self.captured_attributes_a);
        writer.write_u8(self.fine_a);
        writer.write_u8(self.horizontal_scroll);
        writer.write_bool(self.copy_a);
        writer.write_bool(self.copy_b);
    }

    pub fn read_state(reader: &mut StateReader<'_>) -> Result<Self> {
        ensure!(reader.read_u8()? == 2, "unsupported render plane state");
        let mut value = Self {
            cells: Cells::read(reader)?,
            ..Self::default()
        };
        reader.read_exact(&mut value.phase_b)?;
        reader.read_exact(&mut value.window)?;
        for word in &mut value.vertical {
            *word = reader.read_u16()?;
        }
        value.scroll = Capture::read_state(reader, 3)?;
        value.names = Capture::read_state(reader, 3)?;
        for (index, capture) in value.pixels.iter_mut().enumerate() {
            *capture = Capture::read_state(reader, 3 + (index as u8 & 1))?;
        }
        for bank in [&mut value.staged, &mut value.current] {
            for plane in bank {
                for row in plane {
                    reader.read_exact(row)?;
                }
            }
        }
        reader.read_exact(&mut value.staged_attributes)?;
        reader.read_exact(&mut value.attributes)?;
        value.captured_attributes_a = reader.read_u8()?;
        value.fine_a = reader.read_u8()?;
        value.horizontal_scroll = reader.read_u8()?;
        value.copy_a = reader.read_bool()?;
        value.copy_b = reader.read_bool()?;
        ensure!(
            value.phase_b.into_iter().all(|v| v < 16)
                && value.fine_a < 16
                && value.window.into_iter().all(|v| v & !0x9f == 0)
                && value.vertical.into_iter().all(|v| v < 2048)
                && value.scroll.bytes[1] < 4
                && value.scroll.bytes[3] < 4,
            "invalid held render plane values"
        );
        Ok(value)
    }
}

fn attributes(names: [u8; 4], m5: bool) -> u8 {
    let mut result = 0;
    for (index, source) in [3, 5, 6, 7].into_iter().enumerate() {
        result |= ((names[3] >> source) & 1) << (2 * index);
    }
    let first = if m5 { [3, 5, 6, 7] } else { [1, 3, 8, 4] };
    for (index, source) in first.into_iter().enumerate() {
        result |= (((u16::from(names[1]) >> source) & 1) as u8) << (2 * index + 1);
    }
    result
}
