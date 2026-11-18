#[path = "sound_copy_cells.rs"]
mod copy_cells;
#[path = "sound_history.rs"]
mod history;
use super::super::sound_semantics::{Plan, Shape};
use super::SoundPins;
use crate::hardware::vdp::state::{StateReader, StateWriter};
use anyhow::{Result, ensure};
pub(super) use history::M1History;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct M1Cells {
    busy: bool,
    busy_low: bool,
    first_high: bool,
    second: bool,
    read_master: bool,
    read_latch: bool,
    read_end: bool,
    read_end_high: bool,
    refresh_phase: bool,
    wait_high: bool,
    wait_low: bool,
    pub reset_enabled: bool,
    address_enabled: bool,
    clear_high: bool,
    read_select: bool,
    read_data: bool,
    mreq: bool,
    read: bool,
    m1: bool,
    refresh: bool,
    address: u16,
}

impl M1Cells {
    fn released(wait: bool) -> Self {
        Self {
            busy: false,
            busy_low: false,
            first_high: false,
            second: false,
            read_master: false,
            read_latch: true,
            read_end: false,
            read_end_high: false,
            refresh_phase: false,
            wait_high: !wait,
            wait_low: !wait,
            reset_enabled: true,
            address_enabled: false,
            clear_high: false,
            read_select: false,
            read_data: true,
            mreq: false,
            read: false,
            m1: false,
            refresh: false,
            address: 0,
        }
    }
    fn memory_boundary(address: u16, write: bool) -> Self {
        let mut cells = Self::released(false);
        cells.read_end_high = true;
        cells.address_enabled = true;
        cells.clear_high = true;
        cells.read_data = !write;
        cells.address = address;
        cells
    }
    fn final_high(address: u16) -> Self {
        Self {
            busy: false,
            busy_low: true,
            first_high: false,
            second: false,
            read_master: false,
            read_latch: true,
            read_end: false,
            read_end_high: false,
            refresh_phase: true,
            wait_high: true,
            wait_low: true,
            reset_enabled: true,
            address_enabled: true,
            clear_high: true,
            read_select: false,
            read_data: true,
            mreq: true,
            read: false,
            m1: false,
            refresh: true,
            address,
        }
    }

    fn boundary(address: u16) -> Self {
        let mut cells = Self::final_high(address);
        for _ in 0..2 {
            cells.settle(false, false, false, address, address);
        }
        cells
    }

    pub fn settle(
        &mut self,
        high: bool,
        internal_reset: bool,
        blocked: bool,
        fetch: u16,
        refresh_address: u16,
    ) {
        if high {
            self.read_master = !self.read_end && !(!self.second && self.read_latch);
            self.read_end_high = self.read_end;
        } else {
            self.read_latch = !(self.read_master && self.busy);
            self.refresh_phase = self.read_end_high && self.busy;
            self.busy_low = self.busy;
        }
        let first = !(blocked || self.busy_low);
        if high {
            self.first_high = first;
        } else {
            self.second = self.busy && self.first_high;
            self.wait_low = self.wait_high;
        }
        self.read_end = !self.read_latch && self.wait_low;
        if high {
            self.reset_enabled = !self.second;
            self.clear_high = internal_reset || self.refresh_phase;
        }
        let clear = !high && self.clear_high || high && self.read_end;
        let select = !high && self.read_select;
        if clear {
            self.mreq = false;
        }
        if select {
            self.mreq = true;
        }
        if select && self.read_data {
            self.read = true;
        }
        if clear {
            self.read = false;
        }
        if high {
            if self.read_end || blocked {
                self.m1 = false;
            }
            if first {
                self.m1 = true;
            }
            self.refresh = self.read_end || self.refresh_phase;
            self.address_enabled = !blocked;
            if first {
                self.address = fetch;
            }
            if self.read_end {
                self.address = refresh_address;
            }
            self.read_select = first || self.read_end;
            self.read_data = !self.read_end;
            self.wait_high = true;
            self.busy = !blocked && !self.refresh_phase && !internal_reset;
        }
    }

    pub fn pins(&self) -> SoundPins {
        SoundPins {
            address: self.address_enabled.then_some(self.address),
            data: None,
            mreq: self.mreq,
            read: self.read,
            write: false,
            m1: self.m1,
            refresh: self.refresh,
            iorq: false,
            bus_ack: false,
        }
    }

    pub fn quiescent(&self) -> bool {
        !self.address_enabled && !(self.mreq || self.read || self.m1 || self.refresh)
    }

    fn write_state(&self, w: &mut StateWriter) {
        for bit in [
            self.busy,
            self.busy_low,
            self.first_high,
            self.second,
            self.read_master,
            self.read_latch,
            self.read_end,
            self.read_end_high,
            self.refresh_phase,
            self.wait_high,
            self.wait_low,
            self.reset_enabled,
            self.address_enabled,
            self.clear_high,
            self.read_select,
            self.read_data,
            self.mreq,
            self.read,
            self.m1,
            self.refresh,
        ] {
            w.write_bool(bit);
        }
        w.write_u16(self.address);
    }

    fn read_state(r: &mut StateReader<'_>) -> Result<Self> {
        Ok(Self {
            busy: r.read_bool()?,
            busy_low: r.read_bool()?,
            first_high: r.read_bool()?,
            second: r.read_bool()?,
            read_master: r.read_bool()?,
            read_latch: r.read_bool()?,
            read_end: r.read_bool()?,
            read_end_high: r.read_bool()?,
            refresh_phase: r.read_bool()?,
            wait_high: r.read_bool()?,
            wait_low: r.read_bool()?,
            reset_enabled: r.read_bool()?,
            address_enabled: r.read_bool()?,
            clear_high: r.read_bool()?,
            read_select: r.read_bool()?,
            read_data: r.read_bool()?,
            mreq: r.read_bool()?,
            read: r.read_bool()?,
            m1: r.read_bool()?,
            refresh: r.read_bool()?,
            address: r.read_u16()?,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct RunningM1 {
    pub cells: M1Cells,
    pub plan: Plan,
    held_address: u16,
    pub cancelled: bool,
    pub repeat_origin: bool,
}

impl RunningM1 {
    pub fn repeat_origin_matches(&self, cells: &M1Cells) -> bool {
        self.repeat_origin
            && self.held_address == cells.address
            && *cells == M1Cells::repeat_boundary(cells.address)
    }
    pub fn after_repeat(plan: Plan, cells: M1Cells) -> Result<Self> {
        ensure!(
            cells == M1Cells::repeat_boundary(cells.address),
            "repeat reset origin differs"
        );
        Ok(Self {
            held_address: cells.address,
            cells,
            plan,
            cancelled: false,
            repeat_origin: true,
        })
    }
    pub fn validate(
        &self,
        high: bool,
        internal: bool,
        copy_high: bool,
        copy_low: bool,
    ) -> Result<()> {
        let phase = match (high, internal, copy_high, copy_low) {
            (false, false, false, false) => 0,
            (true, false, false, false) => 1,
            (false, true, false, false) => 2,
            (true, true, true, false) => 3,
            (false, true, true, true) => 4,
            _ => anyhow::bail!("invalid interrupted M1 reset phase"),
        };
        let mut cells = if self.repeat_origin {
            M1Cells::repeat_boundary(self.held_address)
        } else {
            M1Cells::boundary(self.held_address)
        };
        for step in 1..=phase {
            for _ in 0..2 {
                cells.settle(
                    step % 2 != 0,
                    step >= 2,
                    step >= 4,
                    self.plan.address,
                    self.plan.refresh,
                );
            }
        }
        ensure!(
            cells == self.cells && self.cancelled == (phase >= 2),
            "interrupted M1 pin apertures differ"
        );
        Ok(())
    }
    pub fn settle(&mut self, high: bool, internal: bool, blocked: bool) {
        self.cancelled |= internal;
        self.cells.settle(
            high,
            internal,
            blocked,
            self.plan.address,
            self.plan.refresh,
        );
    }
    pub fn write_state(&self, w: &mut StateWriter) {
        self.cells.write_state(w);
        self.plan.write_state(w);
        w.write_u16(self.held_address);
        w.write_bool(self.cancelled);
        w.write_bool(self.repeat_origin);
    }
    pub fn read_state(r: &mut StateReader<'_>) -> Result<Self> {
        let cells = M1Cells::read_state(r)?;
        let plan = Plan::read_state(r)?;
        let held_address = r.read_u16()?;
        let cancelled = r.read_bool()?;
        let repeat_origin = r.read_bool()?;
        ensure!(
            plan.shape == Shape::Fetch
                && plan.cycles == 4
                && plan.steps == 1
                && plan.address < 0x4000
                && !plan.external
                && !plan.opaque_tail
                && (plan.finishes || repeat_origin),
            "invalid interrupted local M1"
        );
        Ok(Self {
            cells,
            plan,
            held_address,
            cancelled,
            repeat_origin,
        })
    }
}
