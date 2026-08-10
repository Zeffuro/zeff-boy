use super::constants::{
    GG_COLOR_CHANNEL_SCALE_4BIT, MODE4_SPRITE_TABLE_MASK, MODE4_SPRITE_TABLE_REGISTER,
    MODE4_SPRITE_TABLE_SHIFT, MODE4_SPRITE_TERMINATOR_Y, MODE4_SPRITE_X_TILE_TABLE_OFFSET,
    RGBA_CHANNELS, SMS_COLOR_CHANNEL_SCALE_2BIT, SMS_CRAM_SIZE, SMS_GG_COLOR_INDEX_MASK,
    SMS_MODE4_TILE_BYTES, SMS_NAME_TABLE_COLUMNS, SMS_NAME_TABLE_ENTRY_BYTES, SMS_NAME_TABLE_ROWS,
    SMS_SCANLINE_Z80_CYCLES, SMS_TILE_SIZE, SMS_TOTAL_SCANLINES, SMS_VDP_REGISTER_COUNT,
    SMS_VISIBLE_SCANLINES, SMS_VRAM_SIZE, VDP_ADDRESS_MASK, VDP_CONTROL_CODE_SHIFT,
    VDP_CONTROL_REGISTER_WRITE_MASK, VDP_CONTROL_REGISTER_WRITE_VALUE, VDP_REG0_MODE4,
    VDP_REG1_DISPLAY_ENABLE, VDP_REG1_FRAME_IRQ_ENABLE, VDP_REG1_SPRITE_8X16,
    VDP_REGISTER_INDEX_MASK, VDP_REGISTER_MODE_CONTROL_2, VDP_STATUS_CLEAR_MASK,
    VDP_STATUS_SPRITE_COLLISION, VDP_STATUS_SPRITE_OVERFLOW, VDP_STATUS_VBLANK,
};

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
const MODE4_SPRITE_COUNT: usize = 64;
const MODE4_MAX_SPRITES_PER_LINE: usize = 8;
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
    color_mode: Mode4ColorMode,
}

#[derive(Clone, Copy)]
struct TmsSpriteRenderContext {
    width: usize,
    pattern_base: usize,
    sprite_size: usize,
    magnified: bool,
    backdrop: [u8; RGBA_CHANNELS],
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
    pub horizontal_scroll: u8,
    pub vertical_scroll: u8,
    pub backdrop_color_index: usize,
    pub sprite_height: usize,
    pub max_sprites_per_line: usize,
    pub horizontal_scroll_lock: bool,
    pub vertical_scroll_lock: bool,
    pub hide_left_column: bool,
    pub sprite_shift_left: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Vdp {
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
    line_counter: u8,
    line_interrupt_pending: bool,
}

impl Vdp {
    pub fn new() -> Self {
        Self {
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
            line_counter: 0,
            line_interrupt_pending: false,
        }
    }

    pub fn reset(&mut self) {
        *self = Self::new();
    }

    pub fn write_data(&mut self, value: u8) {
        self.control_latch = None;
        match self.code {
            VDP_CODE_CRAM_WRITE => {
                let index = usize::from(self.address) % self.cram.len();
                self.cram[index] = value;
            }
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

    pub fn step_cycles(&mut self, cycles: u32) {
        self.scanline_cycle = self.scanline_cycle.wrapping_add(cycles);
        while self.scanline_cycle >= SMS_SCANLINE_Z80_CYCLES {
            self.scanline_cycle -= SMS_SCANLINE_Z80_CYCLES;
            self.advance_scanline();
        }
        self.h_counter = ((self.scanline_cycle * 256) / SMS_SCANLINE_Z80_CYCLES) as u8;
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

    pub fn interrupt_pending(&self) -> bool {
        (self.frame_interrupt_enabled() && self.status & VDP_STATUS_VBLANK != 0)
            || (self.line_interrupt_enabled() && self.line_interrupt_pending)
    }

    pub(crate) fn write_state(&self, w: &mut zeff_emu_common::save_state::StateWriter) {
        w.write_vec(&self.vram);
        w.write_vec(&self.cram);
        w.write_vec(&self.registers);
        w.write_u16(self.address);
        w.write_u8(self.code);
        match self.control_latch {
            Some(value) => {
                w.write_bool(true);
                w.write_u8(value);
            }
            None => {
                w.write_bool(false);
                w.write_u8(0);
            }
        }
        w.write_u8(self.read_buffer);
        w.write_u8(self.status);
        w.write_u8(self.v_counter);
        w.write_u8(self.h_counter);
        w.write_u16(self.scanline);
        w.write_u32(self.scanline_cycle);
        w.write_u8(self.line_counter);
        w.write_bool(self.line_interrupt_pending);
    }

    pub(crate) fn read_state(
        &mut self,
        r: &mut zeff_emu_common::save_state::StateReader<'_>,
    ) -> anyhow::Result<()> {
        read_fixed_vec(r, &mut self.vram, SMS_VRAM_SIZE, "VDP VRAM")?;
        read_fixed_vec(r, &mut self.cram, SMS_CRAM_SIZE, "VDP CRAM")?;
        read_fixed_vec(
            r,
            &mut self.registers,
            SMS_VDP_REGISTER_COUNT,
            "VDP registers",
        )?;
        self.address = r.read_u16()? & VDP_ADDRESS_MASK;
        self.code = r.read_u8()? & 0x03;
        self.control_latch = if r.read_bool()? {
            Some(r.read_u8()?)
        } else {
            let _unused = r.read_u8()?;
            None
        };
        self.read_buffer = r.read_u8()?;
        self.status = r.read_u8()?;
        self.v_counter = r.read_u8()?;
        self.h_counter = r.read_u8()?;
        self.scanline = r.read_u16()? % SMS_TOTAL_SCANLINES;
        self.scanline_cycle = r.read_u32()? % SMS_SCANLINE_Z80_CYCLES;
        self.line_counter = r.read_u8()?;
        self.line_interrupt_pending = r.read_bool()?;
        Ok(())
    }

    pub fn render_mode4_background_rgba(&self, framebuffer: &mut [u8], area: Mode4RenderArea) {
        self.render_mode4_background_rgba_with_color(framebuffer, area, Mode4ColorMode::Sms);
    }

    pub fn render_mode4_frame_rgba(
        &self,
        framebuffer: &mut [u8],
        area: Mode4RenderArea,
        color_mode: Mode4ColorMode,
    ) {
        if !self.display_enabled() {
            self.fill_mode4_backdrop_rgba(framebuffer, area, color_mode);
            return;
        }
        self.render_mode4_background_rgba_with_color(framebuffer, area, color_mode);
        self.render_mode4_sprites_rgba(framebuffer, area, color_mode);
        self.mask_mode4_left_column_rgba(framebuffer, area, color_mode);
    }

    pub fn render_tms9918_frame_rgba(&self, framebuffer: &mut [u8], width: usize, height: usize) {
        let expected_len = width * height * RGBA_CHANNELS;
        if framebuffer.len() < expected_len {
            return;
        }

        let backdrop = self.tms_backdrop_color();
        if !self.display_enabled() {
            self.fill_tms9918_rgba(framebuffer, width, height, backdrop);
            return;
        }

        match self.tms9918_mode() {
            Tms9918Mode::GraphicsI => self.render_tms_graphics_i_rgba(framebuffer, width, height),
            Tms9918Mode::GraphicsII => self.render_tms_graphics_ii_rgba(framebuffer, width, height),
            Tms9918Mode::Multicolor => self.render_tms_multicolor_rgba(framebuffer, width, height),
            Tms9918Mode::Text => self.render_tms_text_rgba(framebuffer, width, height),
            Tms9918Mode::Invalid => self.fill_tms9918_rgba(framebuffer, width, height, backdrop),
        }

        if !matches!(self.tms9918_mode(), Tms9918Mode::Text) {
            self.render_tms_sprites_rgba(framebuffer, width, height);
        }
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
            horizontal_scroll: self.registers[VDP_REGISTER_HORIZONTAL_SCROLL],
            vertical_scroll: self.registers[VDP_REGISTER_VERTICAL_SCROLL],
            backdrop_color_index: self.mode4_backdrop_color_index(),
            sprite_height: self.mode4_sprite_height(),
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
        }
    }

    fn render_mode4_background_rgba_with_color(
        &self,
        framebuffer: &mut [u8],
        area: Mode4RenderArea,
        color_mode: Mode4ColorMode,
    ) {
        let expected_len = area.expected_rgba_len();
        if framebuffer.len() < expected_len {
            return;
        }

        let name_table_base = self.mode4_name_table_base();
        for y in 0..area.height {
            for x in 0..area.width {
                let full_x = area.source_x + x;
                let pixel = self.mode4_background_pixel(name_table_base, full_x, area.source_y + y);
                let rgba = self.mode4_color_rgba(pixel.color_index, color_mode);
                let offset = (y * area.width + x) * RGBA_CHANNELS;
                framebuffer[offset..offset + RGBA_CHANNELS].copy_from_slice(&rgba);
            }
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
        let screen_y = (full_y + v_scroll) % (SMS_NAME_TABLE_ROWS * SMS_TILE_SIZE);
        let tile_y = (screen_y / SMS_TILE_SIZE) % SMS_NAME_TABLE_ROWS;
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

    fn horizontal_scroll_locked_for_y(&self, y: usize) -> bool {
        self.registers[VDP_REGISTER_MODE_CONTROL_1] & VDP_REG0_HORIZONTAL_SCROLL_LOCK != 0
            && y < SMS_TILE_SIZE * 2
    }

    fn vertical_scroll_locked_for_x(&self, x: usize) -> bool {
        self.registers[VDP_REGISTER_MODE_CONTROL_1] & VDP_REG0_VERTICAL_SCROLL_LOCK != 0
            && x >= 24 * SMS_TILE_SIZE
    }

    fn mask_mode4_left_column_rgba(
        &self,
        framebuffer: &mut [u8],
        area: Mode4RenderArea,
        color_mode: Mode4ColorMode,
    ) {
        if self.registers[VDP_REGISTER_MODE_CONTROL_1] & VDP_REG0_HIDE_LEFT_COLUMN == 0 {
            return;
        }
        let expected_len = area.expected_rgba_len();
        if framebuffer.len() < expected_len || area.source_x >= SMS_TILE_SIZE {
            return;
        }

        let columns = (SMS_TILE_SIZE - area.source_x).min(area.width);
        let rgba = self.mode4_color_rgba(self.mode4_backdrop_color_index(), color_mode);
        for y in 0..area.height {
            for x in 0..columns {
                let offset = (y * area.width + x) * RGBA_CHANNELS;
                framebuffer[offset..offset + RGBA_CHANNELS].copy_from_slice(&rgba);
            }
        }
    }

    fn render_tms_graphics_i_rgba(&self, framebuffer: &mut [u8], width: usize, height: usize) {
        let name_base = self.tms_name_table_base();
        let pattern_base = self.tms_pattern_table_base();
        let color_base = self.tms_color_table_base();
        let backdrop = self.tms_backdrop_color();

        for y in 0..height {
            let tile_y = y / SMS_TILE_SIZE;
            let row = y % SMS_TILE_SIZE;
            for x in 0..width {
                let tile_x = x / SMS_TILE_SIZE;
                let col = x % SMS_TILE_SIZE;
                let name_offset = tile_y * TMS_TILE_COLUMNS + tile_x;
                let pattern = self.vram[(name_base + name_offset) % self.vram.len()];
                let pattern_byte = self.vram
                    [(pattern_base + usize::from(pattern) * SMS_TILE_SIZE + row) % self.vram.len()];
                let color_byte =
                    self.vram[(color_base + usize::from(pattern >> 3)) % self.vram.len()];
                let color = if pattern_byte & (0x80 >> col) != 0 {
                    color_byte >> 4
                } else {
                    color_byte & 0x0F
                };
                let rgba = self.tms_color_rgba(color, backdrop);
                let offset = (y * width + x) * RGBA_CHANNELS;
                framebuffer[offset..offset + RGBA_CHANNELS].copy_from_slice(&rgba);
            }
        }
    }

    fn render_tms_graphics_ii_rgba(&self, framebuffer: &mut [u8], width: usize, height: usize) {
        let name_base = self.tms_name_table_base();
        let pattern_base = self.tms_graphics_ii_pattern_table_base();
        let color_base = self.tms_graphics_ii_color_table_base();
        let backdrop = self.tms_backdrop_color();

        for y in 0..height {
            let tile_y = y / SMS_TILE_SIZE;
            let section = tile_y / TMS_GRAPHICS_II_SECTION_TILE_ROWS;
            let row = y % SMS_TILE_SIZE;
            for x in 0..width {
                let tile_x = x / SMS_TILE_SIZE;
                let col = x % SMS_TILE_SIZE;
                let name_offset = tile_y * TMS_TILE_COLUMNS + tile_x;
                let pattern = self.vram[(name_base + name_offset) % self.vram.len()];
                let row_offset =
                    section * TMS_TABLE_SECTION_BYTES + usize::from(pattern) * SMS_TILE_SIZE + row;
                let pattern_byte = self.vram[(pattern_base + row_offset) % self.vram.len()];
                let color_byte = self.vram[(color_base + row_offset) % self.vram.len()];
                let color = if pattern_byte & (0x80 >> col) != 0 {
                    color_byte >> 4
                } else {
                    color_byte & 0x0F
                };
                let rgba = self.tms_color_rgba(color, backdrop);
                let offset = (y * width + x) * RGBA_CHANNELS;
                framebuffer[offset..offset + RGBA_CHANNELS].copy_from_slice(&rgba);
            }
        }
    }

    fn render_tms_multicolor_rgba(&self, framebuffer: &mut [u8], width: usize, height: usize) {
        let name_base = self.tms_name_table_base();
        let pattern_base = self.tms_pattern_table_base();
        let backdrop = self.tms_backdrop_color();

        for y in 0..height {
            let tile_y = y / SMS_TILE_SIZE;
            let color_row = (y % SMS_TILE_SIZE) / 4;
            for x in 0..width {
                let tile_x = x / SMS_TILE_SIZE;
                let color_col = (x % SMS_TILE_SIZE) / 4;
                let name_offset = tile_y * TMS_TILE_COLUMNS + tile_x;
                let pattern = self.vram[(name_base + name_offset) % self.vram.len()];
                let color_byte = self.vram[(pattern_base
                    + usize::from(pattern) * SMS_TILE_SIZE
                    + color_row * 2)
                    % self.vram.len()];
                let color = if color_col == 0 {
                    color_byte >> 4
                } else {
                    color_byte & 0x0F
                };
                let rgba = self.tms_color_rgba(color, backdrop);
                let offset = (y * width + x) * RGBA_CHANNELS;
                framebuffer[offset..offset + RGBA_CHANNELS].copy_from_slice(&rgba);
            }
        }
    }

    fn render_tms_text_rgba(&self, framebuffer: &mut [u8], width: usize, height: usize) {
        let name_base = self.tms_name_table_base();
        let pattern_base = self.tms_pattern_table_base();
        let fg = self.registers[TMS_REGISTER_TEXT_BACKDROP] >> 4;
        let bg = self.registers[TMS_REGISTER_TEXT_BACKDROP] & 0x0F;
        let backdrop = self.tms_backdrop_color();
        self.fill_tms9918_rgba(framebuffer, width, height, backdrop);

        for y in 0..height {
            let tile_y = y / SMS_TILE_SIZE;
            let row = y % SMS_TILE_SIZE;
            for text_x in 0..TMS_TEXT_COLUMNS {
                let x0 = TMS_TEXT_LEFT_MARGIN + text_x * 6;
                if x0 >= width {
                    break;
                }
                let pattern =
                    self.vram[(name_base + tile_y * TMS_TEXT_COLUMNS + text_x) % self.vram.len()];
                let pattern_byte = self.vram
                    [(pattern_base + usize::from(pattern) * SMS_TILE_SIZE + row) % self.vram.len()];
                for col in 0..6usize {
                    let x = x0 + col;
                    if x >= width {
                        continue;
                    }
                    let color = if pattern_byte & (0x80 >> col) != 0 {
                        fg
                    } else {
                        bg
                    };
                    let rgba = self.tms_color_rgba(color, backdrop);
                    let offset = (y * width + x) * RGBA_CHANNELS;
                    framebuffer[offset..offset + RGBA_CHANNELS].copy_from_slice(&rgba);
                }
            }
        }
    }

    fn render_tms_sprites_rgba(&self, framebuffer: &mut [u8], width: usize, height: usize) {
        let attr_base = self.tms_sprite_attribute_table_base();
        let pattern_base = self.tms_sprite_pattern_table_base();
        let sprite_size = self.tms_sprite_base_size();
        let magnified = self.registers[VDP_REGISTER_MODE_CONTROL_2] & TMS_REG1_SPRITE_MAGNIFY != 0;
        let display_size = if magnified {
            sprite_size * 2
        } else {
            sprite_size
        };
        let backdrop = self.tms_backdrop_color();
        let context = TmsSpriteRenderContext {
            width,
            pattern_base,
            sprite_size,
            magnified,
            backdrop,
        };

        for y in 0..height {
            let mut sprites = [None; TMS_MAX_SPRITES_PER_LINE];
            let mut count = 0usize;

            for index in 0..TMS_SPRITE_COUNT {
                let Some(sprite) = self.tms_sprite(attr_base, index) else {
                    break;
                };
                if !tms_sprite_intersects_line(sprite, display_size, y as isize) {
                    continue;
                }
                if count >= TMS_MAX_SPRITES_PER_LINE {
                    break;
                }
                sprites[count] = Some(sprite);
                count += 1;
            }

            for sprite in sprites[..count].iter().rev().flatten() {
                self.render_tms_sprite_row_rgba(framebuffer, y, *sprite, context);
            }
        }
    }

    fn render_tms_sprite_row_rgba(
        &self,
        framebuffer: &mut [u8],
        dest_y: usize,
        sprite: TmsSprite,
        context: TmsSpriteRenderContext,
    ) {
        if sprite.color == TMS_COLOR_TRANSPARENT {
            return;
        }

        let scale = if context.magnified { 2usize } else { 1usize };
        let local_y = dest_y as isize - sprite.y;
        if local_y < 0 {
            return;
        }
        let pattern_y = (local_y as usize / scale).min(context.sprite_size - 1);
        let display_size = context.sprite_size * scale;

        for dest_col in 0..display_size {
            let screen_x = sprite.x + dest_col as isize;
            if !(0..context.width as isize).contains(&screen_x) {
                continue;
            }
            let pattern_x = (dest_col / scale).min(context.sprite_size - 1);
            if !self.tms_sprite_pattern_pixel(
                context.pattern_base,
                sprite.pattern,
                context.sprite_size,
                pattern_x,
                pattern_y,
            ) {
                continue;
            }
            let rgba = self.tms_color_rgba(sprite.color, context.backdrop);
            let offset = (dest_y * context.width + screen_x as usize) * RGBA_CHANNELS;
            framebuffer[offset..offset + RGBA_CHANNELS].copy_from_slice(&rgba);
        }
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
        if y_raw == TMS_SPRITE_TERMINATOR_Y {
            return None;
        }
        let x_raw = self.vram[(offset + 1) % self.vram.len()];
        let pattern = self.vram[(offset + 2) % self.vram.len()];
        let tag = self.vram[(offset + 3) % self.vram.len()];
        let early_clock = tag & TMS_SPRITE_EARLY_CLOCK != 0;
        Some(TmsSprite {
            x: isize::from(x_raw) - if early_clock { 32 } else { 0 },
            y: isize::from(y_raw.wrapping_add(1)),
            pattern,
            color: tag & 0x0F,
        })
    }

    fn fill_tms9918_rgba(
        &self,
        framebuffer: &mut [u8],
        width: usize,
        height: usize,
        color: [u8; RGBA_CHANNELS],
    ) {
        let expected_len = width * height * RGBA_CHANNELS;
        if framebuffer.len() < expected_len {
            return;
        }
        for pixel in framebuffer[..expected_len].chunks_exact_mut(RGBA_CHANNELS) {
            pixel.copy_from_slice(&color);
        }
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

    fn tms_backdrop_color(&self) -> [u8; RGBA_CHANNELS] {
        TMS9918_PALETTE[usize::from(self.registers[TMS_REGISTER_TEXT_BACKDROP] & 0x0F)]
    }

    fn tms_color_rgba(&self, color: u8, backdrop: [u8; RGBA_CHANNELS]) -> [u8; RGBA_CHANNELS] {
        if color & 0x0F == TMS_COLOR_TRANSPARENT {
            backdrop
        } else {
            TMS9918_PALETTE[usize::from(color & 0x0F)]
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

    fn mode4_name_table_base(&self) -> usize {
        usize::from(self.registers[MODE4_NAME_TABLE_REGISTER] & MODE4_NAME_TABLE_MASK)
            << MODE4_NAME_TABLE_SHIFT
    }

    fn mode4_sprite_table_base(&self) -> usize {
        usize::from(self.registers[MODE4_SPRITE_TABLE_REGISTER] & MODE4_SPRITE_TABLE_MASK)
            << MODE4_SPRITE_TABLE_SHIFT
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

    fn fill_mode4_backdrop_rgba(
        &self,
        framebuffer: &mut [u8],
        area: Mode4RenderArea,
        color_mode: Mode4ColorMode,
    ) {
        let expected_len = area.expected_rgba_len();
        if framebuffer.len() < expected_len {
            return;
        }

        let rgba = self.mode4_color_rgba(self.mode4_backdrop_color_index(), color_mode);
        for pixel in framebuffer[..expected_len].chunks_exact_mut(RGBA_CHANNELS) {
            pixel.copy_from_slice(&rgba);
        }
    }

    fn mode4_backdrop_color_index(&self) -> usize {
        MODE4_PALETTE_COLOR_OFFSET
            + usize::from(self.registers[VDP_REGISTER_BACKDROP_COLOR] & MODE4_BACKDROP_COLOR_MASK)
    }

    fn mode4_enabled(&self) -> bool {
        self.registers[VDP_REGISTER_MODE_CONTROL_1] & VDP_REG0_MODE4 != 0
    }

    fn render_mode4_sprites_rgba(
        &self,
        framebuffer: &mut [u8],
        area: Mode4RenderArea,
        color_mode: Mode4ColorMode,
    ) {
        let expected_len = area.expected_rgba_len();
        if framebuffer.len() < expected_len {
            return;
        }

        let table_base = self.mode4_sprite_table_base();
        let name_table_base = self.mode4_name_table_base();
        let sprite_height = self.mode4_sprite_height();
        let x_shift = self.mode4_sprite_x_shift();
        let context = Mode4SpriteRenderContext {
            area,
            name_table_base,
            color_mode,
        };

        for dest_y in 0..area.height {
            let screen_y = (area.source_y + dest_y) as isize;
            let mut sprites_on_line = 0usize;
            for sprite_index in 0..MODE4_SPRITE_COUNT {
                let Some(sprite) =
                    self.mode4_sprite(table_base, sprite_height, x_shift, sprite_index)
                else {
                    break;
                };
                let Some(row) = mode4_sprite_row_for_line(sprite, sprite_height, screen_y) else {
                    continue;
                };
                if sprites_on_line >= MODE4_MAX_SPRITES_PER_LINE {
                    break;
                }
                sprites_on_line += 1;

                self.render_mode4_sprite_row_rgba(framebuffer, context, dest_y, sprite, row);
            }
        }
    }

    fn render_mode4_sprite_row_rgba(
        &self,
        framebuffer: &mut [u8],
        context: Mode4SpriteRenderContext,
        dest_y: usize,
        sprite: Mode4Sprite,
        row: usize,
    ) {
        let area = context.area;
        let pattern_row = row % SMS_TILE_SIZE;
        let pattern_tile = usize::from(sprite.tile_index) + row / SMS_TILE_SIZE;

        for col in 0..SMS_TILE_SIZE {
            let screen_x = sprite.x + col as isize;
            let dest_x = screen_x - area.source_x as isize;
            if !(0..area.width as isize).contains(&dest_x) {
                continue;
            }

            let color = self.mode4_sprite_color(pattern_tile, col, pattern_row);
            if color == 0 {
                continue;
            }
            let full_x = area.source_x + dest_x as usize;
            let full_y = area.source_y + dest_y;
            if self
                .mode4_background_pixel(context.name_table_base, full_x, full_y)
                .priority
            {
                continue;
            }
            let rgba =
                self.mode4_color_rgba(color + MODE4_PALETTE_COLOR_OFFSET, context.color_mode);
            let offset = (dest_y * area.width + dest_x as usize) * RGBA_CHANNELS;
            framebuffer[offset..offset + RGBA_CHANNELS].copy_from_slice(&rgba);
        }
    }

    fn mode4_sprite_height(&self) -> usize {
        if self.registers[VDP_REGISTER_MODE_CONTROL_2] & VDP_REG1_SPRITE_8X16 != 0 {
            SMS_TILE_SIZE * 2
        } else {
            SMS_TILE_SIZE
        }
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
        sprite_height: usize,
        x_shift: isize,
        index: usize,
    ) -> Option<Mode4Sprite> {
        let y_raw = self.vram[(table_base + index) % self.vram.len()];
        if y_raw == MODE4_SPRITE_TERMINATOR_Y {
            return None;
        }
        let x_tile_offset = table_base + MODE4_SPRITE_X_TILE_TABLE_OFFSET + index * 2;
        let mut tile_index = self.vram[(x_tile_offset + 1) % self.vram.len()];
        if sprite_height == SMS_TILE_SIZE * 2 {
            tile_index &= !1;
        }
        Some(Mode4Sprite {
            x: isize::from(self.vram[x_tile_offset % self.vram.len()]) + x_shift,
            y: isize::from(y_raw.wrapping_add(1)),
            tile_index,
        })
    }

    fn mode4_sprite_color(&self, tile_index: usize, col: usize, row: usize) -> usize {
        let pattern_base = tile_index * SMS_MODE4_TILE_BYTES + row * 4;
        let bit = MODE4_PATTERN_LEFT_PIXEL_MASK >> col;
        let mut color = MODE4_TRANSPARENT_COLOR;
        for plane in 0..MODE4_PATTERN_PLANES {
            if self.vram[(pattern_base + plane) % self.vram.len()] & bit != 0 {
                color |= 1 << plane;
            }
        }
        color
    }

    fn advance_scanline(&mut self) {
        if self.mode4_enabled() {
            self.evaluate_mode4_sprite_status_for_scanline(self.scanline);
        } else {
            self.evaluate_tms_sprite_status_for_scanline(self.scanline);
        }
        self.scanline += 1;
        if self.scanline == SMS_VISIBLE_SCANLINES {
            self.status |= VDP_STATUS_VBLANK;
        } else if self.scanline >= SMS_TOTAL_SCANLINES {
            self.scanline = 0;
            self.status &= !VDP_STATUS_VBLANK;
            self.line_counter = self.registers[VDP_REGISTER_LINE_COUNTER];
        }
        self.step_line_counter();
        self.v_counter = self.scanline as u8;
    }

    fn step_line_counter(&mut self) {
        if self.scanline >= SMS_VISIBLE_SCANLINES {
            self.line_counter = self.registers[VDP_REGISTER_LINE_COUNTER];
            return;
        }

        if self.line_counter == 0 {
            self.line_counter = self.registers[VDP_REGISTER_LINE_COUNTER];
            self.line_interrupt_pending = true;
        } else {
            self.line_counter = self.line_counter.wrapping_sub(1);
        }
    }

    fn evaluate_mode4_sprite_status_for_scanline(&mut self, scanline: u16) {
        if !self.display_enabled() || scanline >= SMS_VISIBLE_SCANLINES {
            return;
        }

        let table_base = self.mode4_sprite_table_base();
        let sprite_height = self.mode4_sprite_height();
        let x_shift = self.mode4_sprite_x_shift();
        let mut sprites_on_line = 0usize;
        let mut occupied = [false; 256];
        let screen_y = isize::from(scanline as i16);

        for sprite_index in 0..MODE4_SPRITE_COUNT {
            let Some(sprite) = self.mode4_sprite(table_base, sprite_height, x_shift, sprite_index)
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

            let pattern_row = row % SMS_TILE_SIZE;
            let pattern_tile = usize::from(sprite.tile_index) + row / SMS_TILE_SIZE;
            for col in 0..SMS_TILE_SIZE {
                let screen_x = sprite.x + col as isize;
                if !(0..256).contains(&screen_x) {
                    continue;
                }
                if self.mode4_sprite_color(pattern_tile, col, pattern_row) == 0 {
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

            if sprite.color == TMS_COLOR_TRANSPARENT {
                continue;
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

fn read_fixed_vec(
    r: &mut zeff_emu_common::save_state::StateReader<'_>,
    out: &mut [u8],
    expected_len: usize,
    label: &str,
) -> anyhow::Result<()> {
    let bytes = r.read_vec(expected_len)?;
    if bytes.len() != expected_len {
        anyhow::bail!(
            "Sega 8-bit save-state {label} size mismatch: expected {expected_len}, got {}",
            bytes.len()
        );
    }
    out.copy_from_slice(&bytes);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hardware::constants::{
        SMS_SCREEN_H, SMS_SCREEN_W, VDP_STATUS_SPRITE_COLLISION, VDP_STATUS_VBLANK,
    };

    const SMS_RED: u8 = 0x03;
    const SMS_GREEN: u8 = 0x0C;
    const SMS_RED_RGBA: [u8; RGBA_CHANNELS] = [0xFF, 0x00, 0x00, 0xFF];
    const SMS_GREEN_RGBA: [u8; RGBA_CHANNELS] = [0x00, 0xFF, 0x00, 0xFF];
    const MODE4_TEST_SPRITE_TABLE_REGISTER: u8 = 0x7E;
    const MODE4_TOP_SCANLINE_SPRITE_Y: u8 = 0xFF;

    fn set_tile_row(vdp: &mut Vdp, tile_index: usize, row: usize, planes: [u8; 4]) {
        let base = tile_index * SMS_MODE4_TILE_BYTES + row * 4;
        vdp.vram[base..base + 4].copy_from_slice(&planes);
    }

    fn render_area(
        width: usize,
        height: usize,
        source_x: usize,
        source_y: usize,
    ) -> Mode4RenderArea {
        Mode4RenderArea::new(width, height, source_x, source_y)
    }

    fn set_name_entry(vdp: &mut Vdp, tile_x: usize, tile_y: usize, entry: u16) {
        vdp.registers[MODE4_NAME_TABLE_REGISTER] = MODE4_NAME_TABLE_MASK;
        let base = vdp.mode4_name_table_base();
        let offset =
            base + ((tile_y * SMS_NAME_TABLE_COLUMNS + tile_x) * SMS_NAME_TABLE_ENTRY_BYTES);
        let [lo, hi] = entry.to_le_bytes();
        vdp.vram[offset] = lo;
        vdp.vram[offset + 1] = hi;
    }

    fn use_mode4_test_sprite_table(vdp: &mut Vdp) -> usize {
        vdp.registers[MODE4_SPRITE_TABLE_REGISTER] = MODE4_TEST_SPRITE_TABLE_REGISTER;
        vdp.mode4_sprite_table_base()
    }

    fn set_mode4_sprite(
        vdp: &mut Vdp,
        sprite_table: usize,
        sprite_index: usize,
        y_raw: u8,
        x: u8,
        tile_index: u8,
    ) {
        vdp.vram[sprite_table + sprite_index] = y_raw;
        let x_tile_offset = sprite_table + MODE4_SPRITE_X_TILE_TABLE_OFFSET + sprite_index * 2;
        vdp.vram[x_tile_offset] = x;
        vdp.vram[x_tile_offset + 1] = tile_index;
    }

    fn terminate_mode4_sprites(vdp: &mut Vdp, sprite_table: usize, sprite_index: usize) {
        vdp.vram[sprite_table + sprite_index] = MODE4_SPRITE_TERMINATOR_Y;
    }

    fn set_tms_name(vdp: &mut Vdp, tile_x: usize, tile_y: usize, pattern: u8) {
        let base = vdp.tms_name_table_base();
        vdp.vram[base + tile_y * TMS_TILE_COLUMNS + tile_x] = pattern;
    }

    fn set_tms_pattern_row(vdp: &mut Vdp, pattern_base: usize, pattern: u8, row: usize, byte: u8) {
        vdp.vram[pattern_base + usize::from(pattern) * SMS_TILE_SIZE + row] = byte;
    }

    fn set_tms_color_row(vdp: &mut Vdp, color_base: usize, pattern: u8, row: usize, byte: u8) {
        vdp.vram[color_base + usize::from(pattern) * SMS_TILE_SIZE + row] = byte;
    }

    fn set_tms_sprite(vdp: &mut Vdp, index: usize, y: isize, x: u8, pattern: u8, color: u8) {
        let base = vdp.tms_sprite_attribute_table_base() + index * TMS_SPRITE_ATTRIBUTE_BYTES;
        vdp.vram[base] = (y as u8).wrapping_sub(1);
        vdp.vram[base + 1] = x;
        vdp.vram[base + 2] = pattern;
        vdp.vram[base + 3] = color & 0x0F;
    }

    #[test]
    fn control_port_sets_vram_write_address_and_data_port_writes_vram() {
        let mut vdp = Vdp::new();

        vdp.write_control(0x34);
        vdp.write_control(0x41);
        vdp.write_data(0xAA);

        assert_eq!(vdp.vram()[0x0134], 0xAA);
        assert_eq!(vdp.address(), 0x0135);
    }

    #[test]
    fn control_port_register_write_updates_registers() {
        let mut vdp = Vdp::new();

        vdp.write_control(0xE4);
        vdp.write_control(0x82);

        assert_eq!(vdp.registers()[2], 0xE4);
        assert_eq!(vdp.code(), VDP_CODE_REGISTER_WRITE);
    }

    #[test]
    fn control_port_sets_cram_write_address_and_data_port_writes_cram() {
        let mut vdp = Vdp::new();

        vdp.write_control(0x03);
        vdp.write_control(0xC0);
        vdp.write_data(0x2A);

        assert_eq!(vdp.cram()[3], 0x2A);
        assert_eq!(vdp.address(), 4);
    }

    #[test]
    fn data_reads_use_vdp_read_buffer_and_increment_address() {
        let mut vdp = Vdp::new();
        vdp.write_control(0x00);
        vdp.write_control(0x40);
        vdp.write_data(0x11);
        vdp.write_data(0x22);
        vdp.write_control(0x00);
        vdp.write_control(0x00);

        assert_eq!(vdp.read_data(), 0x11);
        assert_eq!(vdp.read_data(), 0x22);
    }

    #[test]
    fn status_read_clears_latched_status_bits() {
        let mut vdp = Vdp::new();

        vdp.set_status_bits(VDP_STATUS_VBLANK | VDP_STATUS_SPRITE_COLLISION | 0x03);

        assert_eq!(
            vdp.read_status(),
            VDP_STATUS_VBLANK | VDP_STATUS_SPRITE_COLLISION | 0x03
        );
        assert_eq!(vdp.status(), 0x03);
    }

    #[test]
    fn frame_interrupt_pending_requires_enable_and_vblank_status() {
        let mut vdp = Vdp::new();

        vdp.set_status_bits(VDP_STATUS_VBLANK);
        assert!(!vdp.interrupt_pending());

        vdp.write_control(VDP_REG1_FRAME_IRQ_ENABLE);
        vdp.write_control(VDP_CONTROL_REGISTER_WRITE_VALUE | VDP_REGISTER_MODE_CONTROL_2 as u8);
        assert!(vdp.frame_interrupt_enabled());
        assert!(vdp.interrupt_pending());

        assert_eq!(vdp.read_status() & VDP_STATUS_VBLANK, VDP_STATUS_VBLANK);
        assert!(!vdp.interrupt_pending());
    }

    #[test]
    fn line_interrupt_counter_asserts_irq_and_status_read_clears_it() {
        let mut vdp = Vdp::new();

        vdp.write_control(VDP_REG0_LINE_IRQ_ENABLE);
        vdp.write_control(VDP_CONTROL_REGISTER_WRITE_VALUE | VDP_REGISTER_MODE_CONTROL_1 as u8);
        vdp.write_control(0);
        vdp.write_control(VDP_CONTROL_REGISTER_WRITE_VALUE | VDP_REGISTER_LINE_COUNTER as u8);

        vdp.step_cycles(SMS_SCANLINE_Z80_CYCLES);

        assert!(vdp.line_interrupt_enabled());
        assert!(vdp.line_interrupt_pending());
        assert!(vdp.interrupt_pending());
        assert_eq!(vdp.read_status() & VDP_STATUS_VBLANK, 0);
        assert!(!vdp.line_interrupt_pending());
        assert!(!vdp.interrupt_pending());
    }

    #[test]
    fn mode4_debug_snapshot_decodes_layout_and_register_flags() {
        let mut vdp = Vdp::new();

        vdp.registers[VDP_REGISTER_MODE_CONTROL_1] = VDP_REG0_MODE4
            | VDP_REG0_HORIZONTAL_SCROLL_LOCK
            | VDP_REG0_VERTICAL_SCROLL_LOCK
            | VDP_REG0_HIDE_LEFT_COLUMN
            | VDP_REG0_SPRITE_SHIFT_LEFT;
        vdp.registers[VDP_REGISTER_MODE_CONTROL_2] = VDP_REG1_SPRITE_8X16;
        vdp.registers[MODE4_NAME_TABLE_REGISTER] = MODE4_NAME_TABLE_MASK;
        vdp.registers[MODE4_SPRITE_TABLE_REGISTER] = MODE4_TEST_SPRITE_TABLE_REGISTER;
        vdp.registers[VDP_REGISTER_HORIZONTAL_SCROLL] = 13;
        vdp.registers[VDP_REGISTER_VERTICAL_SCROLL] = 21;
        vdp.registers[VDP_REGISTER_BACKDROP_COLOR] = 5;

        let snapshot = vdp.mode4_debug_snapshot();

        assert!(snapshot.enabled);
        assert_eq!(
            snapshot.name_table_base,
            usize::from(MODE4_NAME_TABLE_MASK) << MODE4_NAME_TABLE_SHIFT
        );
        assert_eq!(
            snapshot.sprite_table_base,
            usize::from(MODE4_TEST_SPRITE_TABLE_REGISTER & MODE4_SPRITE_TABLE_MASK)
                << MODE4_SPRITE_TABLE_SHIFT
        );
        assert_eq!(snapshot.horizontal_scroll, 13);
        assert_eq!(snapshot.vertical_scroll, 21);
        assert_eq!(
            snapshot.backdrop_color_index,
            MODE4_PALETTE_COLOR_OFFSET + 5
        );
        assert_eq!(snapshot.sprite_height, SMS_TILE_SIZE * 2);
        assert_eq!(snapshot.max_sprites_per_line, MODE4_MAX_SPRITES_PER_LINE);
        assert!(snapshot.horizontal_scroll_lock);
        assert!(snapshot.vertical_scroll_lock);
        assert!(snapshot.hide_left_column);
        assert!(snapshot.sprite_shift_left);
    }

    #[test]
    fn mode4_background_renderer_decodes_tile_pixels_and_sms_cram() {
        let mut vdp = Vdp::new();
        let mut framebuffer = vec![0; SMS_TILE_SIZE * SMS_TILE_SIZE * RGBA_CHANNELS];

        set_tile_row(&mut vdp, 1, 0, [0x80, 0x80, 0x00, 0x00]);
        set_name_entry(&mut vdp, 0, 0, 1);
        vdp.cram[0] = 0x00;
        vdp.cram[3] = 0x03;

        vdp.render_mode4_background_rgba(
            &mut framebuffer,
            render_area(SMS_TILE_SIZE, SMS_TILE_SIZE, 0, 0),
        );

        assert_eq!(&framebuffer[0..RGBA_CHANNELS], &[0xFF, 0x00, 0x00, 0xFF]);
        assert_eq!(
            &framebuffer[RGBA_CHANNELS..RGBA_CHANNELS * 2],
            &[0x00, 0x00, 0x00, 0xFF]
        );
    }

    #[test]
    fn mode4_background_renderer_honors_flips_and_palette_bit() {
        let mut vdp = Vdp::new();
        let mut framebuffer = vec![0; SMS_TILE_SIZE * SMS_TILE_SIZE * RGBA_CHANNELS];

        set_tile_row(&mut vdp, 2, 7, [0x01, 0x00, 0x00, 0x00]);
        set_name_entry(
            &mut vdp,
            0,
            0,
            2 | MODE4_TILE_HFLIP | MODE4_TILE_VFLIP | MODE4_TILE_PALETTE,
        );
        vdp.cram[17] = 0x0C;

        vdp.render_mode4_background_rgba(
            &mut framebuffer,
            render_area(SMS_TILE_SIZE, SMS_TILE_SIZE, 0, 0),
        );

        assert_eq!(&framebuffer[0..RGBA_CHANNELS], &[0x00, 0xFF, 0x00, 0xFF]);
    }

    #[test]
    fn mode4_background_renderer_applies_global_scroll_registers() {
        let mut vdp = Vdp::new();
        let mut framebuffer = vec![0; SMS_TILE_SIZE * SMS_TILE_SIZE * RGBA_CHANNELS];

        set_tile_row(&mut vdp, 3, 0, [0x80, 0x80, 0x00, 0x00]);
        set_name_entry(&mut vdp, 31, 0, 3);
        vdp.registers[VDP_REGISTER_HORIZONTAL_SCROLL] = 8;
        vdp.cram[3] = 0x03;

        vdp.render_mode4_background_rgba(
            &mut framebuffer,
            render_area(SMS_TILE_SIZE, SMS_TILE_SIZE, 0, 0),
        );

        assert_eq!(&framebuffer[0..RGBA_CHANNELS], &[0xFF, 0x00, 0x00, 0xFF]);
    }

    #[test]
    fn mode4_background_renderer_honors_top_row_horizontal_scroll_lock() {
        let mut vdp = Vdp::new();
        let mut framebuffer = vec![0; SMS_TILE_SIZE * SMS_TILE_SIZE * RGBA_CHANNELS];

        set_tile_row(&mut vdp, 1, 0, [0x80, 0x00, 0x00, 0x00]);
        set_tile_row(&mut vdp, 2, 0, [0x80, 0x80, 0x00, 0x00]);
        set_name_entry(&mut vdp, 0, 0, 1);
        set_name_entry(&mut vdp, 31, 0, 2);
        vdp.registers[VDP_REGISTER_MODE_CONTROL_1] = VDP_REG0_HORIZONTAL_SCROLL_LOCK;
        vdp.registers[VDP_REGISTER_HORIZONTAL_SCROLL] = 8;
        vdp.cram[1] = 0x03;
        vdp.cram[3] = 0x0C;

        vdp.render_mode4_background_rgba(
            &mut framebuffer,
            render_area(SMS_TILE_SIZE, SMS_TILE_SIZE, 0, 0),
        );

        assert_eq!(&framebuffer[0..RGBA_CHANNELS], &[0xFF, 0x00, 0x00, 0xFF]);
    }

    #[test]
    fn mode4_background_renderer_honors_right_column_vertical_scroll_lock() {
        let mut vdp = Vdp::new();
        let mut framebuffer = vec![0; SMS_TILE_SIZE * SMS_TILE_SIZE * RGBA_CHANNELS];

        set_tile_row(&mut vdp, 1, 0, [0x80, 0x00, 0x00, 0x00]);
        set_tile_row(&mut vdp, 2, 0, [0x80, 0x80, 0x00, 0x00]);
        set_name_entry(&mut vdp, 24, 0, 1);
        set_name_entry(&mut vdp, 24, 1, 2);
        vdp.registers[VDP_REGISTER_MODE_CONTROL_1] = VDP_REG0_VERTICAL_SCROLL_LOCK;
        vdp.registers[VDP_REGISTER_VERTICAL_SCROLL] = 8;
        vdp.cram[1] = 0x03;
        vdp.cram[3] = 0x0C;

        vdp.render_mode4_background_rgba(
            &mut framebuffer,
            render_area(SMS_TILE_SIZE, SMS_TILE_SIZE, 192, 0),
        );

        assert_eq!(&framebuffer[0..RGBA_CHANNELS], &[0xFF, 0x00, 0x00, 0xFF]);
    }

    #[test]
    fn mode4_frame_renderer_decodes_game_gear_cram() {
        let mut vdp = Vdp::new();
        let mut framebuffer = vec![0; SMS_TILE_SIZE * SMS_TILE_SIZE * RGBA_CHANNELS];

        vdp.registers[VDP_REGISTER_MODE_CONTROL_2] = VDP_REG1_DISPLAY_ENABLE;
        set_tile_row(&mut vdp, 1, 0, [0x80, 0x80, 0x00, 0x00]);
        set_name_entry(&mut vdp, 0, 0, 1);
        vdp.vram[0] = MODE4_SPRITE_TERMINATOR_Y;
        vdp.cram[6] = 0x0F;
        vdp.cram[7] = 0x00;

        vdp.render_mode4_frame_rgba(
            &mut framebuffer,
            render_area(SMS_TILE_SIZE, SMS_TILE_SIZE, 0, 0),
            Mode4ColorMode::GameGear,
        );

        assert_eq!(&framebuffer[0..RGBA_CHANNELS], &[0xFF, 0x00, 0x00, 0xFF]);
    }

    #[test]
    fn mode4_frame_renderer_draws_nonzero_sprites_over_background() {
        let mut vdp = Vdp::new();
        let mut framebuffer = vec![0; SMS_TILE_SIZE * SMS_TILE_SIZE * RGBA_CHANNELS];

        vdp.registers[VDP_REGISTER_MODE_CONTROL_2] = VDP_REG1_DISPLAY_ENABLE;
        set_tile_row(&mut vdp, 4, 0, [0x80, 0x00, 0x00, 0x00]);
        vdp.registers[MODE4_SPRITE_TABLE_REGISTER] = 0x7E;
        let sprite_table = vdp.mode4_sprite_table_base();
        vdp.vram[sprite_table] = 0xFF;
        vdp.vram[sprite_table + 1] = MODE4_SPRITE_TERMINATOR_Y;
        vdp.vram[sprite_table + MODE4_SPRITE_X_TILE_TABLE_OFFSET] = 0;
        vdp.vram[sprite_table + MODE4_SPRITE_X_TILE_TABLE_OFFSET + 1] = 4;
        vdp.cram[17] = 0x03;

        vdp.render_mode4_frame_rgba(
            &mut framebuffer,
            render_area(SMS_TILE_SIZE, SMS_TILE_SIZE, 0, 0),
            Mode4ColorMode::Sms,
        );

        assert_eq!(&framebuffer[0..RGBA_CHANNELS], &[0xFF, 0x00, 0x00, 0xFF]);
    }

    #[test]
    fn mode4_frame_renderer_honors_priority_background_pixels_over_sprites() {
        let mut vdp = Vdp::new();
        let mut framebuffer = vec![0; SMS_TILE_SIZE * SMS_TILE_SIZE * RGBA_CHANNELS];

        vdp.registers[VDP_REGISTER_MODE_CONTROL_2] = VDP_REG1_DISPLAY_ENABLE;
        set_tile_row(&mut vdp, 1, 0, [MODE4_PATTERN_LEFT_PIXEL_MASK, 0, 0, 0]);
        set_name_entry(&mut vdp, 0, 0, 1 | MODE4_TILE_PRIORITY);
        set_tile_row(&mut vdp, 4, 0, [MODE4_PATTERN_LEFT_PIXEL_MASK, 0, 0, 0]);
        let sprite_table = use_mode4_test_sprite_table(&mut vdp);
        set_mode4_sprite(&mut vdp, sprite_table, 0, MODE4_TOP_SCANLINE_SPRITE_Y, 0, 4);
        terminate_mode4_sprites(&mut vdp, sprite_table, 1);
        vdp.cram[1] = SMS_GREEN;
        vdp.cram[MODE4_PALETTE_COLOR_OFFSET + 1] = SMS_RED;

        vdp.render_mode4_frame_rgba(
            &mut framebuffer,
            render_area(SMS_TILE_SIZE, SMS_TILE_SIZE, 0, 0),
            Mode4ColorMode::Sms,
        );

        assert_eq!(&framebuffer[0..RGBA_CHANNELS], &SMS_GREEN_RGBA);
    }

    #[test]
    fn mode4_frame_renderer_draws_sprites_over_transparent_priority_background_pixels() {
        let mut vdp = Vdp::new();
        let mut framebuffer = vec![0; SMS_TILE_SIZE * SMS_TILE_SIZE * RGBA_CHANNELS];

        vdp.registers[VDP_REGISTER_MODE_CONTROL_2] = VDP_REG1_DISPLAY_ENABLE;
        set_name_entry(&mut vdp, 0, 0, 1 | MODE4_TILE_PALETTE | MODE4_TILE_PRIORITY);
        set_tile_row(&mut vdp, 4, 0, [MODE4_PATTERN_LEFT_PIXEL_MASK, 0, 0, 0]);
        let sprite_table = use_mode4_test_sprite_table(&mut vdp);
        set_mode4_sprite(&mut vdp, sprite_table, 0, MODE4_TOP_SCANLINE_SPRITE_Y, 0, 4);
        terminate_mode4_sprites(&mut vdp, sprite_table, 1);
        vdp.cram[MODE4_PALETTE_COLOR_OFFSET] = SMS_GREEN;
        vdp.cram[MODE4_PALETTE_COLOR_OFFSET + 1] = SMS_RED;

        vdp.render_mode4_frame_rgba(
            &mut framebuffer,
            render_area(SMS_TILE_SIZE, SMS_TILE_SIZE, 0, 0),
            Mode4ColorMode::Sms,
        );

        assert_eq!(&framebuffer[0..RGBA_CHANNELS], &SMS_RED_RGBA);
    }

    #[test]
    fn mode4_frame_renderer_blanks_to_backdrop_when_display_disabled() {
        let mut vdp = Vdp::new();
        let mut framebuffer = vec![0; SMS_TILE_SIZE * SMS_TILE_SIZE * RGBA_CHANNELS];

        set_tile_row(&mut vdp, 1, 0, [0x80, 0x80, 0x00, 0x00]);
        set_name_entry(&mut vdp, 0, 0, 1);
        vdp.registers[7] = 1;
        vdp.cram[17] = 0x0C;

        vdp.render_mode4_frame_rgba(
            &mut framebuffer,
            render_area(SMS_TILE_SIZE, SMS_TILE_SIZE, 0, 0),
            Mode4ColorMode::Sms,
        );

        assert_eq!(&framebuffer[0..RGBA_CHANNELS], &[0x00, 0xFF, 0x00, 0xFF]);
    }

    #[test]
    fn mode4_frame_renderer_masks_left_column_to_backdrop() {
        let mut vdp = Vdp::new();
        let mut framebuffer = vec![0; SMS_TILE_SIZE * 2 * SMS_TILE_SIZE * RGBA_CHANNELS];

        vdp.registers[VDP_REGISTER_MODE_CONTROL_1] = VDP_REG0_HIDE_LEFT_COLUMN;
        vdp.registers[VDP_REGISTER_MODE_CONTROL_2] = VDP_REG1_DISPLAY_ENABLE;
        vdp.registers[MODE4_SPRITE_TABLE_REGISTER] = 0x7E;
        let sprite_table = vdp.mode4_sprite_table_base();
        vdp.vram[sprite_table] = MODE4_SPRITE_TERMINATOR_Y;
        set_tile_row(&mut vdp, 1, 0, [0xFF, 0x00, 0x00, 0x00]);
        set_name_entry(&mut vdp, 0, 0, 1);
        set_name_entry(&mut vdp, 1, 0, 1);
        vdp.registers[7] = 2;
        vdp.cram[1] = 0x03;
        vdp.cram[18] = 0x0C;

        vdp.render_mode4_frame_rgba(
            &mut framebuffer,
            render_area(SMS_TILE_SIZE * 2, SMS_TILE_SIZE, 0, 0),
            Mode4ColorMode::Sms,
        );

        assert_eq!(&framebuffer[0..RGBA_CHANNELS], &[0x00, 0xFF, 0x00, 0xFF]);
        let unmasked = SMS_TILE_SIZE * RGBA_CHANNELS;
        assert_eq!(
            &framebuffer[unmasked..unmasked + RGBA_CHANNELS],
            &[0xFF, 0x00, 0x00, 0xFF]
        );
    }

    #[test]
    fn mode4_sprite_status_latches_collision_and_overflow_on_scanline() {
        let mut vdp = Vdp::new();

        vdp.registers[VDP_REGISTER_MODE_CONTROL_1] = VDP_REG0_MODE4;
        vdp.registers[VDP_REGISTER_MODE_CONTROL_2] = VDP_REG1_DISPLAY_ENABLE;
        vdp.registers[MODE4_SPRITE_TABLE_REGISTER] = 0x7E;
        let sprite_table = vdp.mode4_sprite_table_base();
        set_tile_row(&mut vdp, 4, 0, [0x80, 0x00, 0x00, 0x00]);
        for sprite in 0..9usize {
            vdp.vram[sprite_table + sprite] = 0xFF;
            let xt = sprite_table + MODE4_SPRITE_X_TILE_TABLE_OFFSET + sprite * 2;
            vdp.vram[xt] = 0;
            vdp.vram[xt + 1] = 4;
        }
        vdp.vram[sprite_table + 9] = MODE4_SPRITE_TERMINATOR_Y;

        vdp.step_cycles(SMS_SCANLINE_Z80_CYCLES);

        assert_eq!(
            vdp.status() & VDP_STATUS_SPRITE_COLLISION,
            VDP_STATUS_SPRITE_COLLISION
        );
        assert_eq!(
            vdp.status() & VDP_STATUS_SPRITE_OVERFLOW,
            VDP_STATUS_SPRITE_OVERFLOW
        );
    }

    #[test]
    fn mode4_frame_renderer_limits_to_eight_sprites_per_line() {
        let mut vdp = Vdp::new();
        let mut framebuffer = vec![0; SMS_TILE_SIZE * SMS_TILE_SIZE * RGBA_CHANNELS];

        vdp.registers[VDP_REGISTER_MODE_CONTROL_2] = VDP_REG1_DISPLAY_ENABLE;
        vdp.registers[MODE4_SPRITE_TABLE_REGISTER] = 0x7E;
        let sprite_table = vdp.mode4_sprite_table_base();
        set_tile_row(&mut vdp, 1, 0, [0x80, 0x00, 0x00, 0x00]);
        vdp.cram[17] = 0x03;
        for sprite in 0..8usize {
            vdp.vram[sprite_table + sprite] = 0xFF;
            let xt = sprite_table + MODE4_SPRITE_X_TILE_TABLE_OFFSET + sprite * 2;
            vdp.vram[xt] = 240;
            vdp.vram[xt + 1] = 0;
        }
        vdp.vram[sprite_table + 8] = 0xFF;
        let ninth_xt = sprite_table + MODE4_SPRITE_X_TILE_TABLE_OFFSET + 8 * 2;
        vdp.vram[ninth_xt] = 0;
        vdp.vram[ninth_xt + 1] = 1;
        vdp.vram[sprite_table + 9] = MODE4_SPRITE_TERMINATOR_Y;

        vdp.render_mode4_frame_rgba(
            &mut framebuffer,
            render_area(SMS_TILE_SIZE, SMS_TILE_SIZE, 0, 0),
            Mode4ColorMode::Sms,
        );

        assert_eq!(&framebuffer[0..RGBA_CHANNELS], &[0x00, 0x00, 0x00, 0xFF]);
    }

    #[test]
    fn tms9918_graphics_i_renderer_uses_pattern_name_and_color_tables() {
        let mut vdp = Vdp::new();
        let mut framebuffer = vec![0; SMS_TILE_SIZE * SMS_TILE_SIZE * RGBA_CHANNELS];

        vdp.registers[VDP_REGISTER_MODE_CONTROL_2] = VDP_REG1_DISPLAY_ENABLE;
        vdp.registers[TMS_REGISTER_NAME_TABLE] = 0x0E;
        vdp.registers[TMS_REGISTER_COLOR_TABLE] = 0x20;
        vdp.registers[TMS_REGISTER_PATTERN_TABLE] = 0x00;
        vdp.registers[TMS_REGISTER_TEXT_BACKDROP] = 0x01;
        set_tms_name(&mut vdp, 0, 0, 1);
        set_tms_pattern_row(&mut vdp, 0, 1, 0, 0x80);
        vdp.vram[vdp.tms_color_table_base()] = 0x60;

        vdp.render_tms9918_frame_rgba(&mut framebuffer, SMS_TILE_SIZE, SMS_TILE_SIZE);

        assert_eq!(&framebuffer[0..RGBA_CHANNELS], &[0xD4, 0x52, 0x4D, 0xFF]);
        assert_eq!(
            &framebuffer[RGBA_CHANNELS..RGBA_CHANNELS * 2],
            &[0x00, 0x00, 0x00, 0xFF]
        );
    }

    #[test]
    fn tms9918_graphics_ii_renderer_uses_sectioned_pattern_and_color_tables() {
        let mut vdp = Vdp::new();
        let mut framebuffer = vec![0; SMS_SCREEN_W * SMS_SCREEN_H * RGBA_CHANNELS];

        vdp.registers[VDP_REGISTER_MODE_CONTROL_1] = TMS_REG0_MODE_GRAPHICS_II;
        vdp.registers[VDP_REGISTER_MODE_CONTROL_2] = VDP_REG1_DISPLAY_ENABLE;
        vdp.registers[TMS_REGISTER_NAME_TABLE] = 0x0E;
        vdp.registers[TMS_REGISTER_COLOR_TABLE] = 0x80;
        vdp.registers[TMS_REGISTER_PATTERN_TABLE] = 0x00;
        vdp.registers[TMS_REGISTER_TEXT_BACKDROP] = 0x01;
        set_tms_name(&mut vdp, 0, 8, 2);
        set_tms_pattern_row(&mut vdp, TMS_TABLE_SECTION_BYTES, 2, 0, 0x80);
        set_tms_color_row(&mut vdp, 0x2000 + TMS_TABLE_SECTION_BYTES, 2, 0, 0x50);

        vdp.render_tms9918_frame_rgba(&mut framebuffer, SMS_SCREEN_W, SMS_SCREEN_H);

        assert_eq!(&framebuffer[0..RGBA_CHANNELS], &[0x00, 0x00, 0x00, 0xFF]);
        let section_1_pixel = (SMS_TILE_SIZE * 8 * SMS_SCREEN_W) * RGBA_CHANNELS;
        assert_eq!(
            &framebuffer[section_1_pixel..section_1_pixel + RGBA_CHANNELS],
            &[0x7D, 0x76, 0xFC, 0xFF]
        );
    }

    #[test]
    fn tms9918_text_renderer_draws_six_pixel_wide_characters() {
        let mut vdp = Vdp::new();
        let mut framebuffer = vec![0; SMS_SCREEN_W * SMS_TILE_SIZE * RGBA_CHANNELS];

        vdp.registers[VDP_REGISTER_MODE_CONTROL_2] = VDP_REG1_DISPLAY_ENABLE | TMS_REG1_MODE_TEXT;
        vdp.registers[TMS_REGISTER_NAME_TABLE] = 0x0E;
        vdp.registers[TMS_REGISTER_PATTERN_TABLE] = 0x00;
        vdp.registers[TMS_REGISTER_TEXT_BACKDROP] = 0xF1;
        vdp.vram[vdp.tms_name_table_base()] = 3;
        set_tms_pattern_row(&mut vdp, 0, 3, 0, 0x80);

        vdp.render_tms9918_frame_rgba(&mut framebuffer, SMS_SCREEN_W, SMS_TILE_SIZE);

        let text_pixel = TMS_TEXT_LEFT_MARGIN * RGBA_CHANNELS;
        assert_eq!(
            &framebuffer[text_pixel..text_pixel + RGBA_CHANNELS],
            &[0xFF, 0xFF, 0xFF, 0xFF]
        );
        assert_eq!(&framebuffer[0..RGBA_CHANNELS], &[0x00, 0x00, 0x00, 0xFF]);
    }

    #[test]
    fn tms9918_renderer_draws_basic_sprites_over_background() {
        let mut vdp = Vdp::new();
        let mut framebuffer = vec![0; SMS_TILE_SIZE * SMS_TILE_SIZE * RGBA_CHANNELS];

        vdp.registers[VDP_REGISTER_MODE_CONTROL_2] = VDP_REG1_DISPLAY_ENABLE;
        vdp.registers[TMS_REGISTER_SPRITE_ATTRIBUTE_TABLE] = 0x00;
        vdp.registers[TMS_REGISTER_SPRITE_PATTERN_TABLE] = 0x01;
        let pattern_base = vdp.tms_sprite_pattern_table_base();
        set_tms_pattern_row(&mut vdp, pattern_base, 4, 0, 0x80);
        vdp.vram[0] = 0xFF;
        vdp.vram[1] = 0;
        vdp.vram[2] = 4;
        vdp.vram[3] = 6;
        vdp.vram[4] = TMS_SPRITE_TERMINATOR_Y;

        vdp.render_tms9918_frame_rgba(&mut framebuffer, SMS_TILE_SIZE, SMS_TILE_SIZE);

        assert_eq!(&framebuffer[0..RGBA_CHANNELS], &[0xD4, 0x52, 0x4D, 0xFF]);
    }

    #[test]
    fn tms9918_sprite_status_latches_collision_and_fifth_sprite() {
        let mut vdp = Vdp::new();

        vdp.registers[VDP_REGISTER_MODE_CONTROL_2] = VDP_REG1_DISPLAY_ENABLE;
        vdp.registers[TMS_REGISTER_SPRITE_ATTRIBUTE_TABLE] = 0x00;
        vdp.registers[TMS_REGISTER_SPRITE_PATTERN_TABLE] = 0x01;
        let pattern_base = vdp.tms_sprite_pattern_table_base();
        for pattern in 0..5 {
            set_tms_pattern_row(&mut vdp, pattern_base, pattern, 0, 0x80);
        }
        set_tms_sprite(&mut vdp, 0, 0, 10, 0, 2);
        set_tms_sprite(&mut vdp, 1, 0, 10, 1, 3);
        set_tms_sprite(&mut vdp, 2, 0, 30, 2, 4);
        set_tms_sprite(&mut vdp, 3, 0, 40, 3, 5);
        set_tms_sprite(&mut vdp, 4, 0, 50, 4, 6);
        vdp.vram[5 * TMS_SPRITE_ATTRIBUTE_BYTES] = TMS_SPRITE_TERMINATOR_Y;

        vdp.step_cycles(SMS_SCANLINE_Z80_CYCLES);

        assert_eq!(
            vdp.status() & VDP_STATUS_SPRITE_COLLISION,
            VDP_STATUS_SPRITE_COLLISION
        );
        assert_eq!(
            vdp.status() & VDP_STATUS_SPRITE_OVERFLOW,
            VDP_STATUS_SPRITE_OVERFLOW
        );
        assert_eq!(vdp.status() & 0x1F, 4);
    }

    #[test]
    fn stepping_cycles_advances_counters_and_latches_vblank() {
        let mut vdp = Vdp::new();

        vdp.step_cycles(SMS_SCANLINE_Z80_CYCLES - 1);
        assert_eq!(vdp.scanline(), 0);
        assert_ne!(vdp.h_counter(), 0);

        vdp.step_cycles(1);
        assert_eq!(vdp.scanline(), 1);
        assert_eq!(vdp.v_counter(), 1);

        vdp.step_cycles(SMS_SCANLINE_Z80_CYCLES * u32::from(SMS_VISIBLE_SCANLINES - 1));
        assert_eq!(vdp.scanline(), SMS_VISIBLE_SCANLINES);
        assert_eq!(vdp.status() & VDP_STATUS_VBLANK, VDP_STATUS_VBLANK);
        assert_eq!(vdp.read_status() & VDP_STATUS_VBLANK, VDP_STATUS_VBLANK);
        assert_eq!(vdp.status() & VDP_STATUS_VBLANK, 0);

        vdp.step_cycles(
            SMS_SCANLINE_Z80_CYCLES * u32::from(SMS_TOTAL_SCANLINES - SMS_VISIBLE_SCANLINES),
        );
        assert_eq!(vdp.scanline(), 0);
    }
}
