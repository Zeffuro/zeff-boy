mod address;
mod mode4;
mod read_responder;
use mode4::Mode4;
mod state;

use read_responder::{DescriptorWrite, FunctionalReadResponder};

use super::render_timing::{Event, RenderTiming};
use super::{
    domain::{RenderContext, SharedBus},
    ram,
    render_sat::SpriteAttributes,
};
use crate::hardware::vdp::control_core::RegisterStage;
use crate::hardware::vdp::state::{StateReader, StateWriter};
use anyhow::{Result, ensure};

macro_rules! cells {
    ($($name:ident:$mask:expr),+ $(,)?) => {
        #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
        struct Cells { $( $name:[u16;2], )+ }
        impl Cells {
            fn commit(&mut self) { $(self.$name[1]=self.$name[0];)+ }
            fn write(self,w:&mut StateWriter) { $(for v in self.$name {w.write_u16(v);})+ }
            fn read(r:&mut StateReader<'_>)->Result<Self> {
                let mut s=Self::default();
                $(for v in &mut s.$name {*v=r.read_u16()?;ensure!(*v & !$mask==0,"invalid sprite control cell");})+
                Ok(s)
            }
        }
    }
}

cells! { scan:127, scan_steps:31, scan_slots:3, vertical_history:3, scan_reset:1,
address_delay:7, size_word:2047, size_bits:15, ypos:1023, active_guard:1,
stop_flags:3, line_reset:1, serial_delay:1, filler:3, index0:127,index1:127,index2:127,
attributes_ready:7, queue_index:31, line_end:3, masking:15, tiles:3,
pattern_slots:3, render_ready:15, fetch_pipe:7, parity:1, parity_delays:3,
draw_enabled:1, descriptor_source:1, attribute_delay:1 }

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct FetchWord {
    pub pattern: u16,
    pub x: u16,
    pub flip: bool,
    pub palette: u8,
    pub priority: bool,
    pub width: u8,
    pub height: u8,
    pub row: u8,
}

impl FetchWord {
    fn write(self, w: &mut StateWriter) {
        w.write_u16(self.pattern);
        w.write_u16(self.x);
        w.write_bool(self.flip);
        w.write_u8(self.palette);
        w.write_bool(self.priority);
        w.write_u8(self.width);
        w.write_u8(self.height);
        w.write_u8(self.row);
    }
    fn read(r: &mut StateReader<'_>) -> Result<Self> {
        let s = Self {
            pattern: r.read_u16()?,
            x: r.read_u16()?,
            flip: r.read_bool()?,
            palette: r.read_u8()?,
            priority: r.read_bool()?,
            width: r.read_u8()?,
            height: r.read_u8()?,
            row: r.read_u8()?,
        };
        ensure!(
            s.pattern < 2048
                && s.x < 512
                && s.palette < 4
                && s.width < 4
                && s.height < 4
                && s.row < 64,
            "invalid held sprite descriptor"
        );
        Ok(s)
    }
}

#[derive(Clone, Copy)]
pub(super) struct RasterControl {
    pub selected: FetchWord,
    pub fetch_pipe: u8,
    pub ready: u8,
    pub mode4_x: u8,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct SpriteControl {
    cells: Cells,
    mode4: Mode4,
    sat: SpriteAttributes,
    sat_known: [bool; 2],
    complement_known: bool,
    queue: [[u8; 20]; 2],
    descriptor_reads: FunctionalReadResponder,
    selected: FetchWord,
    complement_size: u16,
    captured_size: u16,
    complement_y: u16,
    subtract_y: u16,
    captured_y: u16,
    delayed_y: u16,
    delta: u16,
    serial_y_lead: bool,
    size_capture: bool,
    stopped: bool,
    overflow: bool,
    overflow_output: bool,
    drawing: bool,
    zero_x: bool,
    stride_enable: bool,
    stride_step: bool,
    stride: [u8; 2],
    size: u8,
    row: u8,
    serial_lead: [bool; 2],
    serial_delay: [[bool; 2]; 2],
    attributes: [u8; 2],
    pattern: [u8; 2],
}

impl SpriteControl {
    pub fn software_origin() -> Self {
        Self {
            cells: Cells::default(),
            mode4: Mode4::default(),
            sat: SpriteAttributes::software_origin(),
            sat_known: [true; 2],
            complement_known: true,
            queue: [[0; 20]; 2],
            descriptor_reads: FunctionalReadResponder::software_origin(),
            selected: FetchWord::default(),
            complement_size: 0,
            captured_size: 0,
            complement_y: 0,
            subtract_y: 0,
            captured_y: 0,
            delayed_y: 0,
            delta: 0,
            serial_y_lead: false,
            size_capture: false,
            stopped: false,
            overflow: false,
            overflow_output: false,
            drawing: false,
            zero_x: false,
            stride_enable: false,
            stride_step: false,
            stride: [0; 2],
            size: 0,
            row: 0,
            serial_lead: [false; 2],
            serial_delay: [[false; 2]; 2],
            attributes: [0; 2],
            pattern: [0; 2],
        }
    }

    pub fn overflow_output(&self) -> bool {
        self.overflow_output
    }
    pub fn raster(&self) -> RasterControl {
        RasterControl {
            selected: self.selected,
            fetch_pipe: self.cells.fetch_pipe[1] as u8,
            ready: self.cells.render_ready[1] as u8,
            mode4_x: self.mode4.x,
        }
    }

    pub fn begin_descriptor_edge(&mut self, horizontal_master: bool) -> Result<()> {
        let qualified = self.cells.descriptor_source[1] != 0;
        let capture = self.cells.attributes_ready[1] & 2 != 0;
        let word = if qualified {
            self.decode_descriptor()
        } else {
            FetchWord::default()
        };
        self.descriptor_reads.begin_edge(
            self.cells.queue_index[1] as u8,
            horizontal_master,
            DescriptorWrite {
                pattern: capture,
                x: capture || !qualified,
                attributes: capture,
                word,
            },
        )
    }

    pub fn finish_descriptor_edge(&mut self) -> Result<bool> {
        self.descriptor_reads.finish_edge()
    }

    pub fn validate_descriptor_frontier(&self) -> Result<()> {
        self.descriptor_reads.validate_frontier()
    }

    pub fn clock(
        &mut self,
        c: RenderContext,
        t: RenderTiming,
        mut bus: SharedBus,
        serial: &mut u8,
        external_serial: u8,
    ) -> Result<SharedBus> {
        let p = c.phases;
        let regs = c.registers.at(RegisterStage::ScrollX);
        let wide = regs[12] & 1 != 0;
        let m5 = regs[1] & 4 != 0;
        let scan_advance = !m5 && self.cells.scan_slots[1] & 1 != 0;
        let attribute_slot = t.gate(Event::SpriteAttribute);
        let serial_slot = t.gate(Event::SpriteSerial) || t.gate(Event::Mode4SpriteSerial);
        let scanner_phase = if m5 {
            self.cells.scan_steps[1] & 6 != 0 || self.cells.address_delay[1] & 4 != 0
        } else {
            self.cells.scan_steps[1] & 4 != 0
        };
        if p.horizontal_master {
            let old = self.cells;
            let selected_index = self
                .sat
                .selected_index(old.scan[1] as u8, old.address_delay[1] & 2 != 0);
            let mut memory = c.frontend;
            memory.address = bus.address;
            memory.data = bus.data;
            self.sat.clock(
                p,
                ram::Profile {
                    registers: regs,
                    mode128: c.mode128,
                    interlace_latch: [false; 2],
                    test0: 0,
                },
                memory,
            )?;
            let entry = self.sat.entry(selected_index);
            let size0 = old.size_bits[1] & 2 != 0;
            let size1 = old.size_bits[1] & 1 != 0;
            let dy = self.delta;
            let in_range = !(!size0 && !size1 && dy & 8 != 0)
                && !(!size0 && dy & 16 != 0)
                && !(size0 && !size1 && dy & 24 == 24)
                && dy & 0xc0 == 0xc0
                && (!m5 || dy & 0x100 != 0)
                && dy & 32 == 0
                && old.active_guard[1] != 0;
            let phase = old.scan_steps[1] & if m5 { 12 } else { 24 } != 0;
            let selected_y = if !m5 && old.scan_steps[1] & 16 != 0 {
                u16::from(self.mode4.serial_y)
            } else {
                self.delayed_y
            };
            let terminal = phase
                && if m5 {
                    self.captured_size & 127 == 127
                } else {
                    selected_y == 208
                };
            let top = usize::from(if !m5 {
                7u8
            } else if wide {
                19u8
            } else {
                15u8
            });
            let available = self.queue[1][top] & 128 != 0;
            let full = in_range && !available;
            let filler = old.filler[1] & 1 != 0;
            let append = in_range && available || filler;
            let tile_start = old.tiles[1] == 0 && attribute_slot;
            let tile_done = old.attribute_delay[1] != 0 && old.tiles[1] == 0;
            let masked = old.masking[1] & 2 == 0 && !self.zero_x;
            let stop_draw = old.masking[1] & 8 != 0 && !masked;
            let old_index = usize::from(old.queue_index[1]);
            if tile_start {
                self.selected = self.descriptor_reads.selected().map_err(|error| anyhow::anyhow!(
                        "{error}; index={} HC={} VC={} phases={:?} R1={:02x} R12={:02x} tiles={} attribute={} line_end={} attributes_ready={} descriptor_source={} pattern_slots={} render_ready={} fetch_pipe={} draw_enabled={} masking={} scan_end={} pixels={}",
                        old_index, c.horizontal, c.vertical,
                        [p.horizontal_master, p.horizontal_slave, p.data_master, p.data_slave],
                        regs[1], regs[12], old.tiles[1], attribute_slot, old.line_end[1],
                        old.attributes_ready[1], old.descriptor_source[1], old.pattern_slots[1],
                        old.render_ready[1], old.fetch_pipe[1], old.draw_enabled[1], old.masking[1],
                        t.scan_end(), t.memory_pixels()
                    ))?;
            }
            // Zero is only the canonical payload of an explicitly unknown capture.
            self.cells.size_word[0] = entry.map_or(0, |entry| {
                u16::from(entry.size) << 7 | u16::from(entry.link)
            });
            self.cells.ypos[0] = entry.map_or(0, |entry| entry.position);
            self.sat_known[0] = entry.is_some();
            self.complement_known = self.sat_known[1];
            self.complement_size = if self.complement_known {
                old.size_word[1] ^ 2047
            } else {
                0
            };
            self.complement_y = if self.complement_known {
                old.ypos[1] ^ 1023
            } else {
                0
            };
            self.size_capture = scanner_phase;
            self.cells.size_bits[0] = ((!self.captured_size >> 7) & 12)
                | if m5 {
                    (!self.captured_size >> 7) & 3
                } else {
                    u16::from(regs[1] & 2 != 0)
                };
            self.subtract_y = selected_y ^ 1023;
            self.cells.scan_steps[0] = ((old.scan_steps[1] << 1)
                | u16::from(t.gate(Event::SpriteScan) && (c.vertical_display || c.vertical_last)))
                & 31;
            self.cells.scan_slots[0] =
                ((old.scan_slots[1] << 1) | u16::from(t.gate(Event::SpriteScan))) & 3;
            self.cells.scan_reset[0] =
                u16::from(t.gate(Event::VerticalStep) || old.vertical_history[1] & 3 == 1);
            self.cells.vertical_history[0] =
                ((old.vertical_history[1] << 1) | u16::from(c.vertical_last)) & 3;
            self.cells.address_delay[0] =
                ((old.address_delay[1] << 1) | u16::from(t.gate(Event::SpriteAddress))) & 7;
            self.cells.masking[0] = u16::from(tile_done)
                | u16::from(old.masking[1] & 1 != 0 && old.queue_index[1] & 20 == 20) << 1
                | u16::from(m5 && stop_draw) << 2
                | u16::from(masked) << 3;
            self.cells.line_end[0] =
                u16::from(t.scan_end()) | u16::from(t.scan_end() || t.gate(Event::Zero)) << 1;
            self.cells.attributes_ready[0] = ((old.attributes_ready[1] << 1)
                | u16::from(!available && old.serial_delay[1] != 0))
                & 7;
            self.cells.line_reset[0] = u16::from(t.gate(Event::VerticalStep));
            self.cells.filler[0] = ((old.filler[1] << 1)
                | u16::from(t.gate(Event::SpriteSerial) || t.gate(Event::BlankSprite)))
                & 3;
            self.cells.active_guard[0] = u16::from(!self.stopped && phase);
            self.cells.stop_flags[0] = u16::from(terminal) | u16::from(full) << 1;
            self.cells.serial_delay[0] = u16::from(if m5 {
                serial_slot
            } else {
                self.mode4.serial_slot[1]
            });
            self.cells.index0[0] = if m5 {
                old.scan[1]
            } else {
                64 | u16::from(self.mode4.scan_delay[3]) << 1 | u16::from(t.gate(Event::Odd))
            };
            self.cells.index1[0] = old.index0[1];
            self.cells.index2[0] = old.index1[1];
            self.cells.pattern_slots[0] =
                (old.attribute_delay[1] << 1) | u16::from(m5 && attribute_slot);
            self.cells.attribute_delay[0] = u16::from(attribute_slot);
            self.cells.parity_delays[0] =
                u16::from(old.render_ready[1] & 8 != 0) | old.parity[1] << 1;
            self.cells.draw_enabled[0] = u16::from(self.drawing);
            self.cells.descriptor_source[0] = u16::from(m5 && !tile_start);
            self.cells.fetch_pipe[0] = ((old.fetch_pipe[1] << 1)
                | u16::from(
                    old.draw_enabled[1] != 0
                        && old.render_ready[1] & 8 != 0
                        && (m5 || self.mode4.availability[1] & 512 == 0),
                ))
                & 7;
            self.cells.render_ready[0] = ((old.render_ready[1] << 1) & 6)
                | u16::from(t.gate(Event::Mode4SpritePhase) || m5 && attribute_slot)
                | u16::from(if m5 {
                    old.render_ready[1] & 4 != 0
                } else {
                    self.mode4.ready_delay[1]
                }) << 3;
            self.cells.scan[0] =
                if old.scan_reset[1] != 0 {
                    0
                } else if m5 && old.scan_slots[1] != 0 {
                    u16::from(entry.ok_or_else(|| anyhow::anyhow!(
                    "unowned SAT link consumption index={selected_index} HC={} VC={}",
                    c.horizontal, c.vertical))?.link)
                } else {
                    (old.scan[1] + u16::from(scan_advance)) & 127
                };
            self.cells.queue_index[0] = if old.line_end[1] & 2 != 0 {
                0
            } else {
                (old.queue_index[1]
                    + u16::from(!m5 || old.attributes_ready[1] & 4 != 0 || tile_done))
                    & 31
            };
            self.cells.tiles[0] = if old.line_end[1] & 1 != 0 {
                0
            } else {
                ((if tile_start {
                    u16::from(self.selected.width ^ 3)
                } else {
                    old.tiles[1]
                }) + u16::from(attribute_slot))
                    & 3
            };
            self.mode4.prepare(
                old.scan[1],
                serial_slot,
                t.gate(Event::Mode4SpritePattern),
                old.render_ready[1] & 4 != 0,
                available,
                append,
                self.delta as u8,
            );
            self.stride_enable = !(old.line_end[1] & 1 != 0 || tile_start);
            self.stride_step = old.pattern_slots[1] & 2 != 0;
            self.queue[0] = self.queue[1];
            if append {
                self.queue[0].copy_within(0..19, 1);
                self.queue[0][0] = old.index2[1] as u8 | u8::from(filler) << 7;
            }
        } else if p.horizontal_slave {
            let mut memory = c.frontend;
            memory.address = bus.address;
            memory.data = bus.data;
            self.sat.clock(
                p,
                ram::Profile {
                    registers: regs,
                    mode128: c.mode128,
                    interlace_latch: [false; 2],
                    test0: 0,
                },
                memory,
            )?;
        }
        if p.horizontal_slave {
            let delta = (c.vertical + if m5 { 130 } else { 1 } + self.subtract_y) & 1023;
            self.cells.commit();
            self.mode4.commit();
            self.sat_known[1] = self.sat_known[0];
            self.delta = delta ^ 0x3c0;
            self.delayed_y = self.captured_y ^ 1023;
            self.queue[1] = self.queue[0];
            if self.cells.stop_flags[1] != 0 {
                self.stopped = true;
            } else if self.cells.line_reset[1] != 0 {
                self.stopped = false;
            }
            if self.cells.stop_flags[1] & 2 != 0 {
                self.overflow = true;
            } else if self.cells.line_reset[1] != 0 {
                self.overflow = false;
            }
            if self.cells.line_end[1] & 1 != 0 {
                self.drawing = true;
            } else if self.cells.masking[1] & 4 != 0 {
                self.drawing = false;
            }
        }
        if p.horizontal_slave && p.data_slave && self.size_capture {
            ensure!(
                self.complement_known,
                "unowned SAT size/link capture HC={} VC={}",
                c.horizontal,
                c.vertical
            );
            self.captured_size = self.complement_size;
        }
        if p.data_master {
            self.serial_y_lead = p.horizontal_master && scanner_phase;
            self.mode4.y_delay[1] = self.mode4.y_delay[0];
        }
        if p.data_slave {
            self.mode4.y_delay[0] = self.serial_y_lead;
            if self.mode4.y_delay[1] {
                self.mode4.serial_y = *serial;
            }
        }
        if p.data_slave && self.serial_y_lead {
            ensure!(
                !m5 || self.complement_known,
                "unowned SAT Y capture HC={} VC={}",
                c.horizontal,
                c.vertical
            );
            self.captured_y = if m5 {
                self.complement_y
            } else {
                u16::from(*serial ^ 255) | 768
            };
        }
        if scan_advance {
            bus.address = bus.address & !255 | u32::from(self.cells.scan[1] & 31) << 1;
        }
        if p.horizontal_master && p.data_master && self.cells.filler[1] & 2 != 0 {
            self.size = self.cells.size_bits[1] as u8;
            self.row = (self.delta & 63) as u8;
        }
        self.overflow_output = self.overflow && t.gate(Event::VerticalStep);
        for i in 0..2 {
            if p.data_slave && self.serial_delay[i][1] {
                self.attributes[i] = *serial;
            }
        }
        if p.data_master {
            *serial = external_serial;
            for delay in &mut self.serial_delay {
                delay[1] = delay[0];
            }
            self.serial_lead = [
                p.horizontal_master && self.cells.serial_delay[1] != 0,
                p.horizontal_master && serial_slot,
            ];
        }
        if p.data_slave {
            for i in 0..2 {
                self.serial_delay[i][0] = self.serial_lead[i];
            }
        }
        Ok(self.finish_bus(c, t, bus, *serial))
    }
}
