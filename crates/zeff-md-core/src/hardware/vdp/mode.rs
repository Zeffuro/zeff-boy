use super::constants::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct RasterGeometry {
    pub physical_y: u16,
    pub source_y: u16,
    pub interlaced: bool,
}

impl RasterGeometry {
    pub fn new(physical_y: u16, interlaced: bool, odd: bool) -> Self {
        Self {
            physical_y,
            source_y: if interlaced {
                physical_y * 2 + u16::from(odd)
            } else {
                physical_y
            },
            interlaced,
        }
    }

    pub fn progressive(y: u16) -> Self {
        Self::new(y, false, false)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum HorizontalMode {
    H32,
    H40,
}

impl HorizontalMode {
    pub fn from_register(value: u8) -> Option<Self> {
        match value & H40_CLOCK_MASK {
            0 => Some(Self::H32),
            H40_CLOCK_MASK => Some(Self::H40),
            _ => None,
        }
    }

    pub fn width(self) -> usize {
        match self {
            Self::H32 => H32_SCREEN_WIDTH,
            Self::H40 => H40_SCREEN_WIDTH,
        }
    }

    pub fn sprite_count(self) -> u8 {
        match self {
            Self::H32 => H32_SPRITE_COUNT,
            Self::H40 => H40_SPRITE_COUNT,
        }
    }

    pub fn sprites_per_line(self) -> u8 {
        match self {
            Self::H32 => H32_SPRITES_PER_LINE,
            Self::H40 => H40_SPRITES_PER_LINE,
        }
    }

    pub fn scroll_columns(self) -> u8 {
        (self.width() / WINDOW_X_STEP) as u8
    }

    pub fn window_columns(self) -> usize {
        match self {
            Self::H32 => PLANE_AXIS_SMALL,
            Self::H40 => PLANE_AXIS_MEDIUM,
        }
    }
}
