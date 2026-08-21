use super::bus::{BaseBusDevices, VcePort};
use super::vdc_scanline::VceFrameLength;

pub const VCE_PALETTE_COLORS: usize = 512;
pub const VCE_UNAVAILABLE_READ_VALUE: u8 = 0xFF;
pub const DETERMINISTIC_VCE_RESET_VALUE: u16 = 0;
pub const DETERMINISTIC_VCE_INITIAL_COLOR: VceColor = VceColor::new(0);
pub const DETERMINISTIC_VCE_RESET_PRESERVES_PALETTE: bool = true;

const CONTROL_MASK: u8 = 0x87;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VcePixelClock {
    DivideByFour,
    DivideByThree,
    DivideByTwo,
}

impl VcePixelClock {
    #[inline]
    pub const fn divisor(self) -> u8 {
        match self {
            Self::DivideByFour => 4,
            Self::DivideByThree => 3,
            Self::DivideByTwo => 2,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct VceColor(u16);

impl VceColor {
    #[inline]
    pub const fn new(raw: u16) -> Self {
        Self(raw & 0x01FF)
    }

    #[inline]
    pub const fn raw(self) -> u16 {
        self.0
    }

    #[inline]
    pub const fn red(self) -> u8 {
        ((self.0 >> 3) & 7) as u8
    }

    #[inline]
    pub const fn green(self) -> u8 {
        ((self.0 >> 6) & 7) as u8
    }

    #[inline]
    pub const fn blue(self) -> u8 {
        (self.0 & 7) as u8
    }

    #[inline]
    pub const fn rgb8(self) -> [u8; 3] {
        [
            expand_component(self.red()),
            expand_component(self.green()),
            expand_component(self.blue()),
        ]
    }
}

#[inline]
const fn expand_component(value: u8) -> u8 {
    ((value as u16 * 255) / 7) as u8
}

#[derive(Debug)]
pub struct HuC6260 {
    palette: Box<[VceColor; VCE_PALETTE_COLORS]>,
    control: u8,
    color_table_address: u16,
}

impl Default for HuC6260 {
    fn default() -> Self {
        Self::new()
    }
}

impl HuC6260 {
    pub fn new() -> Self {
        Self {
            palette: Box::new([DETERMINISTIC_VCE_INITIAL_COLOR; VCE_PALETTE_COLORS]),
            control: DETERMINISTIC_VCE_RESET_VALUE as u8,
            color_table_address: DETERMINISTIC_VCE_RESET_VALUE,
        }
    }

    pub fn reset(&mut self) {
        self.control = DETERMINISTIC_VCE_RESET_VALUE as u8;
        self.color_table_address = DETERMINISTIC_VCE_RESET_VALUE;
    }

    #[inline]
    pub fn palette(&self) -> &[VceColor; VCE_PALETTE_COLORS] {
        &self.palette
    }

    #[inline]
    pub const fn control(&self) -> u8 {
        self.control
    }

    #[inline]
    pub const fn color_table_address(&self) -> u16 {
        self.color_table_address
    }

    #[inline]
    pub const fn pixel_clock(&self) -> VcePixelClock {
        match self.control & 3 {
            0 => VcePixelClock::DivideByFour,
            1 => VcePixelClock::DivideByThree,
            _ => VcePixelClock::DivideByTwo,
        }
    }

    #[inline]
    pub const fn blur_enabled(&self) -> bool {
        self.control & 0x04 != 0
    }

    #[inline]
    pub const fn frame_length(&self) -> VceFrameLength {
        if self.control & 0x04 == 0 {
            VceFrameLength::Lines262
        } else {
            VceFrameLength::Lines263
        }
    }

    #[inline]
    pub const fn monochrome_enabled(&self) -> bool {
        self.control & 0x80 != 0
    }

    #[inline]
    pub fn read_port(&mut self, port: VcePort) -> u8 {
        match port.offset() {
            4 => self.current_color().raw() as u8,
            5 => {
                let value = 0xFE | ((self.current_color().raw() >> 8) as u8);
                self.increment_color_table_address();
                value
            }
            _ => VCE_UNAVAILABLE_READ_VALUE,
        }
    }

    #[inline]
    pub fn write_port(&mut self, port: VcePort, value: u8) {
        match port.offset() {
            0 => self.control = value & CONTROL_MASK,
            2 => {
                self.color_table_address = (self.color_table_address & 0x0100) | u16::from(value);
            }
            3 => {
                self.color_table_address =
                    (self.color_table_address & 0x00FF) | (u16::from(value & 1) << 8);
            }
            4 => {
                let high = self.current_color().raw() & 0x0100;
                self.set_current_color(high | u16::from(value));
            }
            5 => {
                let low = self.current_color().raw() & 0x00FF;
                self.set_current_color(low | (u16::from(value & 1) << 8));
                self.increment_color_table_address();
            }
            _ => {}
        }
    }

    #[inline]
    fn current_color(&self) -> VceColor {
        self.palette[usize::from(self.color_table_address)]
    }

    #[inline]
    fn set_current_color(&mut self, raw: u16) {
        self.palette[usize::from(self.color_table_address)] = VceColor::new(raw);
    }

    #[inline]
    fn increment_color_table_address(&mut self) {
        self.color_table_address = self.color_table_address.wrapping_add(1) & 0x01FF;
    }
}

impl BaseBusDevices for HuC6260 {
    #[inline]
    fn read_vce(&mut self, port: VcePort) -> u8 {
        self.read_port(port)
    }

    #[inline]
    fn write_vce(&mut self, port: VcePort, value: u8) {
        self.write_port(port, value);
    }
}
