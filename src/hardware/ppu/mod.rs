mod palette;
mod renderer;
mod sprite;

pub(crate) use sprite::SpriteEntry;
pub(crate) use palette::apply_palette;
pub(crate) use palette::PALETTE_COLORS;

pub(crate) const SCREEN_W: usize = 160;
pub(crate) const SCREEN_H: usize = 144;

const DOTS_PER_LINE: u64 = 456;
const OAM_DOTS: u64 = 80;
const DRAW_DOTS: u64 = 172;

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

    pub(crate) cycles: u64,
    pub(crate) framebuffer: [u8; SCREEN_W * SCREEN_H * 4],

    // Internal state for window
    window_line_counter: u8,
    window_was_active: bool,
}

impl PPU {
    pub(crate) fn new() -> Self {
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

            cycles: 0,
            framebuffer: [0; SCREEN_W * SCREEN_H * 4],

            window_line_counter: 0,
            window_was_active: false,
        }
    }

    /// Step the PPU by the given number of T-cycles.
    /// Returns a bitmask of interrupt flags to OR into IF.
    pub(crate) fn step(&mut self, cycles: u64, vram: &[u8], oam: &[u8]) -> u8 {
        if self.lcdc & 0x80 == 0 {
            // LCD off
            self.cycles = 0;
            self.ly = 0;
            self.stat = (self.stat & !0x03) | 0; // mode 0
            self.window_line_counter = 0;
            return 0;
        }

        self.cycles += cycles;
        let mut interrupts = 0u8;

        while self.cycles >= DOTS_PER_LINE {
            self.cycles -= DOTS_PER_LINE;

            // Render the current scanline before advancing LY
            if self.ly < 144 {
                renderer::render_scanline(self, vram, oam);
            }

            self.ly += 1;

            if self.ly == 144 {
                // Entering VBlank
                interrupts |= 0x01; // VBlank interrupt
                self.window_line_counter = 0;
            }
            if self.ly >= 154 {
                self.ly = 0;
                self.window_line_counter = 0;
            }

            // LYC check
            self.check_lyc(&mut interrupts);
        }

        // Determine current mode within the scanline
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
}

