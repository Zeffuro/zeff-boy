mod palette;
mod renderer;
mod sprite;
mod tiles;

pub(crate) use sprite::SpriteEntry;
pub(crate) use palette::apply_palette;
pub(crate) use palette::cgb_palette_rgba;
pub(crate) use palette::PALETTE_COLORS;
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

    window_line_counter: u8,
    window_was_active: bool,
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
            window_was_active: false,
        }
    }

    pub(crate) fn step(&mut self, cycles: u64, vram: &[u8], oam: &[u8], cgb_mode: bool) -> u8 {
        if self.lcdc & 0x80 == 0 {
            self.cycles = 0;
            self.ly = 0;
            self.stat = (self.stat & !0x03) | 0;
            self.window_line_counter = 0;
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
                self.window_line_counter = 0;
            }
            if self.ly >= 154 {
                self.ly = 0;
                self.window_line_counter = 0;
            }

            self.check_lyc(&mut interrupts);
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

            match current_mode {
                0 if self.stat & 0x08 != 0 => interrupts |= 0x02,
                1 if self.stat & 0x10 != 0 => interrupts |= 0x02,
                2 if self.stat & 0x20 != 0 => interrupts |= 0x02,
                _ => {}
            }
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

    fn check_lyc(&mut self, interrupts: &mut u8) {
        if self.ly == self.lyc {
            self.stat |= 0x04;
            if self.stat & 0x40 != 0 {
                *interrupts |= 0x02;
            }
        } else {
            self.stat &= !0x04;
        }
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

