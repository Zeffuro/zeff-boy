use super::super::render_timing::RenderTiming;
use super::{SpriteRaster, line_port::PortPins};
use crate::hardware::vdp::control_core::PhaseLevels;
use anyhow::{Result, ensure};

impl SpriteRaster {
    pub(super) fn line_pins(&self, m5: bool, timing: RenderTiming, data_master: bool) -> PortPins {
        let displaying = if m5 {
            self.display[1] & 2 != 0
        } else {
            timing.memory_pixels()
        };
        let read = displaying && self.horizontal[1] & 7 == 0;
        let writing =
            self.positions[1].valid && (!self.idle || self.positions[1].previous_inverse_idle);
        let attrs = if displaying {
            0
        } else {
            self.active_attributes << 4
        };
        PortPins {
            address: if displaying {
                self.port.rows[1]
            } else {
                Some(self.positions[1].address)
            },
            enabled: self.read_delay[1] || writing || self.positions[1].writing || read,
            data_master,
            writes: self.writes,
            incoming: self.pixels.row().map(|pixel| pixel | attrs),
        }
    }

    pub fn begin_line_edge(
        &mut self,
        phases: PhaseLevels,
        mode5: bool,
        horizontal: u16,
        timing: RenderTiming,
    ) -> Result<()> {
        let pins = self.line_pins(mode5, timing, phases.data_master);
        let input = (horizontal + if mode5 { 494 } else { 501 }) & 511;
        self.port.begin_edge(pins, phases, input, &self.lines)
    }

    pub fn finish_line_edge(&mut self) -> Result<bool> {
        self.port.finish_edge(&mut self.lines)
    }

    pub(super) fn occupied(
        &self,
        lane: usize,
        m5: bool,
        timing: RenderTiming,
        phases: PhaseLevels,
    ) -> Result<bool> {
        let value = self.output[lane] & 15;
        let known = self.port.output_known[lane] & 15;
        if value != 0 {
            return Ok(true);
        }
        let missing = !known & 15;
        let address = self.line_pins(m5, timing, false).address;
        ensure!(
            missing == 0,
            "unknown sprite line occupancy bits lane={lane} address={address:?} parity={:?} missing={missing:02x} phases={phases:?}",
            address.map(|v| v & 1)
        );
        Ok(false)
    }
    pub(super) fn load_output(
        &self,
        lane: usize,
        base: u8,
        m5: bool,
        timing: RenderTiming,
        phases: PhaseLevels,
    ) -> Result<u8> {
        let missing = !self.port.output_known[lane] & !base & 127;
        let address = self.line_pins(m5, timing, false).address;
        ensure!(
            missing == 0,
            "unknown sprite line shifter bits lane={lane} address={address:?} parity={:?} missing={missing:02x} phases={phases:?}",
            address.map(|v| v & 1)
        );
        Ok(base | self.output[lane])
    }

    pub(super) fn prepare_writes(
        &mut self,
        incoming: [u8; 8],
        m5: bool,
        t: RenderTiming,
        p: PhaseLevels,
    ) -> Result<()> {
        let mut collision = false;
        for (i, &pixel) in incoming.iter().enumerate() {
            let coverage = (if i == 7 {
                !self.edge_split
            } else {
                self.thresholds[i] ^ self.edge_split
            }) && !self.write_disabled;
            let occupied = if coverage && (!self.clear || (pixel != 0 && !collision)) {
                self.occupied(i, m5, t, p)?
            } else {
                false
            };
            self.writes[i] = self.clear || coverage && !occupied;
            collision |= coverage && occupied && pixel != 0;
        }
        self.collision[0] = collision;
        Ok(())
    }
}
