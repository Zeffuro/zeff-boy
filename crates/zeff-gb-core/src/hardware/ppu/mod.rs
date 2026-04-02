mod palette;
mod renderer;
mod sgb;
mod sprite;
mod state;
mod tiles;
mod timing;

use std::fmt;

pub use palette::DmgPalettePreset;
pub use palette::PALETTE_COLORS;
pub use palette::apply_dmg_palette;
pub use palette::apply_palette;
pub use palette::cgb_palette_rgba;
pub use palette::correct_color;
pub use palette::dmg_palette_colors;
pub use sprite::SpriteEntry;
pub use tiles::decode_tile_pixel;
pub use tiles::tile_data_address;

pub const SCREEN_W: usize = 160;
pub const SCREEN_H: usize = 144;

pub const SGB_BORDER_W: usize = 256;
pub const SGB_BORDER_H: usize = 224;
pub const SGB_BORDER_SIZE: usize = SGB_BORDER_W * SGB_BORDER_H * 4;

pub const SGB_TRN_TRANSFER_SIZE: usize = 0x1000;
pub const SGB_CHR_TRANSFER_SIZE: usize = SGB_TRN_TRANSFER_SIZE * 2;
pub const SGB_ATTR_BLOCKS_W: usize = 20;
pub const SGB_ATTR_BLOCKS_H: usize = 18;
pub const SGB_ATTR_MAP_SIZE: usize = SGB_ATTR_BLOCKS_W * SGB_ATTR_BLOCKS_H;
pub const SGB_BORDER_TILEMAP_W: usize = 32;
pub const SGB_BORDER_TILEMAP_H: usize = 32;
pub const SGB_BORDER_TILEMAP_SIZE: usize = SGB_BORDER_TILEMAP_W * SGB_BORDER_TILEMAP_H;
pub const SGB_BORDER_PALETTES: usize = 8;
pub const SGB_BORDER_COLORS_PER_PALETTE: usize = 16;

const DOTS_PER_LINE: u64 = 456;
const OAM_DOTS: u64 = 80;
const DRAW_DOTS_BASE: u64 = 172;
bitflags::bitflags! {
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Lcdc: u8 {
        const BG_ENABLE      = 0x01;
        const OBJ_ENABLE     = 0x02;
        const OBJ_SIZE       = 0x04;
        const BG_TILEMAP     = 0x08;
        const TILE_DATA      = 0x10;
        const WINDOW_ENABLE  = 0x20;
        const WINDOW_TILEMAP = 0x40;
        const LCD_ENABLE     = 0x80;
    }
}

fn default_framebuffer() -> Box<[u8]> {
    vec![0; SCREEN_W * SCREEN_H * 4].into_boxed_slice()
}

fn default_cgb_palette_ram() -> [u8; 64] {
    let mut ram = [0u8; 64];
    let shades = [0x7FFFu16, 0x56B5u16, 0x2D6Bu16, 0x0000u16];
    for palette in 0..8 {
        for (color, shade) in shades.iter().enumerate() {
            let base = palette * 8 + color * 2;
            ram[base] = (*shade & 0x00FF) as u8;
            ram[base + 1] = (*shade >> 8) as u8;
        }
    }
    ram
}

#[derive(Clone, Copy, Debug)]
pub struct PpuDebugFlags {
    pub bg: bool,
    pub window: bool,
    pub sprites: bool,
}

impl Default for PpuDebugFlags {
    fn default() -> Self {
        Self {
            bg: true,
            window: true,
            sprites: true,
        }
    }
}

pub struct PPU {
    pub lcdc: Lcdc,
    pub stat: u8,
    pub scy: u8,
    pub scx: u8,
    pub ly: u8,
    pub lyc: u8,
    pub wy: u8,
    pub wx: u8,
    pub bgp: u8,
    pub obp0: u8,
    pub obp1: u8,
    pub bg_palette_ram: [u8; 64],
    pub obj_palette_ram: [u8; 64],
    pub bcps: u8,
    pub ocps: u8,

    pub cycles: u64,
    pub framebuffer: Box<[u8]>,
    pub sgb_enabled: bool,
    pub sgb_mask_mode: u8,
    pub sgb_active_palette: u8,
    pub sgb_palettes: [[u16; 4]; 4],

    pub sgb_border_enabled: bool,
    pub sgb_border_tile_data: Box<[u8]>,
    pub sgb_border_tilemap: [u16; SGB_BORDER_TILEMAP_SIZE],
    pub sgb_border_palettes: [[u16; SGB_BORDER_COLORS_PER_PALETTE]; SGB_BORDER_PALETTES],
    pub sgb_pal_trn_data: Box<[u8]>,
    pub sgb_attr_trn_data: Box<[u8]>,
    pub sgb_attr_map: [u8; SGB_ATTR_MAP_SIZE],
    sgb_composite_buffer: Box<[u8]>,

    pub window_line_counter: u8,
    pub window_was_active_this_frame: bool,
    pub window_y_triggered: bool,
    pub cgb_mode: bool,
    pub rendered_current_line: bool,
    pub lcd_was_enabled: bool,
    pub blank_first_frame_after_lcd_on: bool,
    prev_stat_line: bool,
    pub debug_flags: PpuDebugFlags,
    pub draw_dots_for_line: u64,
    pub dmg_palette_preset: DmgPalettePreset,
}

impl Default for PPU {
    fn default() -> Self {
        Self::new()
    }
}

impl PPU {
    pub fn new() -> Self {
        let default_bg_palette = default_cgb_palette_ram();
        let default_obj_palette = default_cgb_palette_ram();
        Self {
            lcdc: Lcdc::LCD_ENABLE | Lcdc::TILE_DATA | Lcdc::BG_ENABLE,
            stat: 0x85,
            scy: 0,
            scx: 0,
            ly: 0,
            lyc: 0,
            wy: 0,
            wx: 0,
            bgp: 0xFC,
            obp0: 0xFF,
            obp1: 0xFF,
            bg_palette_ram: default_bg_palette,
            obj_palette_ram: default_obj_palette,
            bcps: 0,
            ocps: 0,

            cycles: 0,
            framebuffer: vec![0; SCREEN_W * SCREEN_H * 4].into_boxed_slice(),
            sgb_enabled: false,
            sgb_mask_mode: 0,
            sgb_active_palette: 0,
            sgb_palettes: [
                [0x7FFF, 0x56B5, 0x2D6B, 0x0000],
                [0x7FFF, 0x56B5, 0x2D6B, 0x0000],
                [0x7FFF, 0x56B5, 0x2D6B, 0x0000],
                [0x7FFF, 0x56B5, 0x2D6B, 0x0000],
            ],

            sgb_border_enabled: false,
            sgb_border_tile_data: vec![0; SGB_CHR_TRANSFER_SIZE].into_boxed_slice(),
            sgb_border_tilemap: [0; SGB_BORDER_TILEMAP_SIZE],
            sgb_border_palettes: [[0; SGB_BORDER_COLORS_PER_PALETTE]; SGB_BORDER_PALETTES],
            sgb_pal_trn_data: vec![0; SGB_TRN_TRANSFER_SIZE].into_boxed_slice(),
            sgb_attr_trn_data: vec![0; SGB_TRN_TRANSFER_SIZE].into_boxed_slice(),
            sgb_attr_map: [0; SGB_ATTR_MAP_SIZE],
            sgb_composite_buffer: vec![0; SGB_BORDER_SIZE].into_boxed_slice(),

            window_line_counter: 0,
            window_was_active_this_frame: false,
            window_y_triggered: false,
            cgb_mode: false,
            rendered_current_line: false,
            prev_stat_line: false,
            lcd_was_enabled: false,
            blank_first_frame_after_lcd_on: false,
            debug_flags: PpuDebugFlags::default(),
            draw_dots_for_line: DRAW_DOTS_BASE,
            dmg_palette_preset: DmgPalettePreset::default(),
        }
    }
}

impl fmt::Debug for PPU {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PPU")
            .field("lcdc", &format_args!("{:#04X}", self.lcdc.bits()))
            .field("stat", &format_args!("{:#04X}", self.stat))
            .field("scy", &self.scy)
            .field("scx", &self.scx)
            .field("ly", &self.ly)
            .field("lyc", &self.lyc)
            .field("wy", &self.wy)
            .field("wx", &self.wx)
            .field("bgp", &format_args!("{:#04X}", self.bgp))
            .field("obp0", &format_args!("{:#04X}", self.obp0))
            .field("obp1", &format_args!("{:#04X}", self.obp1))
            .field("bcps", &format_args!("{:#04X}", self.bcps))
            .field("ocps", &format_args!("{:#04X}", self.ocps))
            .field("cycles", &self.cycles)
            .field("cgb_mode", &self.cgb_mode)
            .field("window_line_counter", &self.window_line_counter)
            .field("debug_flags", &self.debug_flags)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests;
