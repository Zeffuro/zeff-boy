#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct IrqCone {
    pub count: [i32; 2],
    pub overflow: [bool; 2],
    pub t5: bool,
    pub ack_delay: [bool; 2],
    pub l10: bool,
    pub l11: bool,
    pub t6: bool,
    pub a: [bool; 2],
    pub b: [bool; 2],
    pub clear: bool,
    pub hp: bool,
    pub vp: bool,
    pub sound: bool,
    pub ack_external: [bool; 2],
    pub ack_h: [bool; 2],
    pub ack_v: [bool; 2],
    pub priority_h: bool,
    pub priority_v: bool,
    pub ack_reset: bool,
    pub vint_set: bool,
    pub horizontal_zero: [bool; 2],
    pub vertical_zero: [bool; 2],
    pub vertical_last: [bool; 2],
    pub active: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct HvInputs {
    pub h: i32,
    pub v: i32,
    pub step: bool,
    pub height: bool,
    pub sound_end: bool,
}

impl IrqCone {
    #[allow(clippy::too_many_arguments)]
    pub fn eval(
        &mut self,
        d1: bool,
        d2: bool,
        h1: bool,
        h2: bool,
        ack: bool,
        m5: bool,
        old_read: bool,
        ie_h: bool,
        ie_v: bool,
        hit: u8,
        hv: HvInputs,
    ) {
        if ack {
            self.t5 = true;
        } else if self.ack_delay[1] {
            self.t5 = false;
        }
        let capture = self.t5 && m5;
        if d1 {
            self.ack_delay[0] = capture;
            self.l10 = !self.ack_delay[1];
        }
        if d2 {
            self.ack_delay[1] = self.ack_delay[0];
            self.l11 = !self.l10;
        }
        self.ack_reset = self.l11 && self.l10;
        if old_read {
            self.t6 = true;
        } else if self.b[1] {
            self.t6 = false;
        }
        let old_a_n = !self.a[1];
        if h1 {
            self.a[0] = self.t6;
            self.b[0] = self.a[1];
            self.clear = self.b[1] && old_a_n;
        }
        if h2 {
            self.a[1] = self.a[0];
            self.b[1] = self.b[0];
        }
        let clear_h = self.ack_h[1] || (self.clear && !m5);
        let clear_v = self.ack_v[1] || (self.clear && !m5);
        self.priority_v = self.vp && ie_v;
        self.priority_h = self.hp && !self.priority_v && ie_h;
        for (pair, value) in [
            (&mut self.ack_external, false),
            (&mut self.ack_h, self.priority_h),
            (&mut self.ack_v, self.priority_v),
        ] {
            if capture {
                pair[0] = value;
            } else {
                pair[1] = pair[0];
            }
            if self.ack_reset {
                *pair = [false; 2];
            }
        }
        if self.overflow[1] {
            self.hp = true;
        } else if clear_h {
            self.hp = false;
        }
        self.vint_set = self.horizontal_zero[1] && hv.height;
        if self.vint_set {
            self.vp = true;
        } else if clear_v {
            self.vp = false;
        }
        if h1 {
            let active = self.active || self.vertical_last[1];
            let base = if !active || self.overflow[1] {
                i32::from(hit)
            } else {
                self.count[1]
            };
            let total = base + i32::from(hv.step);
            self.count[0] = total & 255;
            self.overflow[0] = total & 256 != 0 && active;
        }
        if h2 {
            self.count[1] = self.count[0];
            self.overflow[1] = self.overflow[0];
        }
        if self.vint_set {
            self.sound = true;
        } else if hv.sound_end {
            self.sound = false;
        }
    }

    pub fn post_hv(&mut self, h1: bool, h2: bool, old: HvInputs, height: bool) -> bool {
        if h1 {
            self.horizontal_zero[0] = old.h == 0;
            self.vertical_zero[0] = old.v == 0;
            self.vertical_last[0] = old.v == 511;
        }
        if h2 {
            self.horizontal_zero[1] = self.horizontal_zero[0];
            self.vertical_zero[1] = self.vertical_zero[0];
            self.vertical_last[1] = self.vertical_last[0];
        }
        let memory_active = self.active || self.vertical_last[1];
        if self.vertical_zero[1] {
            self.active = true;
        } else if height {
            self.active = false;
        }
        memory_active
    }

    pub fn level(&self) -> i32 {
        if self.priority_v {
            6
        } else if self.priority_h {
            4
        } else {
            0
        }
    }
}

mod state;
