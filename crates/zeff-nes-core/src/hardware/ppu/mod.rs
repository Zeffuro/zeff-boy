mod registers;
mod renderer;

pub use registers::PpuRegisters;
pub use renderer::NES_PALETTE;

use std::fmt;

pub const SCREEN_W: usize = 256;
pub const SCREEN_H: usize = 240;

pub const SCANLINES_PER_FRAME: u16 = 262;
pub const DOTS_PER_SCANLINE: u16 = 341;
pub const VBLANK_SCANLINE: u16 = 241;
pub const PRE_RENDER_SCANLINE: u16 = 261;

const COARSE_X_MASK: u16 = 0x001F;
const NAMETABLE_X_BIT: u16 = 0x0400;
const FINE_Y_MASK: u16 = 0x7000;
const NAMETABLE_Y_BIT: u16 = 0x0800;
const COARSE_Y_MASK: u16 = 0x03E0;
const SCROLL_HORIZONTAL_MASK: u16 = 0x041F;
const SCROLL_VERTICAL_MASK: u16 = 0x7BE0;

fn default_framebuffer() -> Box<[u8]> {
    vec![0u8; SCREEN_W * SCREEN_H * 4].into_boxed_slice()
}

pub struct Ppu {
    pub regs: PpuRegisters,

    pub scanline: u16,
    pub dot: u16,
    pub nmi_output: bool,
    pub in_vblank: bool,
    pub odd_frame: bool,

    pub nametable_ram: [u8; 0x800],
    pub palette_ram: [u8; 32],

    pub oam: [u8; 256],
    pub secondary_oam: [u8; 32],
    pub oam_addr: u8,

    pub v: u16,
    pub t: u16,
    pub fine_x: u8,
    pub w: bool,

    pub read_buffer: u8,
    pub framebuffer: Box<[u8]>,
    pub frame_ready: bool,
    pub frame_count: u64,

    pub bg_shift_pattern_lo: u16,
    pub bg_shift_pattern_hi: u16,
    pub bg_shift_attrib_lo: u16,
    pub bg_shift_attrib_hi: u16,
    pub bg_next_tile_id: u8,
    pub bg_next_tile_attrib: u8,
    pub bg_next_tile_lo: u8,
    pub bg_next_tile_hi: u8,

    pub sprite_count: u8,
    pub sprite_patterns_lo: [u8; 8],
    pub sprite_patterns_hi: [u8; 8],
    pub sprite_attribs: [u8; 8],
    pub sprite_x_counters: [u8; 8],
    pub sprite_zero_rendering: bool,
}

impl Ppu {
    pub fn new() -> Self {
        Self {
            regs: PpuRegisters::new(),
            scanline: 0,
            dot: 0,
            nmi_output: false,
            in_vblank: false,
            odd_frame: false,
            nametable_ram: [0; 0x800],
            palette_ram: [0; 32],
            oam: [0; 256],
            secondary_oam: [0xFF; 32],
            oam_addr: 0,
            v: 0,
            t: 0,
            fine_x: 0,
            w: false,
            read_buffer: 0,
            framebuffer: default_framebuffer(),
            frame_ready: false,
            frame_count: 0,
            bg_shift_pattern_lo: 0,
            bg_shift_pattern_hi: 0,
            bg_shift_attrib_lo: 0,
            bg_shift_attrib_hi: 0,
            bg_next_tile_id: 0,
            bg_next_tile_attrib: 0,
            bg_next_tile_lo: 0,
            bg_next_tile_hi: 0,
            sprite_count: 0,
            sprite_patterns_lo: [0; 8],
            sprite_patterns_hi: [0; 8],
            sprite_attribs: [0; 8],
            sprite_x_counters: [0xFF; 8],
            sprite_zero_rendering: false,
        }
    }

    pub fn tick(&mut self, _cart_mirroring: super::cartridge::Mirroring) -> bool {
        let mut raise_nmi = false;

        if self.scanline == VBLANK_SCANLINE && self.dot == 1 {
            self.in_vblank = true;
            self.regs.set_vblank();
            self.frame_ready = true;
            if self.regs.nmi_enabled() {
                raise_nmi = true;
            }
        }

        if self.scanline == PRE_RENDER_SCANLINE {
            if self.dot == 1 {
                self.in_vblank = false;
                self.regs.clear_vblank();
                self.regs.clear_sprite_zero_hit();
                self.regs.clear_sprite_overflow();
            }

            if self.dot == 339 && self.odd_frame && self.regs.rendering_enabled() {
                self.dot = 0;
                self.scanline = 0;
                self.odd_frame = !self.odd_frame;
                self.frame_count += 1;
                return raise_nmi;
            }
        }

        self.dot += 1;
        if self.dot > 340 {
            self.dot = 0;
            self.scanline += 1;
            if self.scanline > PRE_RENDER_SCANLINE {
                self.scanline = 0;
                self.odd_frame = !self.odd_frame;
                self.frame_count += 1;
            }
        }

        raise_nmi
    }

    pub fn increment_scroll_x(&mut self) {
        if !self.regs.rendering_enabled() { return; }
        if (self.v & COARSE_X_MASK) == 31 {
            self.v &= !COARSE_X_MASK;
            self.v ^= NAMETABLE_X_BIT;
        } else {
            self.v += 1;
        }
    }

    pub fn increment_scroll_y(&mut self) {
        if !self.regs.rendering_enabled() { return; }
        if (self.v & FINE_Y_MASK) != FINE_Y_MASK {
            self.v += 0x1000;
        } else {
            self.v &= !FINE_Y_MASK;
            let mut coarse_y = (self.v & COARSE_Y_MASK) >> 5;
            if coarse_y == 29 {
                coarse_y = 0;
                self.v ^= NAMETABLE_Y_BIT;
            } else if coarse_y == 31 {
                coarse_y = 0;
            } else {
                coarse_y += 1;
            }
            self.v = (self.v & !COARSE_Y_MASK) | (coarse_y << 5);
        }
    }

    pub fn copy_horizontal_bits(&mut self) {
        if !self.regs.rendering_enabled() { return; }
        self.v = (self.v & !SCROLL_HORIZONTAL_MASK) | (self.t & SCROLL_HORIZONTAL_MASK);
    }

    pub fn copy_vertical_bits(&mut self) {
        if !self.regs.rendering_enabled() { return; }
        self.v = (self.v & !SCROLL_VERTICAL_MASK) | (self.t & SCROLL_VERTICAL_MASK);
    }

    pub fn load_bg_shifters(&mut self) {
        self.bg_shift_pattern_lo =
            (self.bg_shift_pattern_lo & 0xFF00) | self.bg_next_tile_lo as u16;
        self.bg_shift_pattern_hi =
            (self.bg_shift_pattern_hi & 0xFF00) | self.bg_next_tile_hi as u16;
        self.bg_shift_attrib_lo = (self.bg_shift_attrib_lo & 0xFF00)
            | if self.bg_next_tile_attrib & 0x01 != 0 { 0xFF } else { 0x00 };
        self.bg_shift_attrib_hi = (self.bg_shift_attrib_hi & 0xFF00)
            | if self.bg_next_tile_attrib & 0x02 != 0 { 0xFF } else { 0x00 };
    }

    pub fn update_shifters(&mut self) {
        if self.regs.show_bg() {
            self.bg_shift_pattern_lo <<= 1;
            self.bg_shift_pattern_hi <<= 1;
            self.bg_shift_attrib_lo <<= 1;
            self.bg_shift_attrib_hi <<= 1;
        }
        if self.regs.show_sprites() {
            for i in 0..self.sprite_count as usize {
                if self.sprite_x_counters[i] > 0 {
                    self.sprite_x_counters[i] -= 1;
                } else {
                    self.sprite_patterns_lo[i] <<= 1;
                    self.sprite_patterns_hi[i] <<= 1;
                }
            }
        }
    }
}

impl fmt::Debug for Ppu {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PPU")
            .field("scanline", &self.scanline)
            .field("dot", &self.dot)
            .field("v", &format_args!("{:#06X}", self.v))
            .field("t", &format_args!("{:#06X}", self.t))
            .field("fine_x", &self.fine_x)
            .field("in_vblank", &self.in_vblank)
            .field("frame_count", &self.frame_count)
            .finish_non_exhaustive()
    }
}
