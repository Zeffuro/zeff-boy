use super::*;

impl M1Cells {
    pub(in super::super) fn write_state_copy(&self, w: &mut StateWriter) {
        self.write_state(w);
    }
    pub(in super::super) fn read_state_copy(r: &mut StateReader<'_>) -> Result<Self> {
        Self::read_state(r)
    }
    pub(in super::super) fn copy_write_boundary(address: u16) -> Self {
        let mut cells = Self::released(false);
        cells.address_enabled = true;
        cells.address = address;
        cells.read_data = false;
        cells
    }
    pub(in super::super) fn settle_repeat(&mut self, high: bool, final_high: bool) {
        if high {
            self.read_master = !self.read_end && !(!self.second && self.read_latch);
            self.read_end_high = self.read_end;
        } else {
            self.read_latch = !(self.read_master && self.busy);
            self.refresh_phase = self.read_end_high && self.busy;
            self.busy_low = self.busy;
        }
        let first = !self.busy_low;
        if high {
            self.first_high = first;
        } else {
            self.second = self.busy && self.first_high;
            self.wait_low = true;
        }
        self.read_end = !self.read_latch;
        if high {
            self.reset_enabled = true;
            self.clear_high = self.read_end || self.refresh_phase;
        }
        if !high && self.clear_high {
            self.mreq = false;
            self.read = false;
        }
        if high {
            if self.read_end {
                self.m1 = false;
            }
            self.refresh = false;
            self.address_enabled = true;
            self.read_select = first;
            self.read_data = false;
            self.wait_high = true;
            self.busy = !final_high;
        }
    }
    pub(in super::super) fn repeat_boundary(address: u16) -> Self {
        let mut cells = Self::copy_write_boundary(address);
        for stage in 1..=10 {
            for _ in 0..2 {
                cells.settle_repeat(stage % 2 != 0, stage >= 9);
            }
        }
        cells
    }
}
