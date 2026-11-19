use crate::hardware::vdp::state::{StateReader, StateWriter};
use anyhow::{Result, ensure};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct SpriteSignals {
    pub overflow: bool,
    pub collision: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Inputs {
    pub pal: bool,
    pub dma_active: bool,
    pub hblank: bool,
    pub vblank: bool,
    pub odd_field: bool,
    pub vint_pending: bool,
    pub fifo_full: bool,
    pub fifo_empty: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct Status {
    overflow: bool,
    collision: bool,
}

impl Status {
    pub fn sample(&mut self, old_vint: bool, signals: SpriteSignals, read_clear: bool) {
        // Sprite sets take priority over the delayed status-read clear.
        if !old_vint && signals.overflow {
            self.overflow = true;
        } else if read_clear {
            self.overflow = false;
        }
        if signals.collision {
            self.collision = true;
        } else if read_clear {
            self.collision = false;
        }
    }

    pub fn word(self, input: Inputs) -> u16 {
        u16::from(input.pal)
            | u16::from(input.dma_active) << 1
            | u16::from(input.hblank) << 2
            | u16::from(input.vblank) << 3
            | u16::from(input.odd_field) << 4
            | u16::from(self.collision) << 5
            | u16::from(self.overflow) << 6
            | u16::from(input.vint_pending) << 7
            | u16::from(input.fifo_full) << 8
            | u16::from(input.fifo_empty) << 9
    }

    pub fn write_state(self, writer: &mut StateWriter) {
        writer.write_u8(1);
        writer.write_bool(self.overflow);
        writer.write_bool(self.collision);
    }

    pub fn read_state(reader: &mut StateReader<'_>) -> Result<Self> {
        ensure!(
            reader.read_u8()? == 1,
            "unsupported VDP sprite-status state"
        );
        Ok(Self {
            overflow: reader.read_bool()?,
            collision: reader.read_bool()?,
        })
    }
}
