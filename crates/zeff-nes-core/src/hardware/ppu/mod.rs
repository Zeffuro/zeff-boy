mod registers;
mod renderer;

pub use registers::PpuRegisters;
pub use renderer::NES_PALETTE;
pub use renderer::NES_RGB_2C03_PALETTE;
pub use renderer::NesBasePalette;
pub use renderer::NesPalette;
pub use renderer::NesPaletteMode;
pub use renderer::apply_nes_emphasis;
pub use renderer::apply_nes_palette_mode;
pub use renderer::apply_rgb_ppu_emphasis;
pub use renderer::parse_nes_palette_bytes;

use std::fmt;

use crate::hardware::constants::{
    FRAMEBUFFER_LEN, MASK_SHOW_BG, MASK_SHOW_BG_LEFT8, MASK_SHOW_SPRITES, MASK_SHOW_SPRITES_LEFT8,
    PPU_DOTS_PER_SCANLINE, PPU_PALETTE_RAM_BYTES, PRIMARY_OAM_BYTES, SCREEN_WIDTH,
    SECONDARY_OAM_BYTES,
};
use crate::hardware::timing::NesTiming;

pub const SCREEN_W: usize = crate::hardware::constants::SCREEN_WIDTH;
pub const SCREEN_H: usize = crate::hardware::constants::SCREEN_HEIGHT;
pub const FRAMEBUFFER_SIZE: usize = crate::hardware::constants::FRAMEBUFFER_LEN;
pub const SCANLINES_PER_FRAME: u16 = crate::hardware::constants::NTSC_SCANLINES_PER_FRAME;
pub const DOTS_PER_SCANLINE: u16 = crate::hardware::constants::PPU_DOTS_PER_SCANLINE;
pub const VBLANK_SCANLINE: u16 = crate::hardware::constants::VBLANK_START_SCANLINE;
pub const PRE_RENDER_SCANLINE: u16 = SCANLINES_PER_FRAME - 1;

// blargg's ppu_open_bus test documents the PPU I/O latch as a per-bit
// dynamic latch where bits that are not refreshed with a 1 decay to 0 after
// roughly 600 ms. At NTSC timing this is approximately 36 frames.
pub(crate) const PPU_IO_LATCH_DECAY_PPU_CYCLES: u64 =
    crate::hardware::constants::CPU_CYCLES_PER_FRAME * 3 * 36;

const LAST_DOT: u16 = PPU_DOTS_PER_SCANLINE - 1;
const ODD_FRAME_SKIP_DOT: u16 = LAST_DOT - 1;

const COARSE_X_MASK: u16 = 0x001F;
const NAMETABLE_X_BIT: u16 = 0x0400;
const FINE_Y_MASK: u16 = 0x7000;
const NAMETABLE_Y_BIT: u16 = 0x0800;
const COARSE_Y_MASK: u16 = 0x03E0;
const SCROLL_HORIZONTAL_MASK: u16 = 0x041F;
const SCROLL_VERTICAL_MASK: u16 = 0x7BE0;
const PPUMASK_RENDERING_BITS: u8 = MASK_SHOW_BG | MASK_SHOW_SPRITES;
const PPUMASK_RENDERING_DELAY_DOTS: u8 = 1;

pub struct Ppu {
    pub(crate) regs: PpuRegisters,

    pub(crate) scanline: u16,
    pub(crate) dot: u16,
    pub(crate) nmi_output: bool,
    pub(crate) in_vblank: bool,
    pub(crate) odd_frame: bool,
    pub(crate) suppress_vblank_edge: bool,

    pub(crate) nametable_ram: [u8; 0x1000],
    pub(crate) palette_ram: [u8; PPU_PALETTE_RAM_BYTES],

    pub(crate) oam: [u8; PRIMARY_OAM_BYTES],
    pub(crate) secondary_oam: [u8; SECONDARY_OAM_BYTES],
    pub(crate) oam_addr: u8,

    pub(crate) v: u16,
    pub(crate) t: u16,
    pub(crate) fine_x: u8,
    pub(crate) w: bool,

    pub(crate) read_buffer: u8,

    pub(crate) io_latch: u8,
    pub(crate) io_latch_decay_at_ppu_cycle: [u64; 8],
    rendering_mask_delay: u8,
    rendering_mask_latched_bits: u8,
    pub(crate) framebuffer: Box<[u8; FRAMEBUFFER_LEN]>,
    pub(crate) frame_ready: bool,
    pub(crate) frame_count: u64,
    pub(crate) odd_frame_dot_skip_enabled: bool,
    vblank_start_scanline: u16,
    pre_render_scanline: u16,

    pub(crate) bg_shift_pattern_lo: u16,
    pub(crate) bg_shift_pattern_hi: u16,
    pub(crate) bg_shift_attrib_lo: u16,
    pub(crate) bg_shift_attrib_hi: u16,
    pub(crate) bg_next_tile_id: u8,
    pub(crate) bg_next_tile_attrib: u8,
    pub(crate) bg_next_tile_lo: u8,
    pub(crate) bg_next_tile_hi: u8,

    pub(crate) sprite_count: u8,
    pub(crate) sprite_patterns_lo: [u8; 8],
    pub(crate) sprite_patterns_hi: [u8; 8],
    pub(crate) sprite_attribs: [u8; 8],
    pub(crate) sprite_x_counters: [u8; 8],
    pub(crate) sprite_zero_rendering: bool,

    pub(crate) sprite_eval_oam_addr: u8,
    pub(crate) sprite_eval_secondary_addr: u8,
    pub(crate) sprite_eval_latch: u8,
    pub(crate) sprite_eval_in_range: bool,
    pub(crate) sprite_eval_done: bool,
    pub(crate) sprite_eval_sprite_zero: bool,
    pub(crate) sprite_eval_overflow_remaining: u8,
}

impl Default for Ppu {
    fn default() -> Self {
        Self::new()
    }
}

impl Ppu {
    pub fn new() -> Self {
        Self::new_with_timing(NesTiming::Ntsc)
    }

    pub(crate) fn new_with_timing(timing: NesTiming) -> Self {
        Self {
            regs: PpuRegisters::new(),
            scanline: 0,
            dot: 0,
            nmi_output: false,
            in_vblank: false,
            odd_frame: false,
            suppress_vblank_edge: false,
            nametable_ram: [0; 0x1000],
            palette_ram: [0; PPU_PALETTE_RAM_BYTES],
            oam: [0; PRIMARY_OAM_BYTES],
            secondary_oam: [0xFF; SECONDARY_OAM_BYTES],
            oam_addr: 0,
            v: 0,
            t: 0,
            fine_x: 0,
            w: false,
            read_buffer: 0,
            io_latch: 0,
            io_latch_decay_at_ppu_cycle: [0; 8],
            rendering_mask_delay: 0,
            rendering_mask_latched_bits: 0,
            framebuffer: Box::new([0u8; FRAMEBUFFER_LEN]),
            frame_ready: false,
            frame_count: 0,
            odd_frame_dot_skip_enabled: timing.odd_frame_dot_skip(),
            vblank_start_scanline: timing.vblank_start_scanline(),
            pre_render_scanline: timing.pre_render_scanline(),
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
            sprite_eval_oam_addr: 0,
            sprite_eval_secondary_addr: 0,
            sprite_eval_latch: 0xFF,
            sprite_eval_in_range: false,
            sprite_eval_done: false,
            sprite_eval_sprite_zero: false,
            sprite_eval_overflow_remaining: 0,
        }
    }

    #[inline]
    fn effective_mask(&self) -> u8 {
        if self.rendering_mask_delay > 0 {
            (self.regs.mask & !PPUMASK_RENDERING_BITS) | self.rendering_mask_latched_bits
        } else {
            self.regs.mask
        }
    }

    #[inline]
    pub(crate) fn write_mask(&mut self, val: u8) {
        let old_rendering_bits = self.effective_mask() & PPUMASK_RENDERING_BITS;
        let new_rendering_bits = val & PPUMASK_RENDERING_BITS;
        self.regs.mask = val;

        if old_rendering_bits != new_rendering_bits {
            self.rendering_mask_latched_bits = old_rendering_bits;
            self.rendering_mask_delay = PPUMASK_RENDERING_DELAY_DOTS;
        } else {
            self.rendering_mask_delay = 0;
            self.rendering_mask_latched_bits = new_rendering_bits;
        }
    }

    #[inline]
    fn tick_rendering_mask_delay(&mut self) {
        if self.rendering_mask_delay > 0 {
            self.rendering_mask_delay -= 1;
            if self.rendering_mask_delay == 0 {
                self.rendering_mask_latched_bits = self.regs.mask & PPUMASK_RENDERING_BITS;
            }
        }
    }

    #[inline]
    pub(crate) fn show_bg(&self) -> bool {
        self.effective_mask() & MASK_SHOW_BG != 0
    }

    #[inline]
    pub(crate) fn show_sprites(&self) -> bool {
        self.effective_mask() & MASK_SHOW_SPRITES != 0
    }

    #[inline]
    pub(crate) fn rendering_enabled(&self) -> bool {
        self.effective_mask() & (MASK_SHOW_BG | MASK_SHOW_SPRITES) != 0
    }

    #[inline]
    pub(crate) fn show_bg_left8(&self) -> bool {
        self.effective_mask() & MASK_SHOW_BG_LEFT8 != 0
    }

    #[inline]
    pub(crate) fn show_sprites_left8(&self) -> bool {
        self.effective_mask() & MASK_SHOW_SPRITES_LEFT8 != 0
    }

    pub(crate) fn set_odd_frame_dot_skip_enabled(&mut self, enabled: bool) {
        self.odd_frame_dot_skip_enabled = enabled;
    }

    pub(crate) const fn pre_render_scanline(&self) -> u16 {
        self.pre_render_scanline
    }

    pub(crate) const fn vblank_start_scanline(&self) -> u16 {
        self.vblank_start_scanline
    }

    #[inline]
    pub(crate) fn io_latch_value_at(&self, ppu_cycles: u64) -> u8 {
        let mut value = self.io_latch;
        for bit in 0..8 {
            let mask = 1u8 << bit;
            if value & mask != 0 && ppu_cycles >= self.io_latch_decay_at_ppu_cycle[bit] {
                value &= !mask;
            }
        }
        value
    }

    #[inline]
    pub(crate) fn decay_io_latch_at(&mut self, ppu_cycles: u64) {
        self.io_latch = self.io_latch_value_at(ppu_cycles);
    }

    #[inline]
    pub(crate) fn refresh_io_latch_bits(&mut self, value: u8, mask: u8, ppu_cycles: u64) {
        let mut latch = self.io_latch_value_at(ppu_cycles);
        latch = (latch & !mask) | (value & mask);
        self.io_latch = latch;

        let decay_at = ppu_cycles.saturating_add(PPU_IO_LATCH_DECAY_PPU_CYCLES);
        for bit in 0..8 {
            if mask & (1u8 << bit) != 0 {
                self.io_latch_decay_at_ppu_cycle[bit] = decay_at;
            }
        }
    }

    pub fn peek_register_at(&self, addr: u16, ppu_cycles: u64) -> u8 {
        let latch = self.io_latch_value_at(ppu_cycles);
        match addr {
            0x2002 => (self.regs.status & 0xE0) | (latch & 0x1F),
            0x2004 => {
                let mut data = self.oam[self.oam_addr as usize];
                if self.oam_addr & 0x03 == 0x02 {
                    data &= !0x1C;
                }
                data
            }
            0x2007 => {
                let ppu_addr = self.v & 0x3FFF;
                if ppu_addr >= 0x3F00 {
                    (self.palette_ram[(ppu_addr as usize - 0x3F00) & 0x1F] & 0x3F) | (latch & 0xC0)
                } else {
                    self.read_buffer
                }
            }
            _ => latch,
        }
    }

    #[inline]
    pub fn tick(&mut self) -> bool {
        let mut raise_nmi = false;

        if self.scanline == self.vblank_start_scanline && self.dot == 1 {
            if self.suppress_vblank_edge {
                self.suppress_vblank_edge = false;
                self.in_vblank = false;
                self.regs.clear_vblank();
                self.nmi_output = false;
            } else {
                self.in_vblank = true;
                self.regs.set_vblank();
                self.frame_ready = true;
                let new_nmi_output = self.regs.nmi_enabled();
                raise_nmi = !self.nmi_output && new_nmi_output;
                self.nmi_output = new_nmi_output;
            }
        }

        if self.scanline == self.pre_render_scanline {
            if self.dot == 1 {
                self.in_vblank = false;
                self.regs.clear_vblank();
                self.nmi_output = false;
                self.regs.clear_sprite_zero_hit();
                self.regs.clear_sprite_overflow();
            }

            if self.odd_frame_dot_skip_enabled
                && self.dot == ODD_FRAME_SKIP_DOT
                && self.odd_frame
                && self.rendering_enabled()
            {
                self.dot = 0;
                self.scanline = 0;
                self.odd_frame = !self.odd_frame;
                self.frame_count += 1;
                self.tick_rendering_mask_delay();
                return raise_nmi;
            }
        }

        self.tick_rendering_mask_delay();

        self.dot += 1;
        if self.dot > LAST_DOT {
            self.dot = 0;
            self.scanline += 1;
            if self.scanline > self.pre_render_scanline {
                self.scanline = 0;
                self.odd_frame = !self.odd_frame;
                self.frame_count += 1;
            }
        }

        raise_nmi
    }

    #[inline]
    pub fn increment_scroll_x(&mut self) {
        if !self.rendering_enabled() {
            return;
        }
        if (self.v & COARSE_X_MASK) == 31 {
            self.v &= !COARSE_X_MASK;
            self.v ^= NAMETABLE_X_BIT;
        } else {
            self.v += 1;
        }
    }

    #[inline]
    pub fn increment_scroll_y(&mut self) {
        if !self.rendering_enabled() {
            return;
        }
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

    #[inline]
    pub fn copy_horizontal_bits(&mut self) {
        if !self.rendering_enabled() {
            return;
        }
        self.v = (self.v & !SCROLL_HORIZONTAL_MASK) | (self.t & SCROLL_HORIZONTAL_MASK);
    }

    #[inline]
    pub fn copy_vertical_bits(&mut self) {
        if !self.rendering_enabled() {
            return;
        }
        self.v = (self.v & !SCROLL_VERTICAL_MASK) | (self.t & SCROLL_VERTICAL_MASK);
    }

    #[inline]
    pub fn load_bg_shifters(&mut self) {
        self.bg_shift_pattern_lo =
            (self.bg_shift_pattern_lo & 0xFF00) | self.bg_next_tile_lo as u16;
        self.bg_shift_pattern_hi =
            (self.bg_shift_pattern_hi & 0xFF00) | self.bg_next_tile_hi as u16;
        self.bg_shift_attrib_lo = (self.bg_shift_attrib_lo & 0xFF00)
            | if self.bg_next_tile_attrib & 0x01 != 0 {
                0xFF
            } else {
                0x00
            };
        self.bg_shift_attrib_hi = (self.bg_shift_attrib_hi & 0xFF00)
            | if self.bg_next_tile_attrib & 0x02 != 0 {
                0xFF
            } else {
                0x00
            };
    }

    #[inline]
    pub fn update_background_shifters(&mut self) {
        if self.show_bg() {
            self.bg_shift_pattern_lo <<= 1;
            self.bg_shift_pattern_hi <<= 1;
            self.bg_shift_attrib_lo <<= 1;
            self.bg_shift_attrib_hi <<= 1;
        }
    }

    #[inline]
    pub fn update_sprite_shifters(&mut self) {
        if self.show_sprites() && (1..=SCREEN_WIDTH as u16).contains(&self.dot) {
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

    #[inline]
    pub fn update_shifters(&mut self) {
        self.update_background_shifters();
        self.update_sprite_shifters();
    }

    pub fn write_state(&self, w: &mut crate::save_state::StateWriter) {
        w.write_u8(self.regs.ctrl);
        w.write_u8(self.regs.mask);
        w.write_u8(self.regs.status);

        w.write_u16(self.scanline);
        w.write_u16(self.dot);
        w.write_bool(self.nmi_output);
        w.write_bool(self.in_vblank);
        w.write_bool(self.odd_frame);

        w.write_bytes(&self.nametable_ram);
        w.write_bytes(&self.palette_ram);

        w.write_bytes(&self.oam);
        w.write_bytes(&self.secondary_oam);
        w.write_u8(self.oam_addr);

        w.write_u16(self.v);
        w.write_u16(self.t);
        w.write_u8(self.fine_x);
        w.write_bool(self.w);

        w.write_u8(self.read_buffer);
        w.write_u8(self.io_latch);
        w.write_u64(self.frame_count);

        w.write_u16(self.bg_shift_pattern_lo);
        w.write_u16(self.bg_shift_pattern_hi);
        w.write_u16(self.bg_shift_attrib_lo);
        w.write_u16(self.bg_shift_attrib_hi);
        w.write_u8(self.bg_next_tile_id);
        w.write_u8(self.bg_next_tile_attrib);
        w.write_u8(self.bg_next_tile_lo);
        w.write_u8(self.bg_next_tile_hi);

        w.write_u8(self.sprite_count);
        w.write_bytes(&self.sprite_patterns_lo);
        w.write_bytes(&self.sprite_patterns_hi);
        w.write_bytes(&self.sprite_attribs);
        w.write_bytes(&self.sprite_x_counters);
        w.write_bool(self.sprite_zero_rendering);
    }

    pub fn read_state(&mut self, r: &mut crate::save_state::StateReader) -> anyhow::Result<()> {
        self.regs.ctrl = r.read_u8()?;
        self.regs.mask = r.read_u8()?;
        self.regs.status = r.read_u8()?;

        self.scanline = r.read_u16()?;
        self.dot = r.read_u16()?;
        self.nmi_output = r.read_bool()?;
        self.in_vblank = r.read_bool()?;
        self.odd_frame = r.read_bool()?;

        r.read_exact(&mut self.nametable_ram)?;
        r.read_exact(&mut self.palette_ram)?;

        r.read_exact(&mut self.oam)?;
        r.read_exact(&mut self.secondary_oam)?;
        self.oam_addr = r.read_u8()?;

        self.v = r.read_u16()?;
        self.t = r.read_u16()?;
        self.fine_x = r.read_u8()?;
        self.w = r.read_bool()?;

        self.read_buffer = r.read_u8()?;
        self.io_latch = r.read_u8()?;
        self.frame_count = r.read_u64()?;

        self.bg_shift_pattern_lo = r.read_u16()?;
        self.bg_shift_pattern_hi = r.read_u16()?;
        self.bg_shift_attrib_lo = r.read_u16()?;
        self.bg_shift_attrib_hi = r.read_u16()?;
        self.bg_next_tile_id = r.read_u8()?;
        self.bg_next_tile_attrib = r.read_u8()?;
        self.bg_next_tile_lo = r.read_u8()?;
        self.bg_next_tile_hi = r.read_u8()?;

        self.sprite_count = r.read_u8()?;
        r.read_exact(&mut self.sprite_patterns_lo)?;
        r.read_exact(&mut self.sprite_patterns_hi)?;
        r.read_exact(&mut self.sprite_attribs)?;
        r.read_exact(&mut self.sprite_x_counters)?;
        self.sprite_zero_rendering = r.read_bool()?;

        self.frame_ready = false;
        self.suppress_vblank_edge = false;
        self.rendering_mask_delay = 0;
        self.rendering_mask_latched_bits = self.regs.mask & PPUMASK_RENDERING_BITS;

        Ok(())
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

#[cfg(test)]
mod tests {
    use super::{LAST_DOT, ODD_FRAME_SKIP_DOT, Ppu};
    use crate::hardware::constants::{CTRL_NMI_ENABLE, STATUS_VBLANK};
    use crate::hardware::timing::NesTiming;

    #[test]
    fn odd_frame_dot_skip_is_enabled_by_default() {
        let mut ppu = Ppu::new();
        ppu.regs.mask = 0x18;
        ppu.scanline = NesTiming::Ntsc.pre_render_scanline();
        ppu.dot = ODD_FRAME_SKIP_DOT;
        ppu.odd_frame = true;

        ppu.tick();

        assert_eq!(ppu.scanline, 0);
        assert_eq!(ppu.dot, 0);
        assert!(!ppu.odd_frame);
    }

    #[test]
    fn odd_frame_dot_skip_can_be_disabled_for_rgb_ppu() {
        let mut ppu = Ppu::new();
        ppu.set_odd_frame_dot_skip_enabled(false);
        ppu.regs.mask = 0x18;
        ppu.scanline = NesTiming::Ntsc.pre_render_scanline();
        ppu.dot = ODD_FRAME_SKIP_DOT;
        ppu.odd_frame = true;

        ppu.tick();

        assert_eq!(ppu.scanline, NesTiming::Ntsc.pre_render_scanline());
        assert_eq!(ppu.dot, LAST_DOT);
        assert!(ppu.odd_frame);
    }

    #[test]
    fn pal_and_dendy_frames_use_312_scanlines_without_odd_frame_skip() {
        for timing in [NesTiming::Pal, NesTiming::Dendy] {
            let mut ppu = Ppu::new_with_timing(timing);
            ppu.regs.mask = 0x18;
            ppu.scanline = timing.pre_render_scanline();
            ppu.dot = ODD_FRAME_SKIP_DOT;
            ppu.odd_frame = true;

            ppu.tick();
            assert_eq!(
                (ppu.scanline, ppu.dot),
                (timing.pre_render_scanline(), LAST_DOT)
            );
            ppu.tick();
            assert_eq!((ppu.scanline, ppu.dot), (0, 0));
            assert!(!ppu.odd_frame);
            assert_eq!(ppu.frame_count, 1);
        }
    }

    #[test]
    fn regional_vblank_edges_follow_the_timing_profile() {
        for timing in [NesTiming::Ntsc, NesTiming::Pal, NesTiming::Dendy] {
            let mut ppu = Ppu::new_with_timing(timing);
            ppu.regs.ctrl = CTRL_NMI_ENABLE;
            ppu.scanline = timing.vblank_start_scanline();
            ppu.dot = 1;

            assert!(ppu.tick());
            assert!(ppu.in_vblank);
            assert_ne!(ppu.regs.status & STATUS_VBLANK, 0);
            assert!(ppu.frame_ready);
        }

        let mut dendy = Ppu::new_with_timing(NesTiming::Dendy);
        dendy.regs.ctrl = CTRL_NMI_ENABLE;
        dendy.scanline = NesTiming::Pal.vblank_start_scanline();
        dendy.dot = 1;

        assert!(!dendy.tick());
        assert!(!dendy.in_vblank);
        assert_eq!(dendy.regs.status & STATUS_VBLANK, 0);
        assert!(!dendy.frame_ready);
    }

    #[test]
    fn ppumask_rendering_enable_is_delayed_one_dot() {
        let mut ppu = Ppu::new();

        ppu.write_mask(0x18);

        assert!(!ppu.rendering_enabled());
        ppu.tick();
        assert!(ppu.rendering_enabled());
    }

    #[test]
    fn ppumask_rendering_disable_is_delayed_one_dot() {
        let mut ppu = Ppu::new();
        ppu.regs.mask = 0x18;

        ppu.write_mask(0x00);

        assert!(ppu.rendering_enabled());
        ppu.tick();
        assert!(!ppu.rendering_enabled());
    }
}
