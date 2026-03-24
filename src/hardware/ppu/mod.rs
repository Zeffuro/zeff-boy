mod palette;
mod renderer;
mod sgb;
mod sprite;
mod state;
mod tiles;
mod timing;

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
    pub(crate) debug_enable_bg: bool,
    pub(crate) debug_enable_window: bool,
    pub(crate) debug_enable_sprites: bool,
    pub(crate) color_correction: crate::settings::ColorCorrection,
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
            debug_enable_bg: true,
            debug_enable_window: true,
            debug_enable_sprites: true,
            color_correction: crate::settings::ColorCorrection::None,
        }
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

        ppu.lcdc = 0xA0;
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

        ppu.lcdc = 0xA0;
        ppu.wy = 0;
        ppu.wx = 7;

        ppu.step(DOTS_PER_LINE, &vram, &oam, false);
        ppu.step(DOTS_PER_LINE, &vram, &oam, false);
        assert_eq!(ppu.window_line_counter, 2);

        ppu.lcdc &= !0x20;
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

        ppu.lcdc = 0xA0;
        ppu.wy = 0;
        ppu.wx = 167;

        for _ in 0..8 {
            ppu.step(DOTS_PER_LINE, &vram, &oam, false);
        }

        assert_eq!(ppu.window_line_counter, 0);
        assert!(!ppu.window_was_active_this_frame);
    }
}
