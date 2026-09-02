use anyhow::{Result, bail};

pub const POCKET_CAMERA_FRAME_BYTES: usize = 128 * 112;

#[derive(Clone, Debug, Default)]
pub struct ReplayJoypadFrame {
    pub buttons: u8,
    pub dpad: u8,
    pub buttons_p2: u8,
    pub dpad_p2: u8,
    pub buttons_p3: u8,
    pub dpad_p3: u8,
    pub buttons_p4: u8,
    pub dpad_p4: u8,
    pub buttons_p5: u8,
    pub dpad_p5: u8,
    pub zapper: ReplayZapperFrame,
    pub host_tilt: (f32, f32),
    pub camera_frame: Option<Vec<u8>>,
    pub coleco: [ReplayColecoControllerFrame; 2],
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ReplayColecoControllerFrame {
    pub up: bool,
    pub right: bool,
    pub down: bool,
    pub left: bool,
    pub left_button: bool,
    pub right_button: bool,
    pub keypad: u8,
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
            && self.buttons_p3 == other.buttons_p3
            && self.dpad_p3 == other.dpad_p3
            && self.buttons_p4 == other.buttons_p4
            && self.dpad_p4 == other.dpad_p4
            && self.buttons_p5 == other.buttons_p5
            && self.dpad_p5 == other.dpad_p5
            && self.zapper == other.zapper
            && self.host_tilt.0.to_bits() == other.host_tilt.0.to_bits()
            && self.host_tilt.1.to_bits() == other.host_tilt.1.to_bits()
            && self.camera_frame == other.camera_frame
            && self.coleco == other.coleco
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
            buttons_p3: 0,
            dpad_p3: 0,
            buttons_p4: 0,
            dpad_p4: 0,
            buttons_p5: 0,
            dpad_p5: 0,
            zapper: ReplayZapperFrame::default(),
            host_tilt: (0.0, 0.0),
            camera_frame: None,
            coleco: [ReplayColecoControllerFrame::default(); 2],
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

    pub fn uses_coleco_input(&self) -> bool {
        self.coleco != [ReplayColecoControllerFrame::default(); 2]
    }

    pub fn uses_non_coleco_input(&self) -> bool {
        self.buttons != 0
            || self.dpad != 0
            || self.buttons_p2 != 0
            || self.dpad_p2 != 0
            || self.buttons_p3 != 0
            || self.dpad_p3 != 0
            || self.buttons_p4 != 0
            || self.dpad_p4 != 0
            || self.buttons_p5 != 0
            || self.dpad_p5 != 0
            || self.uses_zapper_input()
            || self.uses_host_tilt_input()
            || self.uses_host_camera_input()
    }
}

impl ReplayColecoControllerFrame {
    pub fn from_packed(value: u16) -> Result<Self> {
        if value & !0x03FF != 0 {
            bail!("invalid replay Coleco controller bits: {value:#05X}");
        }
        let keypad = (value >> 6) as u8;
        if keypad > 12 {
            bail!("invalid replay Coleco keypad key: {keypad}");
        }
        Ok(Self {
            up: value & 0x01 != 0,
            right: value & 0x02 != 0,
            down: value & 0x04 != 0,
            left: value & 0x08 != 0,
            left_button: value & 0x10 != 0,
            right_button: value & 0x20 != 0,
            keypad,
        })
    }

    pub fn packed(self) -> u16 {
        u16::from(self.up)
            | (u16::from(self.right) << 1)
            | (u16::from(self.down) << 2)
            | (u16::from(self.left) << 3)
            | (u16::from(self.left_button) << 4)
            | (u16::from(self.right_button) << 5)
            | (u16::from(self.keypad) << 6)
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
