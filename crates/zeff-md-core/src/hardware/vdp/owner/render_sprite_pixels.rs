use crate::hardware::vdp::control_core::PhaseLevels;
use crate::hardware::vdp::state::{StateReader, StateWriter};
use anyhow::{Result, ensure};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct SpritePixels {
    lead: [bool; 2],
    delay: [[bool; 2]; 2],
    complete: [u8; 2],
    bytes: [u8; 4],
    staged: [u8; 3],
    captured: [u8; 4],
    selection: [u8; 2],
    active: bool,
    span: bool,
    flip: [bool; 2],
    alignment: [bool; 2],
    alignment_source: bool,
    group: [u8; 2],
    rotation: u8,
    selected_byte: u8,
    inactive: bool,
    inactive_delay: [bool; 2],
    falling: [bool; 2],
    rotate_enabled: bool,
    rotate_clear: [bool; 2],
    slots: [u8; 8],
    row: [u8; 8],
}

impl SpritePixels {
    pub fn active(&self) -> bool {
        self.active
    }
    pub fn row(&self) -> [u8; 8] {
        self.row
    }

    #[allow(clippy::too_many_arguments)]
    pub fn clock(
        &mut self,
        p: PhaseLevels,
        m5: bool,
        ready: u8,
        fetch: bool,
        display: bool,
        flip: bool,
        x: u16,
        serial: u8,
    ) {
        if p.horizontal_master {
            self.inactive = !display;
        }
        let start = self.span && p.horizontal_slave;
        if p.data_master {
            self.lead = [
                p.horizontal_master && ready & 4 != 0,
                p.horizontal_master && ready & 8 != 0,
            ];
            for pair in &mut self.delay {
                pair[1] = pair[0];
            }
            self.complete[1] = self.complete[0];
            self.selection[0] = ((self.selection[1] << 1) | u8::from(!start)) & 15;
            self.active = self.selection[1] & 8 == 0;
            self.flip[0] = flip;
            self.rotation = (self.group[1] << 1) | u8::from(self.alignment[1]);
            self.alignment[0] = self.alignment_source;
            self.inactive_delay[0] = self.inactive;
            self.falling[0] = self.inactive_delay[1] && !self.inactive;
        }
        if p.data_slave {
            let old_second = self.delay[1][1];
            for i in 0..2 {
                self.delay[i][0] = self.lead[i];
            }
            self.complete[0] = ((self.complete[1] << 1) | u8::from(old_second)) & 3;
            self.selection[1] = self.selection[0];
            self.flip[1] = self.flip[0];
            self.alignment[1] = self.alignment[0];
            self.inactive_delay[1] = self.inactive_delay[0];
            self.falling[1] = self.falling[0];
            if old_second {
                self.staged = [self.bytes[2], self.bytes[1], self.bytes[0]];
            }
            for (index, gate) in [self.lead[0], self.delay[0][1], self.lead[1], old_second]
                .into_iter()
                .enumerate()
            {
                if gate {
                    self.bytes[index] = serial;
                }
            }
            if self.complete[1] & 2 != 0 {
                self.captured = [
                    self.bytes[3],
                    self.staged[0],
                    self.staged[1],
                    self.staged[2],
                ];
            }
        }
        if p.horizontal_master {
            self.span = fetch;
        }
        if p.data_master {
            let mut byte = 0;
            if !self.falling[1] {
                if m5 {
                    for (position, index) in [1, 0, 3, 2].into_iter().enumerate() {
                        let gate = if self.flip[1] { position } else { 3 - position };
                        if self.selection[1] & (1 << gate) == 0 {
                            byte |= self.captured[index];
                        }
                    }
                } else {
                    for position in 0..4 {
                        if self.selection[1] & (1 << position) == 0 {
                            for (plane, source) in self.captured.into_iter().enumerate() {
                                byte |= ((source >> (7 - 2 * position)) & 1) << (7 - plane);
                                byte |= ((source >> (6 - 2 * position)) & 1) << (3 - plane);
                            }
                        }
                    }
                }
            }
            self.selected_byte = if self.alignment[1] ^ self.flip[1] {
                byte.rotate_left(4)
            } else {
                byte
            };
            self.group[0] = if start {
                ((x >> 1) & 3) as u8
            } else {
                (self.group[1] + 1) & 3
            };
        }
        if p.data_slave {
            self.group[1] = self.group[0];
        }
        if start {
            self.alignment_source = x & 1 != 0;
        }
        if p.data_slave {
            for (index, slot) in self.slots.iter_mut().enumerate() {
                if !self.rotate_enabled
                    || usize::from(self.rotation) == index
                    || usize::from(self.rotation) == (index + 7) & 7
                {
                    *slot = (self.selected_byte >> if index & 1 == 0 { 4 } else { 0 }) & 15;
                }
            }
            if self.rotate_clear[1] || self.active {
                self.row = self.slots;
            }
            self.rotate_clear[0] = !self.rotate_enabled;
        }
    }

    pub fn finish(&mut self, p: PhaseLevels) {
        if p.data_master {
            self.rotate_clear[1] = self.rotate_clear[0];
            self.rotate_enabled = !self.falling[1];
        }
    }

    pub fn write_state(&self, w: &mut StateWriter) {
        w.write_u8(1);
        for v in self.lead {
            w.write_bool(v);
        }
        for pair in self.delay {
            for v in pair {
                w.write_bool(v);
            }
        }
        w.write_bytes(&self.complete);
        w.write_bytes(&self.bytes);
        w.write_bytes(&self.staged);
        w.write_bytes(&self.captured);
        w.write_bytes(&self.selection);
        for v in [self.active, self.span] {
            w.write_bool(v);
        }
        for pair in [self.flip, self.alignment] {
            for v in pair {
                w.write_bool(v);
            }
        }
        w.write_bool(self.alignment_source);
        w.write_bytes(&self.group);
        w.write_u8(self.rotation);
        w.write_u8(self.selected_byte);
        w.write_bool(self.inactive);
        for pair in [self.inactive_delay, self.falling, self.rotate_clear] {
            for v in pair {
                w.write_bool(v);
            }
        }
        w.write_bool(self.rotate_enabled);
        w.write_bytes(&self.slots);
        w.write_bytes(&self.row);
    }

    pub fn read_state(r: &mut StateReader<'_>) -> Result<Self> {
        ensure!(r.read_u8()? == 1, "unsupported sprite pixel state");
        let mut s = Self::default();
        for v in &mut s.lead {
            *v = r.read_bool()?;
        }
        for pair in &mut s.delay {
            for v in pair {
                *v = r.read_bool()?;
            }
        }
        r.read_exact(&mut s.complete)?;
        r.read_exact(&mut s.bytes)?;
        r.read_exact(&mut s.staged)?;
        r.read_exact(&mut s.captured)?;
        r.read_exact(&mut s.selection)?;
        s.active = r.read_bool()?;
        s.span = r.read_bool()?;
        for pair in [&mut s.flip, &mut s.alignment] {
            for v in pair {
                *v = r.read_bool()?;
            }
        }
        s.alignment_source = r.read_bool()?;
        r.read_exact(&mut s.group)?;
        s.rotation = r.read_u8()?;
        s.selected_byte = r.read_u8()?;
        s.inactive = r.read_bool()?;
        for pair in [&mut s.inactive_delay, &mut s.falling, &mut s.rotate_clear] {
            for v in pair {
                *v = r.read_bool()?;
            }
        }
        s.rotate_enabled = r.read_bool()?;
        r.read_exact(&mut s.slots)?;
        r.read_exact(&mut s.row)?;
        ensure!(
            s.complete.into_iter().all(|v| v < 4)
                && s.selection.into_iter().all(|v| v < 16)
                && s.group.into_iter().all(|v| v < 4)
                && s.rotation < 8
                && s.slots.into_iter().chain(s.row).all(|v| v < 16),
            "invalid sprite pixel captures"
        );
        Ok(s)
    }
}
