mod palette;
mod renderer;
mod sgb;
mod sprite;
mod state;
mod tiles;
mod timing;

use std::fmt;

pub(crate) use palette::PALETTE_COLORS;
pub(crate) use palette::apply_palette;
pub(crate) use palette::cgb_palette_rgba;
pub(crate) use sprite::SpriteEntry;
pub(crate) use tiles::decode_tile_pixel;
pub(crate) use tiles::tile_data_address;

pub(crate) const SCREEN_W: usize = 160;
pub(crate) const SCREEN_H: usize = 144;

const DOTS_PER_LINE: u64 = 456;
const OAM_DOTS: u64 = 80;
const DRAW_DOTS: u64 = 172;
pub(crate) const LCDC_BG_ENABLE: u8 = 0x01;
pub(crate) const LCDC_OBJ_ENABLE: u8 = 0x02;
pub(crate) const LCDC_OBJ_SIZE: u8 = 0x04;
pub(crate) const LCDC_BG_TILEMAP: u8 = 0x08;
pub(crate) const LCDC_TILE_DATA: u8 = 0x10;
pub(crate) const LCDC_WINDOW_ENABLE: u8 = 0x20;
pub(crate) const LCDC_WINDOW_TILEMAP: u8 = 0x40;
pub(crate) const LCDC_LCD_ENABLE: u8 = 0x80;

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
pub(crate) struct PpuDebugFlags {
    pub(crate) bg: bool,
    pub(crate) window: bool,
    pub(crate) sprites: bool,
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

pub(crate) struct PPU {
    pub(crate) lcdc: u8,
    pub(crate) stat: u8,
    pub(crate) scy: u8,
    pub(crate) scx: u8,
    pub(crate) ly: u8,
    pub(crate) lyc: u8,
    pub(crate) wy: u8,
    pub(crate) wx: u8,
    pub(crate) bgp: u8,
    pub(crate) obp0: u8,
    pub(crate) obp1: u8,
    pub(crate) bg_palette_ram: [u8; 64],
    pub(crate) obj_palette_ram: [u8; 64],
    pub(crate) bcps: u8,
    pub(crate) ocps: u8,

    pub(crate) cycles: u64,
    pub(crate) framebuffer: Box<[u8]>,
    pub(crate) sgb_enabled: bool,
    pub(crate) sgb_mask_mode: u8,
    pub(crate) sgb_active_palette: u8,
    pub(crate) sgb_palettes: [[u16; 4]; 4],

    pub(crate) window_line_counter: u8,
    pub(crate) window_was_active_this_frame: bool,
    pub(crate) window_y_triggered: bool,
    pub(crate) cgb_mode: bool,
    pub(crate) rendered_current_line: bool,
    prev_stat_line: bool,
    pub(crate) debug_flags: PpuDebugFlags,
    pub(crate) color_correction: crate::settings::ColorCorrection,
    pub(crate) color_correction_matrix: [f32; 9],
}

impl PPU {
    pub(crate) fn new() -> Self {
        let default_bg_palette = default_cgb_palette_ram();
        let default_obj_palette = default_cgb_palette_ram();
        Self {
            lcdc: 0x91,
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

            window_line_counter: 0,
            window_was_active_this_frame: false,
            window_y_triggered: false,
            cgb_mode: false,
            rendered_current_line: false,
            prev_stat_line: false,
            debug_flags: PpuDebugFlags::default(),
            color_correction: crate::settings::ColorCorrection::None,
            color_correction_matrix: [
                1.0, 0.0, 0.0, // R
                0.0, 1.0, 0.0, // G
                0.0, 0.0, 1.0, // B
            ],
        }
    }
}

impl fmt::Debug for PPU {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PPU")
            .field("lcdc", &format_args!("{:#04X}", self.lcdc))
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
            .field("color_correction", &self.color_correction)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stat_interrupt_triggers_only_on_rising_edge() {
        let mut ppu = PPU::new();

        ppu.stat = (ppu.stat & !0x03) | 0x08;
        ppu.ly = 10;
        ppu.lyc = 0;

        assert!(ppu.update_stat_interrupt());
        assert!(!ppu.update_stat_interrupt());

        ppu.stat = (ppu.stat & !0x03) | 0x03;
        assert!(!ppu.update_stat_interrupt());

        ppu.stat = (ppu.stat & !0x03) | 0x00;
        assert!(ppu.update_stat_interrupt());
    }

    #[test]
    fn stat_update_tracks_lyc_coincidence_flag() {
        let mut ppu = PPU::new();

        ppu.stat = (ppu.stat & !0x03) | 0x40;
        ppu.ly = 7;
        ppu.lyc = 7;

        assert!(ppu.update_stat_interrupt());
        assert_ne!(ppu.stat & 0x04, 0);

        ppu.ly = 8;
        assert!(!ppu.update_stat_interrupt());
        assert_eq!(ppu.stat & 0x04, 0);
    }

    #[test]
    fn window_counter_resets_on_frame_wrap_not_vblank_start() {
        let mut ppu = PPU::new();
        let vram = [0u8; 0x4000];
        let oam = [0u8; 160];

        ppu.lcdc = LCDC_LCD_ENABLE | LCDC_WINDOW_ENABLE;
        ppu.wy = 0;
        ppu.wx = 7;

        for _ in 0..144 {
            ppu.step(DOTS_PER_LINE, &vram, &oam, false);
        }

        assert_eq!(ppu.ly, 144);
        assert_eq!(ppu.window_line_counter, 144);
        assert!(ppu.window_was_active_this_frame);

        ppu.step(DOTS_PER_LINE, &vram, &oam, false);
        assert_eq!(ppu.ly, 145);
        assert_eq!(ppu.window_line_counter, 144);

        for _ in 0..9 {
            ppu.step(DOTS_PER_LINE, &vram, &oam, false);
        }

        assert_eq!(ppu.ly, 0);
        assert_eq!(ppu.window_line_counter, 0);
        assert!(!ppu.window_was_active_this_frame);
    }

    #[test]
    fn window_counter_freezes_when_window_disabled_between_scanlines() {
        let mut ppu = PPU::new();
        let vram = [0u8; 0x4000];
        let oam = [0u8; 160];

        ppu.lcdc = LCDC_LCD_ENABLE | LCDC_WINDOW_ENABLE;
        ppu.wy = 0;
        ppu.wx = 7;

        ppu.step(DOTS_PER_LINE, &vram, &oam, false);
        ppu.step(DOTS_PER_LINE, &vram, &oam, false);
        assert_eq!(ppu.window_line_counter, 2);

        ppu.lcdc &= !LCDC_WINDOW_ENABLE;
        for _ in 0..4 {
            ppu.step(DOTS_PER_LINE, &vram, &oam, false);
        }
        assert_eq!(ppu.window_line_counter, 2);
    }

    #[test]
    fn window_counter_requires_wx_visibility_range() {
        let mut ppu = PPU::new();
        let vram = [0u8; 0x4000];
        let oam = [0u8; 160];

        ppu.lcdc = LCDC_LCD_ENABLE | LCDC_WINDOW_ENABLE;
        ppu.wy = 0;
        ppu.wx = 167;

        for _ in 0..8 {
            ppu.step(DOTS_PER_LINE, &vram, &oam, false);
        }

        assert_eq!(ppu.window_line_counter, 0);
        assert!(!ppu.window_was_active_this_frame);
    }

    #[test]
    fn mode_sequence_during_active_scanline() {
        let mut ppu = PPU::new();
        let vram = [0u8; 0x4000];
        let oam = [0u8; 160];

        ppu.lcdc = LCDC_LCD_ENABLE;
        ppu.ly = 0;
        ppu.cycles = 0;

        ppu.step(OAM_DOTS, &vram, &oam, false);
        assert_eq!(ppu.mode(), 2);

        ppu.step(DRAW_DOTS, &vram, &oam, false);
        assert_eq!(ppu.mode(), 3);

        let hblank_dots = DOTS_PER_LINE - OAM_DOTS - DRAW_DOTS;
        ppu.step(hblank_dots - 1, &vram, &oam, false);
        assert_eq!(ppu.mode(), 0);
    }

    #[test]
    fn vblank_interrupt_fires_at_line_144() {
        let mut ppu = PPU::new();
        let vram = [0u8; 0x4000];
        let oam = [0u8; 160];

        ppu.lcdc = LCDC_LCD_ENABLE;

        for _ in 0..143 {
            let irq = ppu.step(DOTS_PER_LINE, &vram, &oam, false);
            assert_eq!(irq & 0x01, 0, "VBlank should not fire before line 144");
        }
        assert_eq!(ppu.ly, 143);

        let irq = ppu.step(DOTS_PER_LINE, &vram, &oam, false);
        assert_ne!(irq & 0x01, 0, "VBlank should fire at line 144");
        assert_eq!(ppu.ly, 144);
        assert_eq!(ppu.mode(), 1);
    }

    #[test]
    fn ly_wraps_to_zero_after_line_153() {
        let mut ppu = PPU::new();
        let vram = [0u8; 0x4000];
        let oam = [0u8; 160];

        ppu.lcdc = LCDC_LCD_ENABLE;

        for _ in 0..154 {
            ppu.step(DOTS_PER_LINE, &vram, &oam, false);
        }
        assert_eq!(ppu.ly, 0);
    }

    #[test]
    fn lcd_disabled_clears_mode_and_ly() {
        let mut ppu = PPU::new();
        let vram = [0u8; 0x4000];
        let oam = [0u8; 160];

        ppu.lcdc = LCDC_LCD_ENABLE;
        for _ in 0..50 {
            ppu.step(DOTS_PER_LINE, &vram, &oam, false);
        }
        assert!(ppu.ly > 0);

        // Disable LCD.
        ppu.lcdc = 0;
        ppu.step(4, &vram, &oam, false);
        assert_eq!(ppu.ly, 0);
        assert_eq!(ppu.mode(), 0);
        assert_eq!(ppu.cycles, 0);
    }

    #[test]
    fn vram_accessible_outside_mode3() {
        let _ppu = PPU::new();

        let mut off_ppu = PPU::new();
        off_ppu.lcdc = 0;
        assert!(off_ppu.cpu_vram_accessible());
        assert!(off_ppu.cpu_oam_accessible());

        let mut draw_ppu = PPU::new();
        draw_ppu.stat = (draw_ppu.stat & !0x03) | 3;
        assert!(!draw_ppu.cpu_vram_accessible());
        assert!(!draw_ppu.cpu_oam_accessible());

        // Mode 0 (HBlank) → accessible.
        let mut hblank_ppu = PPU::new();
        hblank_ppu.stat = (hblank_ppu.stat & !0x03) | 0;
        assert!(hblank_ppu.cpu_vram_accessible());
        assert!(hblank_ppu.cpu_oam_accessible());
    }

    #[test]
    fn lyc_coincidence_sets_stat_flag() {
        let mut ppu = PPU::new();
        ppu.stat = (ppu.stat & !0x03) | 0x40;
        ppu.ly = 5;
        ppu.lyc = 5;

        ppu.update_stat_interrupt();
        assert_ne!(ppu.stat & 0x04, 0, "LYC coincidence flag should be set");

        ppu.ly = 6;
        ppu.update_stat_interrupt();
        assert_eq!(ppu.stat & 0x04, 0, "LYC coincidence flag should be cleared");
    }
}
