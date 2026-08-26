use super::constants::{
    GG_COLOR_CHANNEL_SCALE_4BIT, MODE4_SPRITE_TABLE_MASK, MODE4_SPRITE_TABLE_REGISTER,
    MODE4_SPRITE_TABLE_SHIFT, MODE4_SPRITE_TERMINATOR_Y, MODE4_SPRITE_X_TILE_TABLE_OFFSET,
    RGBA_CHANNELS, SMS_COLOR_CHANNEL_SCALE_2BIT, SMS_CRAM_SIZE, SMS_GG_COLOR_INDEX_MASK,
    SMS_MODE4_TILE_BYTES, SMS_NAME_TABLE_COLUMNS, SMS_NAME_TABLE_ENTRY_BYTES, SMS_NAME_TABLE_ROWS,
    SMS_SCANLINE_Z80_CYCLES, SMS_SCREEN_W, SMS_TILE_SIZE, SMS_VDP_REGISTER_COUNT,
    SMS_VISIBLE_SCANLINES, SMS_VRAM_SIZE, VDP_ADDRESS_MASK, VDP_CONTROL_CODE_SHIFT,
    VDP_CONTROL_REGISTER_WRITE_MASK, VDP_CONTROL_REGISTER_WRITE_VALUE, VDP_REG0_MODE4,
    VDP_REG1_DISPLAY_ENABLE, VDP_REG1_FRAME_IRQ_ENABLE, VDP_REG1_SPRITE_8X16,
    VDP_REG1_SPRITE_MAGNIFY, VDP_REGISTER_INDEX_MASK, VDP_REGISTER_MODE_CONTROL_2,
    VDP_STATUS_CLEAR_MASK, VDP_STATUS_SPRITE_COLLISION, VDP_STATUS_SPRITE_OVERFLOW,
    VDP_STATUS_VBLANK,
};
use super::timing::{Sega8DisplayHeight, Sega8VideoStandard};

mod render;
mod state_io;
mod timing;

const VDP_CODE_VRAM_READ: u8 = 0;
const VDP_CODE_VRAM_WRITE: u8 = 1;
const VDP_CODE_REGISTER_WRITE: u8 = 2;
const VDP_CODE_CRAM_WRITE: u8 = 3;
const MODE4_TILE_INDEX_MASK: u16 = 0x01FF;
const MODE4_TILE_HFLIP: u16 = 0x0200;
const MODE4_TILE_VFLIP: u16 = 0x0400;
const MODE4_TILE_PALETTE: u16 = 0x0800;
const MODE4_TILE_PRIORITY: u16 = 0x1000;
const MODE4_NAME_TABLE_REGISTER: usize = 2;
const MODE4_NAME_TABLE_MASK: u8 = 0x0E;
const MODE4_NAME_TABLE_SHIFT: u8 = 10;
const MODE4_EXTENDED_NAME_TABLE_MASK: u8 = 0x0C;
const MODE4_EXTENDED_NAME_TABLE_OFFSET: usize = 0x0700;
const MODE4_PATTERN_PLANES: usize = 4;
const MODE4_PATTERN_LEFT_PIXEL_MASK: u8 = 0x80;
const MODE4_PALETTE_COLOR_OFFSET: usize = 16;
const MODE4_COLOR_LOW_NIBBLE_MASK: usize = 0x0F;
const MODE4_BACKDROP_COLOR_MASK: u8 = 0x0F;
const MODE4_TRANSPARENT_COLOR: usize = 0;
const VDP_REGISTER_MODE_CONTROL_1: usize = 0;
const VDP_REGISTER_HORIZONTAL_SCROLL: usize = 8;
const VDP_REGISTER_VERTICAL_SCROLL: usize = 9;
const VDP_REGISTER_BACKDROP_COLOR: usize = 7;
const VDP_REGISTER_LINE_COUNTER: usize = 10;
const VDP_REG0_VERTICAL_SCROLL_LOCK: u8 = 0x80;
const VDP_REG0_HORIZONTAL_SCROLL_LOCK: u8 = 0x40;
const VDP_REG0_HIDE_LEFT_COLUMN: u8 = 0x20;
const VDP_REG0_LINE_IRQ_ENABLE: u8 = 0x10;
const VDP_REG0_SPRITE_SHIFT_LEFT: u8 = 0x08;
const VDP_REG0_MODE4_EXTENDED_HEIGHT: u8 = 0x02;
const VDP_REG1_MODE4_224_LINE: u8 = 0x10;
const VDP_REG1_MODE4_240_LINE: u8 = 0x08;
const MODE4_SPRITE_COUNT: usize = 64;
const MODE4_MAX_SPRITES_PER_LINE: usize = 8;
const MODE4_SPRITE_PATTERN_TABLE_REGISTER: usize = 6;
const MODE4_SPRITE_PATTERN_BASE_SELECT: u8 = 0x04;
const MODE4_SPRITE_PATTERN_BASE_HIGH: usize = 0x2000;
const TMS_REGISTER_NAME_TABLE: usize = 2;
const TMS_REGISTER_COLOR_TABLE: usize = 3;
const TMS_REGISTER_PATTERN_TABLE: usize = 4;
const TMS_REGISTER_SPRITE_ATTRIBUTE_TABLE: usize = 5;
const TMS_REGISTER_SPRITE_PATTERN_TABLE: usize = 6;
const TMS_REGISTER_TEXT_BACKDROP: usize = 7;
const TMS_REG0_MODE_GRAPHICS_II: u8 = 0x02;
const TMS_REG1_MODE_MULTICOLOR: u8 = 0x08;
const TMS_REG1_MODE_TEXT: u8 = 0x10;
const TMS_REG1_SPRITE_MAGNIFY: u8 = 0x01;
const TMS_TILE_COLUMNS: usize = 32;
const TMS_TEXT_COLUMNS: usize = 40;
const TMS_TEXT_LEFT_MARGIN: usize = 8;
const TMS_GRAPHICS_II_SECTION_TILE_ROWS: usize = 8;
const TMS_TABLE_SECTION_BYTES: usize = 0x800;
const TMS_SPRITE_COUNT: usize = 32;
const TMS_MAX_SPRITES_PER_LINE: usize = 4;
const TMS_SPRITE_ATTRIBUTE_BYTES: usize = 4;
const TMS_SPRITE_TERMINATOR_Y: u8 = 0xD0;
const TMS_SPRITE_EARLY_CLOCK: u8 = 0x80;
const TMS_COLOR_TRANSPARENT: u8 = 0;
const VDP_MAX_VISIBLE_SCANLINES: usize = 240;
const VDP_PRESENTED_FRAME_WIDTH: usize = SMS_SCREEN_W;
const VDP_PRESENTED_SCANLINE_BYTES: usize = VDP_PRESENTED_FRAME_WIDTH * RGBA_CHANNELS;
const VDP_PRESENTED_FRAMEBUFFER_LEN: usize =
    VDP_PRESENTED_SCANLINE_BYTES * VDP_MAX_VISIBLE_SCANLINES;

fn blank_presented_framebuffer() -> Box<[u8; VDP_PRESENTED_FRAMEBUFFER_LEN]> {
    vec![0; VDP_PRESENTED_FRAMEBUFFER_LEN]
        .into_boxed_slice()
        .try_into()
        .unwrap_or_else(|_| unreachable!("presented framebuffer has fixed size"))
}

const TMS9918_PALETTE: [[u8; RGBA_CHANNELS]; 16] = [
    [0x00, 0x00, 0x00, 0xFF],
    [0x00, 0x00, 0x00, 0xFF],
    [0x21, 0xC8, 0x42, 0xFF],
    [0x5E, 0xDC, 0x78, 0xFF],
    [0x54, 0x55, 0xED, 0xFF],
    [0x7D, 0x76, 0xFC, 0xFF],
    [0xD4, 0x52, 0x4D, 0xFF],
    [0x42, 0xEB, 0xF5, 0xFF],
    [0xFC, 0x55, 0x54, 0xFF],
    [0xFF, 0x79, 0x78, 0xFF],
    [0xD4, 0xC1, 0x54, 0xFF],
    [0xE6, 0xCE, 0x80, 0xFF],
    [0x21, 0xB0, 0x3B, 0xFF],
    [0xC9, 0x5B, 0xBA, 0xFF],
    [0xCC, 0xCC, 0xCC, 0xFF],
    [0xFF, 0xFF, 0xFF, 0xFF],
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Mode4Sprite {
    x: isize,
    y: isize,
    tile_index: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Mode4BackgroundPixel {
    color_index: usize,
    priority: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct TmsSprite {
    x: isize,
    y: isize,
    pattern: u8,
    color: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode4ColorMode {
    Sms,
    GameGear,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tms9918ColorMode {
    Palette,
    GameGearCram,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mode4RenderArea {
    pub width: usize,
    pub height: usize,
    pub source_x: usize,
    pub source_y: usize,
}

impl Mode4RenderArea {
    pub const fn new(width: usize, height: usize, source_x: usize, source_y: usize) -> Self {
        Self {
            width,
            height,
            source_x,
            source_y,
        }
    }

    fn expected_rgba_len(self) -> usize {
        self.width * self.height * RGBA_CHANNELS
    }
}

#[derive(Clone, Copy)]
struct Mode4SpriteRenderContext {
    area: Mode4RenderArea,
    name_table_base: usize,
    sprite_pattern_base: usize,
    sprite_scale: usize,
}

#[derive(Clone, Copy)]
struct TmsSpriteRenderContext {
    area: Mode4RenderArea,
    pattern_base: usize,
    sprite_size: usize,
    magnified: bool,
    backdrop: [u8; RGBA_CHANNELS],
    color_mode: Tms9918ColorMode,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tms9918Mode {
    GraphicsI,
    GraphicsII,
    Multicolor,
    Text,
    Invalid,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mode4VdpDebugSnapshot {
    pub enabled: bool,
    pub name_table_base: usize,
    pub sprite_table_base: usize,
    pub sprite_pattern_base: usize,
    pub horizontal_scroll: u8,
    pub vertical_scroll: u8,
    pub backdrop_color_index: usize,
    pub sprite_height: usize,
    pub sprite_width: usize,
    pub max_sprites_per_line: usize,
    pub horizontal_scroll_lock: bool,
    pub vertical_scroll_lock: bool,
    pub hide_left_column: bool,
    pub sprite_shift_left: bool,
    pub sprite_magnified: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Tms9918VdpDebugSnapshot {
    pub mode: Tms9918Mode,
    pub name_table_base: usize,
    pub pattern_table_base: usize,
    pub color_table_base: usize,
    pub sprite_attribute_table_base: usize,
    pub sprite_pattern_table_base: usize,
    pub backdrop_color: u8,
    pub text_foreground_color: u8,
    pub text_background_color: u8,
    pub sprite_size: usize,
    pub sprite_magnified: bool,
}

pub fn tms9918_palette_rgba(color: u8) -> [u8; RGBA_CHANNELS] {
    TMS9918_PALETTE[usize::from(color & 0x0F)]
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Vdp {
    video_standard: Sega8VideoStandard,
    vram: [u8; SMS_VRAM_SIZE],
    cram: [u8; SMS_CRAM_SIZE],
    registers: [u8; SMS_VDP_REGISTER_COUNT],
    address: u16,
    code: u8,
    control_latch: Option<u8>,
    read_buffer: u8,
    status: u8,
    v_counter: u8,
    h_counter: u8,
    scanline: u16,
    scanline_cycle: u32,
    scanline_display_enabled: [bool; VDP_MAX_VISIBLE_SCANLINES],
    presented_framebuffer: Box<[u8; VDP_PRESENTED_FRAMEBUFFER_LEN]>,
    presented_scanline_valid: [bool; VDP_MAX_VISIBLE_SCANLINES],
    line_counter: u8,
    line_interrupt_pending: bool,
    color_mode: Mode4ColorMode,
    gg_cram_latch: u8,
    scanline_start_registers: [u8; SMS_VDP_REGISTER_COUNT],
}

impl Vdp {
    pub fn new() -> Self {
        Self::new_with_video_standard(Sega8VideoStandard::default())
    }

    pub fn new_with_video_standard(video_standard: Sega8VideoStandard) -> Self {
        Self::new_with_video_standard_and_color_mode(video_standard, Mode4ColorMode::Sms)
    }

    pub fn new_with_video_standard_and_color_mode(
        video_standard: Sega8VideoStandard,
        color_mode: Mode4ColorMode,
    ) -> Self {
        Self {
            video_standard,
            vram: [0; SMS_VRAM_SIZE],
            cram: [0; SMS_CRAM_SIZE],
            registers: [0; SMS_VDP_REGISTER_COUNT],
            address: 0,
            code: VDP_CODE_VRAM_READ,
            control_latch: None,
            read_buffer: 0,
            status: 0,
            v_counter: 0,
            h_counter: 0,
            scanline: 0,
            scanline_cycle: 0,
            scanline_display_enabled: [false; VDP_MAX_VISIBLE_SCANLINES],
            presented_framebuffer: blank_presented_framebuffer(),
            presented_scanline_valid: [false; VDP_MAX_VISIBLE_SCANLINES],
            line_counter: 0,
            line_interrupt_pending: false,
            color_mode,
            gg_cram_latch: 0,
            scanline_start_registers: [0; SMS_VDP_REGISTER_COUNT],
        }
    }

    pub fn reset(&mut self) {
        let video_standard = self.video_standard;
        let color_mode = self.color_mode;
        *self = Self::new_with_video_standard_and_color_mode(video_standard, color_mode);
    }

    pub fn video_standard(&self) -> Sega8VideoStandard {
        self.video_standard
    }

    pub fn total_scanlines(&self) -> u16 {
        self.video_standard.total_scanlines()
    }

    pub fn set_video_standard(&mut self, video_standard: Sega8VideoStandard) {
        self.video_standard = video_standard;
        self.scanline %= self.total_scanlines();
        self.clear_presented_frame_history();
        self.update_v_counter();
    }

    pub fn set_color_mode(&mut self, color_mode: Mode4ColorMode) {
        self.color_mode = color_mode;
        self.clear_presented_frame_history();
    }

    pub fn write_data(&mut self, value: u8) {
        self.control_latch = None;
        match self.code {
            VDP_CODE_CRAM_WRITE => self.write_cram_data(value),
            VDP_CODE_VRAM_READ | VDP_CODE_VRAM_WRITE | VDP_CODE_REGISTER_WRITE => {
                let index = usize::from(self.address) % self.vram.len();
                self.vram[index] = value;
            }
            _ => unreachable!("VDP access code is always two bits"),
        }
        self.increment_address();
    }

    pub fn read_data(&mut self) -> u8 {
        self.control_latch = None;
        let value = self.read_buffer;
        self.read_buffer = self.vram[usize::from(self.address) % self.vram.len()];
        self.increment_address();
        value
    }

    pub fn write_control(&mut self, value: u8) {
        if let Some(first) = self.control_latch.take() {
            if value & VDP_CONTROL_REGISTER_WRITE_MASK == VDP_CONTROL_REGISTER_WRITE_VALUE {
                let register = usize::from(value & VDP_REGISTER_INDEX_MASK);
                self.registers[register] = first;
                self.update_scanline_start_registers_if_not_rendering_visible_line();
                self.code = VDP_CODE_REGISTER_WRITE;
                return;
            }

            self.code = value >> VDP_CONTROL_CODE_SHIFT;
            self.address = ((u16::from(value & 0x3F) << 8) | u16::from(first)) & VDP_ADDRESS_MASK;
            if self.code == VDP_CODE_VRAM_READ {
                self.read_buffer = self.vram[usize::from(self.address) % self.vram.len()];
                self.increment_address();
            }
        } else {
            self.control_latch = Some(value);
        }
    }

    pub fn read_status(&mut self) -> u8 {
        self.control_latch = None;
        let status = self.status;
        self.status &= VDP_STATUS_CLEAR_MASK;
        self.line_interrupt_pending = false;
        status
    }

    pub fn vram(&self) -> &[u8; SMS_VRAM_SIZE] {
        &self.vram
    }

    pub fn cram(&self) -> &[u8; SMS_CRAM_SIZE] {
        &self.cram
    }

    pub fn registers(&self) -> &[u8; SMS_VDP_REGISTER_COUNT] {
        &self.registers
    }

    pub fn address(&self) -> u16 {
        self.address
    }

    pub fn code(&self) -> u8 {
        self.code
    }

    pub fn status(&self) -> u8 {
        self.status
    }

    pub fn frame_interrupt_enabled(&self) -> bool {
        self.registers[VDP_REGISTER_MODE_CONTROL_2] & VDP_REG1_FRAME_IRQ_ENABLE != 0
    }

    pub fn display_enabled(&self) -> bool {
        self.registers[VDP_REGISTER_MODE_CONTROL_2] & VDP_REG1_DISPLAY_ENABLE != 0
    }

    pub fn line_interrupt_enabled(&self) -> bool {
        self.registers[VDP_REGISTER_MODE_CONTROL_1] & VDP_REG0_LINE_IRQ_ENABLE != 0
    }

    pub fn line_interrupt_pending(&self) -> bool {
        self.line_interrupt_pending
    }

    pub(crate) fn gg_cram_latch_state(&self) -> u8 {
        self.gg_cram_latch
    }

    pub(crate) fn set_gg_cram_latch_state(&mut self, latch: u8) {
        self.gg_cram_latch = latch;
    }

    pub fn interrupt_pending(&self) -> bool {
        (self.frame_interrupt_enabled() && self.status & VDP_STATUS_VBLANK != 0)
            || (self.line_interrupt_enabled() && self.line_interrupt_pending)
    }

    pub fn render_mode4_background_rgba(&self, framebuffer: &mut [u8], area: Mode4RenderArea) {
        render::render_mode4_background_rgba(self, framebuffer, area);
    }

    pub fn render_mode4_frame_rgba(
        &self,
        framebuffer: &mut [u8],
        area: Mode4RenderArea,
        color_mode: Mode4ColorMode,
    ) {
        render::render_mode4_frame_rgba(self, framebuffer, area, color_mode);
    }

    pub fn render_mode4_presented_frame_rgba(
        &self,
        framebuffer: &mut [u8],
        area: Mode4RenderArea,
        color_mode: Mode4ColorMode,
    ) {
        render::render_mode4_presented_frame_rgba(self, framebuffer, area, color_mode);
    }

    pub fn render_tms9918_frame_rgba(&self, framebuffer: &mut [u8], width: usize, height: usize) {
        self.render_tms9918_area_rgba(
            framebuffer,
            Mode4RenderArea::new(width, height, 0, 0),
            Tms9918ColorMode::Palette,
        );
    }

    pub fn render_tms9918_area_rgba(
        &self,
        framebuffer: &mut [u8],
        area: Mode4RenderArea,
        color_mode: Tms9918ColorMode,
    ) {
        render::render_tms9918_frame_rgba(self, framebuffer, area, color_mode);
    }

    pub fn render_tms9918_presented_area_rgba(
        &self,
        framebuffer: &mut [u8],
        area: Mode4RenderArea,
        color_mode: Tms9918ColorMode,
    ) {
        render::render_tms9918_presented_frame_rgba(self, framebuffer, area, color_mode);
    }

    pub fn tms9918_mode(&self) -> Tms9918Mode {
        let m1 = self.registers[VDP_REGISTER_MODE_CONTROL_2] & TMS_REG1_MODE_TEXT != 0;
        let m2 = self.registers[VDP_REGISTER_MODE_CONTROL_2] & TMS_REG1_MODE_MULTICOLOR != 0;
        let m3 = self.registers[VDP_REGISTER_MODE_CONTROL_1] & TMS_REG0_MODE_GRAPHICS_II != 0;
        match (m1, m2, m3) {
            (false, false, false) => Tms9918Mode::GraphicsI,
            (false, false, true) => Tms9918Mode::GraphicsII,
            (false, true, false) => Tms9918Mode::Multicolor,
            (true, false, false) => Tms9918Mode::Text,
            _ => Tms9918Mode::Invalid,
        }
    }

    pub fn mode4_debug_snapshot(&self) -> Mode4VdpDebugSnapshot {
        Mode4VdpDebugSnapshot {
            enabled: self.mode4_enabled(),
            name_table_base: self.mode4_name_table_base(),
            sprite_table_base: self.mode4_sprite_table_base(),
            sprite_pattern_base: self.mode4_sprite_pattern_base(),
            horizontal_scroll: self.registers[VDP_REGISTER_HORIZONTAL_SCROLL],
            vertical_scroll: self.registers[VDP_REGISTER_VERTICAL_SCROLL],
            backdrop_color_index: self.mode4_backdrop_color_index(),
            sprite_height: self.mode4_sprite_height(),
            sprite_width: self.mode4_sprite_width(),
            max_sprites_per_line: MODE4_MAX_SPRITES_PER_LINE,
            horizontal_scroll_lock: self.registers[VDP_REGISTER_MODE_CONTROL_1]
                & VDP_REG0_HORIZONTAL_SCROLL_LOCK
                != 0,
            vertical_scroll_lock: self.registers[VDP_REGISTER_MODE_CONTROL_1]
                & VDP_REG0_VERTICAL_SCROLL_LOCK
                != 0,
            hide_left_column: self.registers[VDP_REGISTER_MODE_CONTROL_1]
                & VDP_REG0_HIDE_LEFT_COLUMN
                != 0,
            sprite_shift_left: self.registers[VDP_REGISTER_MODE_CONTROL_1]
                & VDP_REG0_SPRITE_SHIFT_LEFT
                != 0,
            sprite_magnified: self.mode4_sprite_magnified(),
        }
    }

    pub fn tms9918_debug_snapshot(&self) -> Tms9918VdpDebugSnapshot {
        let mode = self.tms9918_mode();
        let text_colors = self.registers[TMS_REGISTER_TEXT_BACKDROP];
        let (pattern_table_base, color_table_base) = match mode {
            Tms9918Mode::GraphicsII => (
                self.tms_graphics_ii_pattern_table_base(),
                self.tms_graphics_ii_color_table_base(),
            ),
            _ => (self.tms_pattern_table_base(), self.tms_color_table_base()),
        };
        Tms9918VdpDebugSnapshot {
            mode,
            name_table_base: self.tms_name_table_base(),
            pattern_table_base,
            color_table_base,
            sprite_attribute_table_base: self.tms_sprite_attribute_table_base(),
            sprite_pattern_table_base: self.tms_sprite_pattern_table_base(),
            backdrop_color: text_colors & 0x0F,
            text_foreground_color: text_colors >> 4,
            text_background_color: text_colors & 0x0F,
            sprite_size: self.tms_sprite_base_size(),
            sprite_magnified: self.registers[VDP_REGISTER_MODE_CONTROL_2] & TMS_REG1_SPRITE_MAGNIFY
                != 0,
        }
    }

    fn mode4_background_pixel(
        &self,
        name_table_base: usize,
        full_x: usize,
        full_y: usize,
    ) -> Mode4BackgroundPixel {
        let h_scroll = if self.horizontal_scroll_locked_for_y(full_y) {
            0
        } else {
            usize::from(self.registers[VDP_REGISTER_HORIZONTAL_SCROLL])
        };
        let v_scroll = if self.vertical_scroll_locked_for_x(full_x) {
            0
        } else {
            usize::from(self.registers[VDP_REGISTER_VERTICAL_SCROLL])
        };
        let name_table_rows = self.mode4_name_table_rows();
        let screen_y = (full_y + v_scroll) % (name_table_rows * SMS_TILE_SIZE);
        let tile_y = (screen_y / SMS_TILE_SIZE) % name_table_rows;
        let row_in_tile = screen_y % SMS_TILE_SIZE;
        let screen_x = full_x.wrapping_sub(h_scroll) % (SMS_NAME_TABLE_COLUMNS * SMS_TILE_SIZE);
        let tile_x = (screen_x / SMS_TILE_SIZE) % SMS_NAME_TABLE_COLUMNS;
        let col_in_tile = screen_x % SMS_TILE_SIZE;
        let tile_entry = self.mode4_name_table_entry(name_table_base, tile_x, tile_y);
        let color_index = self.mode4_pattern_color(tile_entry, col_in_tile, row_in_tile);
        Mode4BackgroundPixel {
            color_index,
            priority: tile_entry & MODE4_TILE_PRIORITY != 0
                && color_index & MODE4_COLOR_LOW_NIBBLE_MASK != MODE4_TRANSPARENT_COLOR,
        }
    }

    fn clear_presented_frame_history(&mut self) {
        self.presented_scanline_valid.fill(false);
    }

    fn tms_presented_color_mode(&self) -> Tms9918ColorMode {
        match self.color_mode {
            Mode4ColorMode::Sms => Tms9918ColorMode::Palette,
            Mode4ColorMode::GameGear => Tms9918ColorMode::GameGearCram,
        }
    }

    fn horizontal_scroll_locked_for_y(&self, y: usize) -> bool {
        self.registers[VDP_REGISTER_MODE_CONTROL_1] & VDP_REG0_HORIZONTAL_SCROLL_LOCK != 0
            && y < SMS_TILE_SIZE * 2
    }

    fn vertical_scroll_locked_for_x(&self, x: usize) -> bool {
        self.registers[VDP_REGISTER_MODE_CONTROL_1] & VDP_REG0_VERTICAL_SCROLL_LOCK != 0
            && x >= 24 * SMS_TILE_SIZE
    }

    fn tms_sprite_pattern_pixel(
        &self,
        pattern_base: usize,
        pattern: u8,
        sprite_size: usize,
        x: usize,
        y: usize,
    ) -> bool {
        let base_pattern = if sprite_size == 16 {
            pattern & !0x03
        } else {
            pattern
        };
        let quadrant = if sprite_size == 16 {
            match (x >= SMS_TILE_SIZE, y >= SMS_TILE_SIZE) {
                (false, false) => 0usize,
                (false, true) => 1usize,
                (true, false) => 2usize,
                (true, true) => 3usize,
            }
        } else {
            0usize
        };
        let pattern_index = usize::from(base_pattern) + quadrant;
        let row = y % SMS_TILE_SIZE;
        let col = x % SMS_TILE_SIZE;
        let byte =
            self.vram[(pattern_base + pattern_index * SMS_TILE_SIZE + row) % self.vram.len()];
        byte & (0x80 >> col) != 0
    }

    fn tms_sprite_base_size(&self) -> usize {
        if self.registers[VDP_REGISTER_MODE_CONTROL_2] & VDP_REG1_SPRITE_8X16 != 0 {
            SMS_TILE_SIZE * 2
        } else {
            SMS_TILE_SIZE
        }
    }

    fn tms_sprite(&self, attr_base: usize, index: usize) -> Option<TmsSprite> {
        let offset = attr_base + index * TMS_SPRITE_ATTRIBUTE_BYTES;
        let y_raw = self.vram[offset % self.vram.len()];
        let y = tms_sprite_y_position(y_raw)?;
        let x_raw = self.vram[(offset + 1) % self.vram.len()];
        let pattern = self.vram[(offset + 2) % self.vram.len()];
        let tag = self.vram[(offset + 3) % self.vram.len()];
        let early_clock = tag & TMS_SPRITE_EARLY_CLOCK != 0;
        Some(TmsSprite {
            x: isize::from(x_raw) - if early_clock { 32 } else { 0 },
            y,
            pattern,
            color: tag & 0x0F,
        })
    }

    fn tms_name_table_base(&self) -> usize {
        usize::from(self.registers[TMS_REGISTER_NAME_TABLE] & 0x0F) << 10
    }

    fn tms_color_table_base(&self) -> usize {
        usize::from(self.registers[TMS_REGISTER_COLOR_TABLE]) << 6
    }

    fn tms_pattern_table_base(&self) -> usize {
        usize::from(self.registers[TMS_REGISTER_PATTERN_TABLE] & 0x07) << 11
    }

    fn tms_graphics_ii_color_table_base(&self) -> usize {
        if self.registers[TMS_REGISTER_COLOR_TABLE] & 0x80 != 0 {
            0x2000
        } else {
            0
        }
    }

    fn tms_graphics_ii_pattern_table_base(&self) -> usize {
        if self.registers[TMS_REGISTER_PATTERN_TABLE] & 0x04 != 0 {
            0x2000
        } else {
            0
        }
    }

    fn tms_sprite_attribute_table_base(&self) -> usize {
        usize::from(self.registers[TMS_REGISTER_SPRITE_ATTRIBUTE_TABLE] & 0x7F) << 7
    }

    fn tms_sprite_pattern_table_base(&self) -> usize {
        usize::from(self.registers[TMS_REGISTER_SPRITE_PATTERN_TABLE] & 0x07) << 11
    }

    fn tms_palette_color_rgba(
        &self,
        color: u8,
        color_mode: Tms9918ColorMode,
    ) -> [u8; RGBA_CHANNELS] {
        let color = color & 0x0F;
        match color_mode {
            Tms9918ColorMode::Palette => tms9918_palette_rgba(color),
            Tms9918ColorMode::GameGearCram => self.mode4_color_rgba(
                MODE4_PALETTE_COLOR_OFFSET + usize::from(color),
                Mode4ColorMode::GameGear,
            ),
        }
    }

    fn tms_backdrop_color(&self, color_mode: Tms9918ColorMode) -> [u8; RGBA_CHANNELS] {
        self.tms_palette_color_rgba(
            self.registers[TMS_REGISTER_TEXT_BACKDROP] & 0x0F,
            color_mode,
        )
    }

    fn tms_color_rgba(
        &self,
        color: u8,
        backdrop: [u8; RGBA_CHANNELS],
        color_mode: Tms9918ColorMode,
    ) -> [u8; RGBA_CHANNELS] {
        if color & 0x0F == TMS_COLOR_TRANSPARENT {
            backdrop
        } else {
            self.tms_palette_color_rgba(color, color_mode)
        }
    }

    pub fn set_status_bits(&mut self, mask: u8) {
        self.status |= mask;
    }

    pub fn set_counters(&mut self, v_counter: u8, h_counter: u8) {
        self.v_counter = v_counter;
        self.h_counter = h_counter;
    }

    pub fn v_counter(&self) -> u8 {
        self.v_counter
    }

    pub fn h_counter(&self) -> u8 {
        self.h_counter
    }

    pub fn scanline(&self) -> u16 {
        self.scanline
    }

    pub fn scanline_cycle(&self) -> u32 {
        self.scanline_cycle
    }

    pub fn line_counter(&self) -> u8 {
        self.line_counter
    }

    fn increment_address(&mut self) {
        self.address = self.address.wrapping_add(1) & VDP_ADDRESS_MASK;
    }

    fn write_cram_data(&mut self, value: u8) {
        let index = usize::from(self.address) % self.cram.len();
        match self.color_mode {
            Mode4ColorMode::Sms => {
                self.cram[index] = value;
            }
            Mode4ColorMode::GameGear => {
                if index & 1 == 0 {
                    self.gg_cram_latch = value;
                } else {
                    let low_index = index & !1;
                    self.cram[low_index] = self.gg_cram_latch;
                    self.cram[index] = value;
                }
            }
        }
    }

    fn mode4_name_table_base(&self) -> usize {
        if self.mode4_extended_height_active() {
            (usize::from(
                self.registers[MODE4_NAME_TABLE_REGISTER] & MODE4_EXTENDED_NAME_TABLE_MASK,
            ) << MODE4_NAME_TABLE_SHIFT)
                + MODE4_EXTENDED_NAME_TABLE_OFFSET
        } else {
            usize::from(self.registers[MODE4_NAME_TABLE_REGISTER] & MODE4_NAME_TABLE_MASK)
                << MODE4_NAME_TABLE_SHIFT
        }
    }

    fn mode4_name_table_rows(&self) -> usize {
        if self.mode4_extended_height_active() {
            32
        } else {
            SMS_NAME_TABLE_ROWS
        }
    }

    fn mode4_sprite_table_base(&self) -> usize {
        usize::from(self.registers[MODE4_SPRITE_TABLE_REGISTER] & MODE4_SPRITE_TABLE_MASK)
            << MODE4_SPRITE_TABLE_SHIFT
    }

    fn mode4_sprite_pattern_base(&self) -> usize {
        if self.registers[MODE4_SPRITE_PATTERN_TABLE_REGISTER] & MODE4_SPRITE_PATTERN_BASE_SELECT
            != 0
        {
            MODE4_SPRITE_PATTERN_BASE_HIGH
        } else {
            0
        }
    }

    fn mode4_name_table_entry(&self, name_table_base: usize, tile_x: usize, tile_y: usize) -> u16 {
        let offset = name_table_base
            + ((tile_y * SMS_NAME_TABLE_COLUMNS + tile_x) * SMS_NAME_TABLE_ENTRY_BYTES);
        let lo = self.vram[offset % self.vram.len()];
        let hi = self.vram[(offset + 1) % self.vram.len()];
        u16::from_le_bytes([lo, hi])
    }

    fn mode4_pattern_color(&self, tile_entry: u16, col: usize, row: usize) -> usize {
        let tile_index = usize::from(tile_entry & MODE4_TILE_INDEX_MASK);
        let pattern_col = if tile_entry & MODE4_TILE_HFLIP != 0 {
            SMS_TILE_SIZE - 1 - col
        } else {
            col
        };
        let pattern_row = if tile_entry & MODE4_TILE_VFLIP != 0 {
            SMS_TILE_SIZE - 1 - row
        } else {
            row
        };
        let pattern_base = tile_index * SMS_MODE4_TILE_BYTES + pattern_row * 4;
        let bit = MODE4_PATTERN_LEFT_PIXEL_MASK >> pattern_col;
        let mut color = MODE4_TRANSPARENT_COLOR;
        for plane in 0..MODE4_PATTERN_PLANES {
            if self.vram[(pattern_base + plane) % self.vram.len()] & bit != 0 {
                color |= 1 << plane;
            }
        }
        if tile_entry & MODE4_TILE_PALETTE != 0 {
            color + MODE4_PALETTE_COLOR_OFFSET
        } else {
            color
        }
    }

    fn sms_color_rgba(&self, color_index: usize) -> [u8; RGBA_CHANNELS] {
        let raw = self.cram[color_index & SMS_GG_COLOR_INDEX_MASK];
        [
            (raw & 0x03) * SMS_COLOR_CHANNEL_SCALE_2BIT,
            ((raw >> 2) & 0x03) * SMS_COLOR_CHANNEL_SCALE_2BIT,
            ((raw >> 4) & 0x03) * SMS_COLOR_CHANNEL_SCALE_2BIT,
            0xFF,
        ]
    }

    fn gg_color_rgba(&self, color_index: usize) -> [u8; RGBA_CHANNELS] {
        let base = (color_index & SMS_GG_COLOR_INDEX_MASK) * 2;
        let raw = u16::from_le_bytes([self.cram[base], self.cram[(base + 1) % self.cram.len()]]);
        [
            ((raw & 0x000F) as u8) * GG_COLOR_CHANNEL_SCALE_4BIT,
            (((raw >> 4) & 0x000F) as u8) * GG_COLOR_CHANNEL_SCALE_4BIT,
            (((raw >> 8) & 0x000F) as u8) * GG_COLOR_CHANNEL_SCALE_4BIT,
            0xFF,
        ]
    }

    fn mode4_color_rgba(
        &self,
        color_index: usize,
        color_mode: Mode4ColorMode,
    ) -> [u8; RGBA_CHANNELS] {
        match color_mode {
            Mode4ColorMode::Sms => self.sms_color_rgba(color_index),
            Mode4ColorMode::GameGear => self.gg_color_rgba(color_index),
        }
    }

    fn mode4_backdrop_color_index(&self) -> usize {
        MODE4_PALETTE_COLOR_OFFSET
            + usize::from(self.registers[VDP_REGISTER_BACKDROP_COLOR] & MODE4_BACKDROP_COLOR_MASK)
    }

    pub fn mode4_enabled(&self) -> bool {
        self.registers[VDP_REGISTER_MODE_CONTROL_1] & VDP_REG0_MODE4 != 0
    }

    fn mode4_display_height(&self) -> Sega8DisplayHeight {
        if !self.mode4_enabled() {
            return Sega8DisplayHeight::Lines192;
        }

        let m2 = self.registers[VDP_REGISTER_MODE_CONTROL_1] & VDP_REG0_MODE4_EXTENDED_HEIGHT != 0;
        let m3 = self.registers[VDP_REGISTER_MODE_CONTROL_2] & VDP_REG1_MODE4_240_LINE != 0;
        let m1 = self.registers[VDP_REGISTER_MODE_CONTROL_2] & VDP_REG1_MODE4_224_LINE != 0;
        match (m2, m3, m1) {
            (true, false, true) => Sega8DisplayHeight::Lines224,
            (true, true, false) => Sega8DisplayHeight::Lines240,
            _ => Sega8DisplayHeight::Lines192,
        }
    }

    fn mode4_extended_height_active(&self) -> bool {
        matches!(
            self.mode4_display_height(),
            Sega8DisplayHeight::Lines224 | Sega8DisplayHeight::Lines240
        )
    }

    fn visible_scanlines_for_timing(&self) -> u16 {
        self.mode4_display_height().lines()
    }

    fn mode4_sprite_height(&self) -> usize {
        self.mode4_sprite_base_height() * self.mode4_sprite_scale()
    }

    fn mode4_sprite_width(&self) -> usize {
        SMS_TILE_SIZE * self.mode4_sprite_scale()
    }

    fn mode4_sprite_base_height(&self) -> usize {
        if self.mode4_sprite_8x16() {
            SMS_TILE_SIZE * 2
        } else {
            SMS_TILE_SIZE
        }
    }

    fn mode4_sprite_scale(&self) -> usize {
        if self.mode4_sprite_magnified() { 2 } else { 1 }
    }

    fn mode4_sprite_magnified(&self) -> bool {
        self.registers[VDP_REGISTER_MODE_CONTROL_2] & VDP_REG1_SPRITE_MAGNIFY != 0
    }

    fn mode4_sprite_8x16(&self) -> bool {
        self.registers[VDP_REGISTER_MODE_CONTROL_2] & VDP_REG1_SPRITE_8X16 != 0
    }

    fn mode4_sprite_x_shift(&self) -> isize {
        if self.registers[VDP_REGISTER_MODE_CONTROL_1] & VDP_REG0_SPRITE_SHIFT_LEFT != 0 {
            -8
        } else {
            0
        }
    }

    fn mode4_sprite(
        &self,
        table_base: usize,
        sprite_base_height: usize,
        x_shift: isize,
        index: usize,
    ) -> Option<Mode4Sprite> {
        let y_raw = self.vram[(table_base + index) % self.vram.len()];
        if y_raw == MODE4_SPRITE_TERMINATOR_Y {
            return None;
        }
        let x_tile_offset = table_base + MODE4_SPRITE_X_TILE_TABLE_OFFSET + index * 2;
        let mut tile_index = self.vram[(x_tile_offset + 1) % self.vram.len()];
        if sprite_base_height == SMS_TILE_SIZE * 2 {
            tile_index &= !1;
        }
        Some(Mode4Sprite {
            x: isize::from(self.vram[x_tile_offset % self.vram.len()]) + x_shift,
            y: isize::from(y_raw.wrapping_add(1)),
            tile_index,
        })
    }

    fn mode4_sprite_color(
        &self,
        sprite_pattern_base: usize,
        tile_index: usize,
        col: usize,
        row: usize,
    ) -> usize {
        let pattern_base = sprite_pattern_base + tile_index * SMS_MODE4_TILE_BYTES + row * 4;
        let bit = MODE4_PATTERN_LEFT_PIXEL_MASK >> col;
        let mut color = MODE4_TRANSPARENT_COLOR;
        for plane in 0..MODE4_PATTERN_PLANES {
            if self.vram[(pattern_base + plane) % self.vram.len()] & bit != 0 {
                color |= 1 << plane;
            }
        }
        color
    }

    fn evaluate_mode4_sprite_status_for_scanline(&mut self, scanline: u16) {
        if !self.display_enabled() || scanline >= self.mode4_display_height().lines() {
            return;
        }

        let table_base = self.mode4_sprite_table_base();
        let sprite_pattern_base = self.mode4_sprite_pattern_base();
        let sprite_base_height = self.mode4_sprite_base_height();
        let sprite_scale = self.mode4_sprite_scale();
        let sprite_height = sprite_base_height * sprite_scale;
        let x_shift = self.mode4_sprite_x_shift();
        let mut sprites_on_line = 0usize;
        let mut occupied = [false; 256];
        let screen_y = isize::from(scanline as i16);

        for sprite_index in 0..MODE4_SPRITE_COUNT {
            let Some(sprite) =
                self.mode4_sprite(table_base, sprite_base_height, x_shift, sprite_index)
            else {
                break;
            };
            let Some(row) = mode4_sprite_row_for_line(sprite, sprite_height, screen_y) else {
                continue;
            };

            sprites_on_line += 1;
            if sprites_on_line > MODE4_MAX_SPRITES_PER_LINE {
                self.status |= VDP_STATUS_SPRITE_OVERFLOW;
                break;
            }

            let pattern_y = row / sprite_scale;
            let pattern_row = pattern_y % SMS_TILE_SIZE;
            let pattern_tile = usize::from(sprite.tile_index) + pattern_y / SMS_TILE_SIZE;
            for dest_col in 0..SMS_TILE_SIZE * sprite_scale {
                let screen_x = sprite.x + dest_col as isize;
                if !(0..256).contains(&screen_x) {
                    continue;
                }
                let col = dest_col / sprite_scale;
                if self.mode4_sprite_color(sprite_pattern_base, pattern_tile, col, pattern_row) == 0
                {
                    continue;
                }
                let x = screen_x as usize;
                if occupied[x] {
                    self.status |= VDP_STATUS_SPRITE_COLLISION;
                    continue;
                }
                occupied[x] = true;
            }
        }
    }

    fn evaluate_tms_sprite_status_for_scanline(&mut self, scanline: u16) {
        if !self.display_enabled()
            || scanline >= SMS_VISIBLE_SCANLINES
            || matches!(
                self.tms9918_mode(),
                Tms9918Mode::Text | Tms9918Mode::Invalid
            )
        {
            return;
        }

        let attr_base = self.tms_sprite_attribute_table_base();
        let pattern_base = self.tms_sprite_pattern_table_base();
        let sprite_size = self.tms_sprite_base_size();
        let magnified = self.registers[VDP_REGISTER_MODE_CONTROL_2] & TMS_REG1_SPRITE_MAGNIFY != 0;
        let scale = if magnified { 2usize } else { 1usize };
        let display_size = sprite_size * scale;
        let screen_y = isize::from(scanline as i16);
        let mut sprites_on_line = 0usize;
        let mut occupied = [false; 256];

        for sprite_index in 0..TMS_SPRITE_COUNT {
            let Some(sprite) = self.tms_sprite(attr_base, sprite_index) else {
                break;
            };
            if !tms_sprite_intersects_line(sprite, display_size, screen_y) {
                continue;
            }

            sprites_on_line += 1;
            if sprites_on_line > TMS_MAX_SPRITES_PER_LINE {
                self.status = (self.status & !0x1F) | (sprite_index as u8 & 0x1F);
                self.status |= VDP_STATUS_SPRITE_OVERFLOW;
                break;
            }

            let pattern_y = ((screen_y - sprite.y) as usize / scale).min(sprite_size - 1);
            for dest_col in 0..display_size {
                let screen_x = sprite.x + dest_col as isize;
                if !(0..256).contains(&screen_x) {
                    continue;
                }

                let pattern_x = (dest_col / scale).min(sprite_size - 1);
                if !self.tms_sprite_pattern_pixel(
                    pattern_base,
                    sprite.pattern,
                    sprite_size,
                    pattern_x,
                    pattern_y,
                ) {
                    continue;
                }

                let x = screen_x as usize;
                if occupied[x] {
                    self.status |= VDP_STATUS_SPRITE_COLLISION;
                    continue;
                }
                occupied[x] = true;
            }
        }
    }
}

impl Default for Vdp {
    fn default() -> Self {
        Self::new()
    }
}

fn mode4_sprite_row_for_line(
    sprite: Mode4Sprite,
    sprite_height: usize,
    screen_y: isize,
) -> Option<usize> {
    let row = screen_y - sprite.y;
    if (0..sprite_height as isize).contains(&row) {
        Some(row as usize)
    } else {
        None
    }
}

fn tms_sprite_intersects_line(sprite: TmsSprite, display_size: usize, screen_y: isize) -> bool {
    (sprite.y..sprite.y + display_size as isize).contains(&screen_y)
}

fn tms_sprite_y_position(raw: u8) -> Option<isize> {
    if raw == TMS_SPRITE_TERMINATOR_Y {
        None
    } else {
        Some(isize::from(raw as i8) + 1)
    }
}

#[cfg(test)]
mod tests;
