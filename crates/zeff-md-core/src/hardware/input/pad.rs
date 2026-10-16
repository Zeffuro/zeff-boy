use crate::constants::{
    IO_TH, PAD_EXTRA_MASK, PAD_HIGH_INPUT_MASK, PAD_LOW_BUTTON_MASK, PAD_LOW_BUTTON_SHIFT,
    PAD_LOW_DIRECTION_MASK, PAD_MODE, PAD_SIX_EXTRA_RISES, PAD_SIX_ID_RISES, PAD_SIX_MAX_RISES,
};

mod state;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ControllerKind {
    #[default]
    ThreeButton,
    SixButton,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Pad {
    pub kind: ControllerKind,
    pub buttons: u8,
    pub extra: u8,
    compatibility: bool,
    rises: u8,
    last_edge: Option<u64>,
}

impl Default for Pad {
    fn default() -> Self {
        Self {
            kind: ControllerKind::ThreeButton,
            buttons: u8::MAX,
            extra: PAD_EXTRA_MASK,
            compatibility: false,
            rises: 0,
            last_edge: None,
        }
    }
}

impl Pad {
    pub fn set_extra(&mut self, value: u8) {
        self.extra = value & PAD_EXTRA_MASK;
    }

    pub fn set_kind(&mut self, kind: ControllerKind) {
        if self.kind != kind {
            self.kind = kind;
            self.compatibility = false;
            self.rises = 0;
            self.last_edge = None;
        }
    }

    pub fn power_on(&mut self) {
        let kind = self.kind;
        let compatibility = kind == ControllerKind::SixButton && self.extra & PAD_MODE == 0;
        *self = Self {
            kind,
            compatibility,
            ..Self::default()
        };
    }

    fn active_rises(&self, now: u64, timeout: u64) -> u8 {
        if self.kind != ControllerKind::SixButton
            || self.compatibility
            || self
                .last_edge
                .is_none_or(|last| now.saturating_sub(last) >= timeout)
        {
            0
        } else {
            self.rises
        }
    }

    pub fn transition(&mut self, high: bool, now: u64, timeout: u64) {
        if self.kind == ControllerKind::SixButton && !self.compatibility {
            self.rises = (self.active_rises(now, timeout) + u8::from(high)).min(PAD_SIX_MAX_RISES);
            self.last_edge = Some(now);
        }
    }

    pub fn read(&self, high: bool, now: u64, timeout: u64) -> u8 {
        let rises = self.active_rises(now, timeout);
        if high {
            let directions = if rises == PAD_SIX_EXTRA_RISES {
                self.extra
            } else {
                self.buttons & PAD_EXTRA_MASK
            };
            IO_TH | (self.buttons & (PAD_HIGH_INPUT_MASK & !PAD_EXTRA_MASK)) | directions
        } else {
            let directions = match rises {
                PAD_SIX_ID_RISES => 0,
                PAD_SIX_EXTRA_RISES => PAD_EXTRA_MASK,
                _ => self.buttons & PAD_LOW_DIRECTION_MASK,
            };
            ((self.buttons >> PAD_LOW_BUTTON_SHIFT) & PAD_LOW_BUTTON_MASK) | directions
        }
    }
}
