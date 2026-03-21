mod palette;
mod renderer;
mod sprite;
mod tiles;

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
    pub(crate) framebuffer: [u8; SCREEN_W * SCREEN_H * 4],
    pub(crate) sgb_enabled: bool,
    pub(crate) sgb_mask_mode: u8,
    pub(crate) sgb_active_palette: u8,
    pub(crate) sgb_palettes: [[u16; 4]; 4],

    pub(crate) window_line_counter: u8,
    pub(crate) window_was_active_this_frame: bool,
    prev_stat_line: bool,
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
            framebuffer: [0; SCREEN_W * SCREEN_H * 4],
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
            prev_stat_line: false,
        }
    }

    pub(crate) fn window_visible_on_current_line(&self) -> bool {
        self.ly < SCREEN_H as u8 && self.lcdc & 0x20 != 0 && self.ly >= self.wy && self.wx <= 166
    }

    pub(crate) fn increment_window_line_counter_after_scanline(&mut self) {
        if self.window_visible_on_current_line() {
            self.window_line_counter = self.window_line_counter.saturating_add(1);
            self.window_was_active_this_frame = true;
        }
    }

    pub(crate) fn step(&mut self, cycles: u64, vram: &[u8], oam: &[u8], cgb_mode: bool) -> u8 {
        if self.lcdc & 0x80 == 0 {
            self.cycles = 0;
            self.ly = 0;
            self.stat = (self.stat & !0x03) | 0;
            self.window_line_counter = 0;
            self.window_was_active_this_frame = false;
            self.prev_stat_line = false;
            return 0;
        }

        self.cycles += cycles;
        let mut interrupts = 0u8;

        while self.cycles >= DOTS_PER_LINE {
            self.cycles -= DOTS_PER_LINE;

            if self.ly < 144 {
                if cgb_mode {
                    renderer::render_scanline_cgb(self, vram, oam);
                } else {
                    renderer::render_scanline_dmg(self, vram, oam);
                }
            }

            self.ly += 1;

            if self.ly == 144 {
                interrupts |= 0x01;
            }
            if self.ly >= 154 {
                self.ly = 0;
                self.window_line_counter = 0;
                self.window_was_active_this_frame = false;
            }
        }

        let previous_mode = self.stat & 0x03;
        let current_mode = if self.ly >= 144 {
            1 // VBlank
        } else if self.cycles <= OAM_DOTS {
            2 // OAM scan
        } else if self.cycles <= OAM_DOTS + DRAW_DOTS {
            3 // Drawing
        } else {
            0 // HBlank
        };

        if current_mode != previous_mode {
            self.stat = (self.stat & !0x03) | current_mode;
        }

        if self.update_stat_interrupt() {
            interrupts |= 0x02;
        }

        interrupts
    }

    pub(crate) fn mode(&self) -> u8 {
        self.stat & 0x03
    }

    pub(crate) fn lcd_enabled(&self) -> bool {
        self.lcdc & 0x80 != 0
    }

    pub(crate) fn cpu_vram_accessible(&self) -> bool {
        !self.lcd_enabled() || self.mode() != 3
    }

    pub(crate) fn cpu_oam_accessible(&self) -> bool {
        !self.lcd_enabled() || (self.mode() != 2 && self.mode() != 3)
    }

    pub(crate) fn cpu_palette_accessible(&self) -> bool {
        !self.lcd_enabled() || self.mode() != 3
    }

    fn update_stat_interrupt(&mut self) -> bool {
        let ly_match = self.ly == self.lyc;
        if ly_match {
            self.stat |= 0x04;
        } else {
            self.stat &= !0x04;
        }

        let mode = self.stat & 0x03;
        let stat_line = (self.stat & 0x40 != 0 && ly_match)
            || (self.stat & 0x20 != 0 && mode == 2)
            || (self.stat & 0x10 != 0 && mode == 1)
            || (self.stat & 0x08 != 0 && mode == 0);

        let rising_edge = stat_line && !self.prev_stat_line;
        self.prev_stat_line = stat_line;
        rising_edge
    }

    pub(crate) fn set_sgb_mode(&mut self, enabled: bool) {
        self.sgb_enabled = enabled;
    }

    pub(crate) fn set_sgb_palette(&mut self, index: usize, colors: [u16; 4]) {
        if index < 4 {
            self.sgb_palettes[index] = colors;
        }
    }

    pub(crate) fn set_sgb_active_palette(&mut self, index: u8) {
        self.sgb_active_palette = index & 0x03;
    }

    pub(crate) fn set_sgb_mask_mode(&mut self, mode: u8) {
        self.sgb_mask_mode = mode & 0x03;
    }

    pub(crate) fn sgb_dmg_rgba(&self, dmg_palette: u8, color_id: u8) -> [u8; 4] {
        if !self.sgb_enabled {
            return palette::apply_palette(dmg_palette, color_id);
        }
        let shade = ((dmg_palette >> (color_id * 2)) & 0x03) as usize;
        rgb555_to_rgba(self.sgb_palettes[self.sgb_active_palette as usize][shade])
    }

    pub(crate) fn sgb_remap_dmg_rgba(&self, rgba: [u8; 4]) -> [u8; 4] {
        if !self.sgb_enabled {
            return rgba;
        }
        for (shade, dmg_color) in palette::PALETTE_COLORS.iter().enumerate() {
            if *dmg_color == rgba {
                return rgb555_to_rgba(self.sgb_palettes[self.sgb_active_palette as usize][shade]);
            }
        }
        rgba
    }
}

fn rgb555_to_rgba(color: u16) -> [u8; 4] {
    let r5 = (color & 0x1F) as u8;
    let g5 = ((color >> 5) & 0x1F) as u8;
    let b5 = ((color >> 10) & 0x1F) as u8;
    let expand = |v: u8| (v << 3) | (v >> 2);
    [expand(r5), expand(g5), expand(b5), 255]
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
