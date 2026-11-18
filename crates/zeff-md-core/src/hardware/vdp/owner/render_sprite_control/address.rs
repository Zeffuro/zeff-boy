use super::*;

impl SpriteControl {
    pub(super) fn finish_bus(
        &mut self,
        c: RenderContext,
        t: RenderTiming,
        mut bus: SharedBus,
        serial: u8,
    ) -> SharedBus {
        let p = c.phases;
        let early = c.registers.at(RegisterStage::ScrollX);
        let wide = early[12] & 1 != 0;
        let m5 = early[1] & 4 != 0;
        if !m5 && (self.cells.attribute_delay[1] != 0 || self.mode4.pattern_slot[1]) {
            let index = if self.cells.attribute_delay[1] != 0 {
                1
            } else {
                0
            };
            let pattern = self.attributes[index];
            let row = self.mode4.row_queue[1][8 + index];
            let bit = if early[1] & 2 != 0 {
                (row >> 3) & 1
            } else {
                pattern & 1
            };
            bus.address = bus.address & !0x3fff
                | u32::from(c.horizontal & 2 == 0) << 1
                | u32::from(pattern & 254) << 5
                | u32::from(row & 7) << 2
                | u32::from(bit) << 5
                | u32::from(early[6] & 4) << 11;
        }
        let late = c.registers.at(RegisterStage::Sprite);
        if !m5 && (self.cells.scan_slots[1] & 1 != 0 || self.cells.address_delay[1] & 1 != 0) {
            bus.address = bus.address & !0x3f00 | u32::from(late[5] & 126) << 7;
        }
        if m5 && self.cells.address_delay[1] & 1 != 0 {
            let index = self.queue[1][usize::from(if wide { 19u8 } else { 15u8 })] & 127;
            let index = if wide {
                index
            } else {
                index & 63 | (late[5] & 1) << 6
            };
            bus.address = u32::from(late[5] & 254) << 9 | u32::from(index) << 3 | 4;
        }
        if !m5 && self.cells.address_delay[1] & 1 != 0 {
            bus.address = bus.address & !255 | u32::from(self.queue[1][7] & 127) << 1;
        }
        if p.horizontal_master {
            self.cells.parity[0] = if t.scan_end() {
                0
            } else {
                (self.cells.parity[1] + (self.cells.parity_delays[1] & 1)) & 1
            };
        }
        if p.horizontal_slave {
            self.cells.parity[1] = self.cells.parity[0];
        }
        let stride = if self.stride_enable {
            (u16::from(self.stride_step)
                + u16::from(self.stride[1])
                + if self.stride_step {
                    u16::from(self.selected.height)
                } else {
                    0
                })
                & 15
        } else {
            0
        };
        if self.cells.pattern_slots[1] & 1 != 0 {
            let offset = (u16::from((self.selected.row >> 3) & 3) + stride) & 15;
            let tile = (self.selected.pattern + offset) & 2047;
            bus.address = u32::from(self.selected.row & 7) << 2
                | u32::from(tile) << 5
                | u32::from(late[6] & 32) << 11;
        }
        if p.horizontal_slave {
            self.zero_x = self.selected.x == 0;
        }
        if p.horizontal_master {
            self.stride[1] = self.stride[0];
        }
        if p.horizontal_slave {
            self.stride[0] = stride as u8;
        }
        if p.data_slave {
            if self.serial_lead[1] {
                self.pattern[0] = serial;
            }
            if self.serial_lead[0] {
                self.pattern[1] = serial;
            }
        }
        if self.cells.fetch_pipe[1] & 1 != 0 {
            self.mode4.x = self.pattern[usize::from(self.cells.parity_delays[1] & 2 != 0)];
        }
        bus
    }

    pub(super) fn decode_descriptor(&self) -> FetchWord {
        let high = self.attributes[1];
        let flip = high & 16 != 0;
        let height = self.size & 3;
        let mut tile = ((self.row >> 3) & 3) + u8::from(flip && height == 2);
        tile &= 3;
        if flip && height != 0 {
            tile ^= 1;
        }
        if flip && height & 2 != 0 {
            tile ^= 2;
        }
        let row = (self.row ^ if flip { 15 } else { 0 }) & 7 | tile << 3 | (self.row & 32);
        FetchWord {
            pattern: u16::from(self.pattern[0]) | u16::from(high & 7) << 8,
            x: u16::from(self.pattern[1]) | u16::from(self.attributes[0] & 1) << 8,
            flip: high & 8 != 0,
            palette: (high >> 5) & 3,
            priority: high & 128 != 0,
            width: (self.size >> 2) & 3,
            height,
            row,
        }
    }
}
