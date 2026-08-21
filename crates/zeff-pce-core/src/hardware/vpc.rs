pub const DETERMINISTIC_VPC_RESET_REGISTERS: [u8; 8] = [0x11, 0x11, 0, 0, 0, 0, 0, 0];
pub const PROVISIONAL_VPC_WINDOW_ORIGIN_AND_THRESHOLD: bool = true;
// Hardware notes and ares disagree with this modes 1/2 compatibility table.
pub const PROVISIONAL_VPC_PRIORITY_MODE_POLICY: VpcPriorityModePolicy =
    VpcPriorityModePolicy::GeargrafxMameCompatibility;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VpcPriorityModePolicy {
    GeargrafxMameCompatibility,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VpcPort(u8);

impl VpcPort {
    #[inline]
    pub const fn from_offset(offset: u8) -> Self {
        Self(offset & 7)
    }

    #[inline]
    pub const fn offset(self) -> u8 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VpcVdc {
    One,
    Two,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VpcWindow {
    One,
    Two,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VpcWindowRegion {
    Both,
    WindowTwo,
    WindowOne,
    Neither,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VpcPixelSource {
    Background,
    Sprite,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VpcVdcPixel(u16);

impl VpcVdcPixel {
    #[inline]
    pub const fn new(bus_value: u16) -> Self {
        Self(bus_value & 0x01FF)
    }

    #[inline]
    pub const fn palette_index(self) -> u16 {
        self.0
    }

    #[inline]
    pub const fn source(self) -> VpcPixelSource {
        if self.0 & 0x0100 == 0 {
            VpcPixelSource::Background
        } else {
            VpcPixelSource::Sprite
        }
    }

    #[inline]
    pub const fn transparent(self) -> bool {
        self.0 & 0x0F == 0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VpcPixelSelection {
    Backdrop,
    VdcOne(VpcVdcPixel),
    VdcTwo(VpcVdcPixel),
}

impl VpcPixelSelection {
    #[inline]
    pub const fn palette_index(self) -> u16 {
        match self {
            Self::Backdrop => 0,
            Self::VdcOne(pixel) | Self::VdcTwo(pixel) => pixel.palette_index(),
        }
    }

    #[inline]
    pub const fn selected_vdc(self) -> Option<VpcVdc> {
        match self {
            Self::Backdrop => None,
            Self::VdcOne(_) => Some(VpcVdc::One),
            Self::VdcTwo(_) => Some(VpcVdc::Two),
        }
    }
}

#[derive(Clone, Debug)]
pub struct HuC6202 {
    priority: [u8; 2],
    windows: [u16; 2],
    direct_vdc: VpcVdc,
}

impl Default for HuC6202 {
    fn default() -> Self {
        Self::new()
    }
}

impl HuC6202 {
    #[inline]
    pub const fn new() -> Self {
        Self {
            priority: [
                DETERMINISTIC_VPC_RESET_REGISTERS[0],
                DETERMINISTIC_VPC_RESET_REGISTERS[1],
            ],
            windows: [0; 2],
            direct_vdc: VpcVdc::One,
        }
    }

    #[inline]
    pub fn reset(&mut self) {
        *self = Self::new();
    }

    #[inline]
    pub const fn priority_control_a(&self) -> u8 {
        self.priority[0]
    }

    #[inline]
    pub const fn priority_control_b(&self) -> u8 {
        self.priority[1]
    }

    #[inline]
    pub const fn window_width(&self, window: VpcWindow) -> u16 {
        match window {
            VpcWindow::One => self.windows[0],
            VpcWindow::Two => self.windows[1],
        }
    }

    #[inline]
    pub const fn direct_vdc_target(&self) -> VpcVdc {
        self.direct_vdc
    }

    #[inline]
    pub const fn read_port(&self, port: VpcPort) -> u8 {
        match port.offset() {
            0 => self.priority[0],
            1 => self.priority[1],
            2 => self.windows[0] as u8,
            3 => (self.windows[0] >> 8) as u8,
            4 => self.windows[1] as u8,
            5 => (self.windows[1] >> 8) as u8,
            _ => 0,
        }
    }

    pub fn write_port(&mut self, port: VpcPort, value: u8) {
        match port.offset() {
            0 => self.priority[0] = value,
            1 => self.priority[1] = value,
            2 => self.windows[0] = (self.windows[0] & 0x0300) | u16::from(value),
            3 => self.windows[0] = (self.windows[0] & 0x00FF) | (u16::from(value & 3) << 8),
            4 => self.windows[1] = (self.windows[1] & 0x0300) | u16::from(value),
            5 => self.windows[1] = (self.windows[1] & 0x00FF) | (u16::from(value & 3) << 8),
            6 => {
                self.direct_vdc = if value & 1 == 0 {
                    VpcVdc::One
                } else {
                    VpcVdc::Two
                };
            }
            _ => {}
        }
    }

    #[inline]
    pub const fn window_region(&self, physical_x: u16) -> VpcWindowRegion {
        match (
            window_active(self.windows[0], physical_x),
            window_active(self.windows[1], physical_x),
        ) {
            (true, true) => VpcWindowRegion::Both,
            (false, true) => VpcWindowRegion::WindowTwo,
            (true, false) => VpcWindowRegion::WindowOne,
            (false, false) => VpcWindowRegion::Neither,
        }
    }

    pub const fn select_pixel(
        &self,
        physical_x: u16,
        vdc_one: VpcVdcPixel,
        vdc_two: VpcVdcPixel,
    ) -> VpcPixelSelection {
        let setting = self.priority_setting(physical_x);
        match setting & 3 {
            0 => VpcPixelSelection::Backdrop,
            1 => VpcPixelSelection::VdcOne(vdc_one),
            2 => VpcPixelSelection::VdcTwo(vdc_two),
            _ if vdc_one.transparent() && !vdc_two.transparent() => {
                VpcPixelSelection::VdcTwo(vdc_two)
            }
            _ if !vdc_one.transparent() && vdc_two.transparent() => {
                VpcPixelSelection::VdcOne(vdc_one)
            }
            _ => match (setting >> 2) & 3 {
                1 if matches!(vdc_one.source(), VpcPixelSource::Background)
                    && matches!(vdc_two.source(), VpcPixelSource::Sprite) =>
                {
                    VpcPixelSelection::VdcTwo(vdc_two)
                }
                2 if matches!(vdc_one.source(), VpcPixelSource::Sprite)
                    && matches!(vdc_two.source(), VpcPixelSource::Background) =>
                {
                    VpcPixelSelection::VdcTwo(vdc_two)
                }
                _ => VpcPixelSelection::VdcOne(vdc_one),
            },
        }
    }

    #[inline]
    const fn priority_setting(&self, physical_x: u16) -> u8 {
        match self.window_region(physical_x) {
            VpcWindowRegion::Both => self.priority[0] & 0x0F,
            VpcWindowRegion::WindowTwo => self.priority[0] >> 4,
            VpcWindowRegion::WindowOne => self.priority[1] & 0x0F,
            VpcWindowRegion::Neither => self.priority[1] >> 4,
        }
    }
}

#[inline]
const fn window_active(width: u16, physical_x: u16) -> bool {
    width >= 0x40 && physical_x <= width - 0x40
}
