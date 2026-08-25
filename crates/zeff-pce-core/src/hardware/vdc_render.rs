use super::vdc::{HuC6270, VdcRegister};

const TILE_SIZE: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BackgroundColorMode {
    Full,
    PlanesZeroAndOne,
    PlanesTwoAndThree,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BackgroundRenderState {
    enabled: bool,
    scroll_x: usize,
    scroll_y: usize,
    width_tiles: usize,
    height_tiles: usize,
    color_mode: BackgroundColorMode,
}

impl BackgroundRenderState {
    pub(super) fn from_register_values(
        control: u16,
        memory_width: u16,
        scroll_x: u16,
        scroll_y: u16,
    ) -> Self {
        let width_tiles = match (memory_width >> 4) & 3 {
            0 => 32,
            1 => 64,
            _ => 128,
        };
        let height_tiles = if memory_width & 0x40 == 0 { 32 } else { 64 };
        let color_mode = if memory_width & 3 != 3 {
            BackgroundColorMode::Full
        } else if memory_width & 0x80 == 0 {
            BackgroundColorMode::PlanesZeroAndOne
        } else {
            BackgroundColorMode::PlanesTwoAndThree
        };
        Self {
            enabled: control & 0x80 != 0,
            scroll_x: usize::from(scroll_x & 0x03FF),
            scroll_y: usize::from(scroll_y & 0x01FF),
            width_tiles,
            height_tiles,
            color_mode,
        }
    }

    #[inline]
    pub const fn enabled(self) -> bool {
        self.enabled
    }

    #[inline]
    pub const fn scroll_x(self) -> usize {
        self.scroll_x
    }

    #[inline]
    pub const fn scroll_y(self) -> usize {
        self.scroll_y
    }

    #[inline]
    pub const fn width_tiles(self) -> usize {
        self.width_tiles
    }

    #[inline]
    pub const fn height_tiles(self) -> usize {
        self.height_tiles
    }

    #[inline]
    pub const fn color_mode(self) -> BackgroundColorMode {
        self.color_mode
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BackgroundScanlineStatus {
    Disabled,
    Rendered,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BackgroundRenderError {
    VramAddressUnavailable { word: u16 },
}

impl HuC6270 {
    pub fn background_render_state(&self) -> BackgroundRenderState {
        BackgroundRenderState::from_register_values(
            self.register(VdcRegister::Control),
            self.register(VdcRegister::MemoryWidth),
            self.register(VdcRegister::BackgroundScrollX),
            self.register(VdcRegister::BackgroundScrollY),
        )
    }

    pub fn render_background_scanline(
        &self,
        state: &BackgroundRenderState,
        display_line: usize,
        output: &mut [u8],
    ) -> Result<BackgroundScanlineStatus, BackgroundRenderError> {
        if !state.enabled {
            return Ok(BackgroundScanlineStatus::Disabled);
        }

        let width_pixels = state.width_tiles * TILE_SIZE;
        let height_pixels = state.height_tiles * TILE_SIZE;
        let virtual_y = (state.scroll_y + display_line) & (height_pixels - 1);
        let tile_y = virtual_y / TILE_SIZE;
        let row = virtual_y % TILE_SIZE;
        let width_mask = width_pixels - 1;
        let mut display_x = 0;
        while display_x < output.len() {
            let virtual_x = (state.scroll_x + display_x) & width_mask;
            let tile_x = virtual_x / TILE_SIZE;
            let column = virtual_x % TILE_SIZE;
            let entry = self.vram()[tile_y * state.width_tiles + tile_x];
            let (planes_zero_one, planes_two_three) =
                self.background_pattern_row(usize::from(entry & 0x0FFF), row, state.color_mode);
            let pixels = (TILE_SIZE - column).min(output.len() - display_x);
            let palette = (entry >> 8) as u8 & 0xF0;
            for offset in 0..pixels {
                let bit = 7 - column - offset;
                let pattern = ((planes_zero_one >> bit) & 1) as u8
                    | (((planes_zero_one >> (bit + 8)) & 1) as u8) << 1
                    | (((planes_two_three >> bit) & 1) as u8) << 2
                    | (((planes_two_three >> (bit + 8)) & 1) as u8) << 3;
                output[display_x + offset] = if pattern == 0 { 0 } else { palette | pattern };
            }
            display_x += pixels;
        }
        Ok(BackgroundScanlineStatus::Rendered)
    }

    fn background_pattern_row(
        &self,
        character_code: usize,
        row: usize,
        color_mode: BackgroundColorMode,
    ) -> (u16, u16) {
        let base = character_code << 4;
        match color_mode {
            BackgroundColorMode::Full => (
                self.background_pattern_word(base + row),
                self.background_pattern_word(base + 8 + row),
            ),
            BackgroundColorMode::PlanesZeroAndOne => (self.background_pattern_word(base + row), 0),
            BackgroundColorMode::PlanesTwoAndThree => {
                (0, self.background_pattern_word(base + 8 + row))
            }
        }
    }

    #[inline]
    fn background_pattern_word(&self, word: usize) -> u16 {
        self.read_logical_vram_word(word as u16)
    }
}
