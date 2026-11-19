use crate::hardware::vdp::state::{StateReader, StateWriter};
use anyhow::{Result, ensure};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Edge {
    None,
    Rise,
    Fall,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct SoundClock {
    half: u64,
    pass: u8,
    high: bool,
    last_edge: u64,
    rises: u64,
    high_width: u64,
    low_width: u64,
}

impl SoundClock {
    pub fn software_origin() -> Self {
        Self {
            half: 0,
            pass: 1,
            high: false,
            last_edge: 0,
            rises: 0,
            high_width: 0,
            low_width: 0,
        }
    }

    pub fn observe(&mut self, half: u64, pass: u8, high: bool) -> Result<Edge> {
        ensure!(pass < 2, "invalid SOUND board pass");
        if (half, pass) == (self.half, self.pass) {
            ensure!(
                high == self.high,
                "SOUND clock changed within one board observation"
            );
            return Ok(Edge::None);
        }
        let expected = if self.pass == 0 {
            (self.half, 1)
        } else {
            (
                self.half
                    .checked_add(1)
                    .ok_or_else(|| anyhow::anyhow!("SOUND half overflow"))?,
                0,
            )
        };
        ensure!((half, pass) == expected, "discontinuous SOUND board clock");
        let edge = if high == self.high {
            Edge::None
        } else if high {
            Edge::Rise
        } else {
            Edge::Fall
        };
        if edge != Edge::None {
            if self.last_edge != 0 {
                let width = half - self.last_edge;
                ensure!(width != 0, "SOUND clock changed twice within one half");
                if self.high {
                    self.high_width = width;
                } else {
                    self.low_width = width;
                }
            }
            self.last_edge = half;
        }
        if edge == Edge::Rise {
            self.rises = self
                .rises
                .checked_add(1)
                .ok_or_else(|| anyhow::anyhow!("SOUND edge overflow"))?;
        }
        self.half = half;
        self.pass = pass;
        self.high = high;
        Ok(edge)
    }

    pub fn half(&self) -> u64 {
        self.half
    }
    pub fn high(&self) -> bool {
        self.high
    }
    pub fn pass(&self) -> u8 {
        self.pass
    }
    pub fn rises(&self) -> u64 {
        self.rises
    }
    pub fn widths(&self) -> Option<(u64, u64)> {
        (self.high_width != 0 && self.low_width != 0).then_some((self.high_width, self.low_width))
    }
    pub fn phase_age(&self) -> u64 {
        self.half - self.last_edge
    }

    pub fn write_state(&self, w: &mut StateWriter) {
        w.write_u64(self.half);
        w.write_u8(self.pass);
        w.write_bool(self.high);
        w.write_u64(self.last_edge);
        w.write_u64(self.rises);
        w.write_u64(self.high_width);
        w.write_u64(self.low_width);
    }

    pub fn read_state(r: &mut StateReader<'_>) -> Result<Self> {
        let s = Self {
            half: r.read_u64()?,
            pass: r.read_u8()?,
            high: r.read_bool()?,
            last_edge: r.read_u64()?,
            rises: r.read_u64()?,
            high_width: r.read_u64()?,
            low_width: r.read_u64()?,
        };
        ensure!(
            s.pass < 2
                && s.last_edge <= s.half
                && s.rises <= s.half
                && s.high_width <= s.half
                && s.low_width <= s.half,
            "invalid SOUND clock history"
        );
        if s.half == 0 {
            ensure!(s == Self::software_origin(), "invalid SOUND clock origin");
        }
        Ok(s)
    }
}
