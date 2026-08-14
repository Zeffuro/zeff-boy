use anyhow::{Result, bail};

pub const POCKET_CAMERA_FRAME_BYTES: usize = 128 * 112;

#[derive(Clone, Debug, Default)]
pub struct ReplayJoypadFrame {
    pub buttons: u8,
    pub dpad: u8,
    pub buttons_p2: u8,
    pub dpad_p2: u8,
    pub zapper: ReplayZapperFrame,
    pub host_tilt: (f32, f32),
    pub camera_frame: Option<Vec<u8>>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ReplayZapperFrame {
    pub enabled: bool,
    pub trigger: bool,
    pub hit: bool,
    pub screen_pos: Option<(u16, u16)>,
}

impl PartialEq for ReplayJoypadFrame {
    fn eq(&self, other: &Self) -> bool {
        self.buttons == other.buttons
            && self.dpad == other.dpad
            && self.buttons_p2 == other.buttons_p2
            && self.dpad_p2 == other.dpad_p2
            && self.zapper == other.zapper
            && self.host_tilt.0.to_bits() == other.host_tilt.0.to_bits()
            && self.host_tilt.1.to_bits() == other.host_tilt.1.to_bits()
            && self.camera_frame == other.camera_frame
    }
}

impl Eq for ReplayJoypadFrame {}

impl ReplayJoypadFrame {
    pub fn p1(buttons: u8, dpad: u8) -> Self {
        Self {
            buttons,
            dpad,
            buttons_p2: 0,
            dpad_p2: 0,
            zapper: ReplayZapperFrame::default(),
            host_tilt: (0.0, 0.0),
            camera_frame: None,
        }
    }

    pub fn uses_host_tilt_input(&self) -> bool {
        self.host_tilt.0 != 0.0 || self.host_tilt.1 != 0.0
    }

    pub fn uses_host_camera_input(&self) -> bool {
        self.camera_frame.is_some()
    }

    pub fn uses_zapper_input(&self) -> bool {
        self.zapper.enabled
            || self.zapper.trigger
            || self.zapper.hit
            || self.zapper.screen_pos.is_some()
    }
}

impl ReplayZapperFrame {
    pub(super) fn flags(self) -> u8 {
        u8::from(self.enabled)
            | (u8::from(self.trigger) << 1)
            | (u8::from(self.hit) << 2)
            | (u8::from(self.screen_pos.is_some()) << 3)
    }

    pub(super) fn from_parts(flags: u8, x: u16, y: u16) -> Result<Self> {
        if flags & !0x0F != 0 {
            bail!("invalid replay zapper flags: {flags:#04X}");
        }
        Ok(Self {
            enabled: flags & 0x01 != 0,
            trigger: flags & 0x02 != 0,
            hit: flags & 0x04 != 0,
            screen_pos: (flags & 0x08 != 0).then_some((x, y)),
        })
    }
}
