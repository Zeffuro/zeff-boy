use super::*;

impl Bus {
    pub(super) fn hblank_timer_reload(&self) -> u16 {
        u16::from_le_bytes([
            self.io[usize::from(HBLANK_TIMER_RELOAD_LO_PORT)],
            self.io[usize::from(HBLANK_TIMER_RELOAD_HI_PORT)],
        ])
    }

    pub(super) fn set_hblank_timer_reload(&mut self, value: u16) {
        let [lo, hi] = value.to_le_bytes();
        self.io[usize::from(HBLANK_TIMER_RELOAD_LO_PORT)] = lo;
        self.io[usize::from(HBLANK_TIMER_RELOAD_HI_PORT)] = hi;
    }

    pub(super) fn hblank_timer_count(&self) -> u16 {
        u16::from_le_bytes([
            self.io[usize::from(HBLANK_TIMER_COUNT_LO_PORT)],
            self.io[usize::from(HBLANK_TIMER_COUNT_HI_PORT)],
        ])
    }

    pub(super) fn set_hblank_timer_count(&mut self, value: u16) {
        let [lo, hi] = value.to_le_bytes();
        self.io[usize::from(HBLANK_TIMER_COUNT_LO_PORT)] = lo;
        self.io[usize::from(HBLANK_TIMER_COUNT_HI_PORT)] = hi;
    }

    pub(super) fn vblank_timer_reload(&self) -> u16 {
        u16::from_le_bytes([
            self.io[usize::from(VBLANK_TIMER_RELOAD_LO_PORT)],
            self.io[usize::from(VBLANK_TIMER_RELOAD_HI_PORT)],
        ])
    }

    pub(super) fn set_vblank_timer_reload(&mut self, value: u16) {
        let [lo, hi] = value.to_le_bytes();
        self.io[usize::from(VBLANK_TIMER_RELOAD_LO_PORT)] = lo;
        self.io[usize::from(VBLANK_TIMER_RELOAD_HI_PORT)] = hi;
    }

    pub(super) fn vblank_timer_count(&self) -> u16 {
        u16::from_le_bytes([
            self.io[usize::from(VBLANK_TIMER_COUNT_LO_PORT)],
            self.io[usize::from(VBLANK_TIMER_COUNT_HI_PORT)],
        ])
    }

    pub(super) fn set_vblank_timer_count(&mut self, value: u16) {
        let [lo, hi] = value.to_le_bytes();
        self.io[usize::from(VBLANK_TIMER_COUNT_LO_PORT)] = lo;
        self.io[usize::from(VBLANK_TIMER_COUNT_HI_PORT)] = hi;
    }

    pub(super) fn step_hblank_timer(&mut self, completed_scanlines: u32) {
        let control = self.io[usize::from(TIMER_CONTROL_PORT)];
        let enabled = control & 0x01 != 0;
        let repeat = control & 0x02 != 0;
        for _ in 0..completed_scanlines {
            let count = self.hblank_timer_count();
            let next_count = count.wrapping_sub(1);
            if enabled && count != 0 {
                self.set_hblank_timer_count(next_count);
                if next_count == 0 {
                    if repeat {
                        self.set_hblank_timer_count(self.hblank_timer_reload());
                    } else {
                        self.set_hblank_timer_reload(0);
                    }
                }
            }
            if next_count == 0 {
                self.raise_interrupt(IRQ_HBLANK_TIMER);
            }
        }
    }

    pub(super) fn step_vblank_timer(&mut self) {
        let control = self.io[usize::from(TIMER_CONTROL_PORT)];
        let enabled = control & 0x04 != 0;
        let count = self.vblank_timer_count();
        let next_count = count.wrapping_sub(1);
        if enabled && count != 0 {
            self.set_vblank_timer_count(next_count);
            if next_count == 0 {
                if control & 0x08 != 0 {
                    self.set_vblank_timer_count(self.vblank_timer_reload());
                } else {
                    self.set_vblank_timer_reload(0);
                }
            }
        }
        if next_count == 0 {
            self.raise_interrupt(IRQ_VBLANK_TIMER);
        }
    }
}
