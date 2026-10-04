use super::Bus;
use crate::hardware::cartridge::ChrFetchKind;
use crate::hardware::constants::{
    ATTRIBUTE_TABLE_BASE, NAMETABLE_BASE, SCREEN_HEIGHT, SCREEN_WIDTH,
};
use crate::hardware::ppu::Ppu;

const FIRST_VISIBLE_DOT: u16 = 1;
const LAST_VISIBLE_DOT: u16 = SCREEN_WIDTH as u16;
const LAST_VISIBLE_SCANLINE: u16 = SCREEN_HEIGHT as u16 - 1;
const BG_PREFETCH_START_DOT: u16 = 321;
const BG_PREFETCH_END_DOT: u16 = 337;
const BG_FETCH_PHASE_DOTS: u16 = 8;
const BG_FETCH_NAMETABLE_DOT: u16 = 0;
const BG_FETCH_ATTRIBUTE_DOT: u16 = 2;
const BG_FETCH_PATTERN_LO_DOT: u16 = 4;
const BG_FETCH_PATTERN_HI_DOT: u16 = 6;
const BG_SCROLL_X_DOT: u16 = 7;
const SCROLL_Y_DOT: u16 = 256;
const SPRITE_EVALUATION_DOT: u16 = 257;
const SPRITE_FETCH_END_DOT: u16 = 320;
const VERTICAL_COPY_START_DOT: u16 = 280;
const VERTICAL_COPY_END_DOT: u16 = 304;
const MMC3_A12_RISING_BG_RIGHT_PATTERN_DOT: u16 = 260;
const MMC3_A12_RISING_BG_LEFT_PATTERN_DOT: u16 = 324;
const PALETTE_INDEX_MASK: u8 = 0x3F;
const GREYSCALE_PALETTE_MASK: usize = 0x30;
const EMPHASIS_SHIFT: u8 = 5;
const EMPHASIS_MASK: u8 = 0x07;
const RGB_ALPHA: u8 = 0xFF;
const NAMETABLE_OFFSET_MASK: u16 = 0x0FFF;
const ATTRIBUTE_NAMETABLE_MASK: u16 = 0x0C00;
const ATTRIBUTE_COARSE_Y_MASK: u16 = 0x38;
const ATTRIBUTE_COARSE_X_MASK: u16 = 0x07;
const FINE_Y_SHIFT: u8 = 12;
const FINE_Y_MASK: u16 = 0x07;
const TILE_BYTES: u16 = 16;
const TILE_PLANE_BYTES: u16 = 8;
const SMALL_SPRITE_HEIGHT: u16 = 8;
const TALL_SPRITE_HEIGHT: u16 = 16;
const SPRITES_PER_SCANLINE: u8 = 8;
const OAM_ENTRY_BYTES: usize = 4;
const OAM_SPRITE_Y_OFFSET: u16 = 1;
const SPRITE_ATTR_FLIP_HORIZONTAL: u8 = 0x40;
const SPRITE_ATTR_FLIP_VERTICAL: u8 = 0x80;
const SPRITE_8X16_BANK_MASK: u16 = 0x01;
const SPRITE_8X16_TILE_MASK: u16 = 0xFE;
const SPRITE_8X16_BANK_BYTES: u16 = 0x1000;
const OAM_EMPTY_X: u8 = 0xFF;

impl Bus {
    pub(super) fn ppu_render_dot(&mut self) {
        let scanline = self.ppu.scanline;
        let dot = self.ppu.dot;
        let rendering = self.ppu.rendering_enabled();
        let visible_line = scanline <= LAST_VISIBLE_SCANLINE;
        let pre_render = scanline == self.ppu.pre_render_scanline();
        let render_line = visible_line || pre_render;

        if rendering && render_line && self.qualified_ppu_a12 {
            self.clock_qualified_ppu_a12(dot);
        } else if rendering && render_line {
            let bg_hi = self.ppu.regs.bg_pattern_addr() != 0;
            let spr_hi = self.ppu.regs.sprite_pattern_addr() != 0;
            let notify_dot = if bg_hi && !spr_hi {
                MMC3_A12_RISING_BG_LEFT_PATTERN_DOT
            } else {
                MMC3_A12_RISING_BG_RIGHT_PATTERN_DOT
            };
            if dot == notify_dot {
                self.cartridge.notify_scanline();
            }
        }

        if visible_line && (FIRST_VISIBLE_DOT..=LAST_VISIBLE_DOT).contains(&dot) {
            if rendering {
                let pal_idx = self.ppu.compose_pixel() as usize;
                Self::write_pixel(&mut self.ppu, dot, scanline, pal_idx, &self.palette_luts);
            } else {
                let pal_idx = (self.ppu.palette_ram[0] & PALETTE_INDEX_MASK) as usize;
                Self::write_pixel(&mut self.ppu, dot, scanline, pal_idx, &self.palette_luts);
            }
        }

        if rendering && render_line {
            if visible_line && dot <= LAST_VISIBLE_DOT && dot.is_multiple_of(2) {
                self.clock_sprite_evaluation();
            }

            let in_bg_range = (FIRST_VISIBLE_DOT..=LAST_VISIBLE_DOT).contains(&dot)
                || (BG_PREFETCH_START_DOT..=BG_PREFETCH_END_DOT).contains(&dot);

            if in_bg_range {
                let bg_reload_dot = (dot - FIRST_VISIBLE_DOT).is_multiple_of(BG_FETCH_PHASE_DOTS);
                if bg_reload_dot {
                    self.ppu.load_bg_shifters();
                }

                self.ppu.update_background_shifters();
                self.ppu.update_sprite_shifters();

                match (dot - FIRST_VISIBLE_DOT) % BG_FETCH_PHASE_DOTS {
                    BG_FETCH_NAMETABLE_DOT => {
                        let addr = NAMETABLE_BASE | (self.ppu.v & NAMETABLE_OFFSET_MASK);
                        self.ppu.bg_next_tile_id = self.ppu_bus_read(addr);
                    }
                    BG_FETCH_ATTRIBUTE_DOT => {
                        let v = self.ppu.v;
                        let addr = ATTRIBUTE_TABLE_BASE
                            | (v & ATTRIBUTE_NAMETABLE_MASK)
                            | ((v >> 4) & ATTRIBUTE_COARSE_Y_MASK)
                            | ((v >> 2) & ATTRIBUTE_COARSE_X_MASK);
                        let attrib = self.ppu_bus_read(addr);
                        let shift = ((v >> 4) & 0x04) | (v & 0x02);
                        self.ppu.bg_next_tile_attrib = (attrib >> shift) & 0x03;
                    }
                    BG_FETCH_PATTERN_LO_DOT => {
                        let base = self.ppu.regs.bg_pattern_addr();
                        let fine_y = (self.ppu.v >> FINE_Y_SHIFT) & FINE_Y_MASK;
                        let addr = base + (self.ppu.bg_next_tile_id as u16) * TILE_BYTES + fine_y;
                        self.ppu.bg_next_tile_lo = self.ppu_bus_read(addr);
                    }
                    BG_FETCH_PATTERN_HI_DOT => {
                        let base = self.ppu.regs.bg_pattern_addr();
                        let fine_y = (self.ppu.v >> FINE_Y_SHIFT) & FINE_Y_MASK;
                        let addr = base
                            + (self.ppu.bg_next_tile_id as u16) * TILE_BYTES
                            + fine_y
                            + TILE_PLANE_BYTES;
                        self.ppu.bg_next_tile_hi = self.ppu_bus_read(addr);
                    }
                    BG_SCROLL_X_DOT => {
                        self.ppu.increment_scroll_x();
                    }
                    _ => {}
                }
            }

            if dot == SCROLL_Y_DOT {
                self.ppu.increment_scroll_y();
            }

            if dot == SPRITE_EVALUATION_DOT {
                self.ppu.copy_horizontal_bits();
                if visible_line && scanline < LAST_VISIBLE_SCANLINE {
                    self.load_evaluated_sprites_for_scanline(scanline + 1);
                } else if pre_render {
                    self.clear_sprite_render_state();
                }
            }

            if pre_render && (VERTICAL_COPY_START_DOT..=VERTICAL_COPY_END_DOT).contains(&dot) {
                self.ppu.copy_vertical_bits();
            }
        }
    }

    fn clock_qualified_ppu_a12(&mut self, dot: u16) {
        let bg_fetch = (FIRST_VISIBLE_DOT..=LAST_VISIBLE_DOT).contains(&dot)
            || (BG_PREFETCH_START_DOT..=BG_PREFETCH_END_DOT).contains(&dot);
        if bg_fetch {
            let phase = (dot - FIRST_VISIBLE_DOT) % BG_FETCH_PHASE_DOTS;
            if phase == 0 {
                self.cartridge.notify_ppu_a12(false, self.ppu_cycles);
            } else if phase == 1 {
                let high = self.ppu.regs.bg_pattern_addr() != 0;
                self.cartridge.notify_ppu_a12(high, self.ppu_cycles);
            }
            return;
        }

        if (SPRITE_EVALUATION_DOT..=SPRITE_FETCH_END_DOT).contains(&dot) {
            let phase = (dot - SPRITE_EVALUATION_DOT) % BG_FETCH_PHASE_DOTS;
            if phase == 0 {
                self.cartridge.notify_ppu_a12(false, self.ppu_cycles);
            } else if phase == 1 {
                let slot = ((dot - SPRITE_EVALUATION_DOT) / BG_FETCH_PHASE_DOTS) as usize;
                let high = self.sprite_fetch_a12[slot];
                self.cartridge.notify_ppu_a12(high, self.ppu_cycles);
            }
            return;
        }

        if matches!(dot, 337 | 339) {
            self.cartridge.notify_ppu_a12(false, self.ppu_cycles);
        }
    }

    #[inline]
    fn write_pixel(
        ppu: &mut Ppu,
        dot: u16,
        scanline: u16,
        pal_idx: usize,
        palette_luts: &[[[u8; 4]; 64]; 8],
    ) {
        let effective_idx = if ppu.regs.greyscale() {
            pal_idx & GREYSCALE_PALETTE_MASK
        } else {
            pal_idx
        };
        let emphasis = ((ppu.regs.mask >> EMPHASIS_SHIFT) & EMPHASIS_MASK) as usize;
        let [r, g, b, _] = palette_luts[emphasis][effective_idx];

        let x = (dot - FIRST_VISIBLE_DOT) as usize;
        let y = scanline as usize;
        let offset = (y * SCREEN_WIDTH + x) * 4;
        ppu.framebuffer[offset..offset + 4].copy_from_slice(&[r, g, b, RGB_ALPHA]);
    }

    #[inline]
    fn sprite_is_in_range(&self, y: u8) -> bool {
        let height = if self.ppu.regs.tall_sprites() {
            TALL_SPRITE_HEIGHT
        } else {
            SMALL_SPRITE_HEIGHT
        };
        self.ppu.scanline.wrapping_sub(u16::from(y)) < height
    }

    #[inline]
    fn begin_sprite_evaluation(&mut self) {
        self.ppu.sprite_eval_oam_addr = 0;
        self.ppu.sprite_eval_secondary_addr = 0;
        self.ppu.sprite_eval_latch = 0xFF;
        self.ppu.sprite_eval_in_range = false;
        self.ppu.sprite_eval_done = false;
        self.ppu.sprite_eval_sprite_zero = false;
        self.ppu.sprite_eval_overflow_remaining = 0;
    }

    #[inline]
    fn clock_sprite_evaluation(&mut self) {
        let dot = self.ppu.dot;
        if (FIRST_VISIBLE_DOT..65).contains(&dot) {
            if dot == 2 {
                self.begin_sprite_evaluation();
            }
            self.ppu.secondary_oam[(dot / 2 - 1) as usize] = 0xFF;
            return;
        }

        if !(65..=LAST_VISIBLE_DOT).contains(&dot) {
            return;
        }

        if dot == 66 {
            self.begin_sprite_evaluation();
        }

        self.ppu.sprite_eval_latch = self.ppu.oam[self.ppu.sprite_eval_oam_addr as usize];

        if self.ppu.sprite_eval_done {
            self.ppu.sprite_eval_oam_addr = self.ppu.sprite_eval_oam_addr.wrapping_add(4);
            return;
        }

        if self.ppu.sprite_eval_secondary_addr < 32 {
            let secondary = self.ppu.sprite_eval_secondary_addr as usize;
            self.ppu.secondary_oam[secondary] = self.ppu.sprite_eval_latch;

            if self.ppu.sprite_eval_oam_addr & 0x03 == 0 {
                self.ppu.sprite_eval_in_range = self.sprite_is_in_range(self.ppu.sprite_eval_latch);
                if !self.ppu.sprite_eval_in_range {
                    self.advance_sprite_eval_to_next_sprite();
                    return;
                }
                if self.ppu.sprite_eval_oam_addr == 0 {
                    self.ppu.sprite_eval_sprite_zero = true;
                }
            }

            self.ppu.sprite_eval_secondary_addr += 1;
            self.ppu.sprite_eval_oam_addr = self.ppu.sprite_eval_oam_addr.wrapping_add(1);
            if self.ppu.sprite_eval_oam_addr & 0x03 == 0 {
                self.ppu.sprite_eval_in_range = false;
                if self.ppu.sprite_eval_oam_addr == 0 {
                    self.ppu.sprite_eval_done = true;
                }
            }
            return;
        }

        if self.ppu.sprite_eval_overflow_remaining > 0 {
            self.ppu.sprite_eval_oam_addr = self.ppu.sprite_eval_oam_addr.wrapping_add(1);
            self.ppu.sprite_eval_overflow_remaining -= 1;
            if self.ppu.sprite_eval_overflow_remaining == 0 {
                self.ppu.sprite_eval_oam_addr &= !0x03;
                self.ppu.sprite_eval_done = true;
            }
            return;
        }

        if self.sprite_is_in_range(self.ppu.sprite_eval_latch) {
            self.ppu.regs.set_sprite_overflow();
            self.ppu.sprite_eval_oam_addr = self.ppu.sprite_eval_oam_addr.wrapping_add(1);
            self.ppu.sprite_eval_overflow_remaining = 3;
        } else {
            let n = (self.ppu.sprite_eval_oam_addr >> 2).wrapping_add(1) & 0x3F;
            let m = self.ppu.sprite_eval_oam_addr.wrapping_add(1) & 0x03;
            self.ppu.sprite_eval_oam_addr = (n << 2) | m;
            if n == 0 {
                self.ppu.sprite_eval_done = true;
            }
        }
    }

    fn advance_sprite_eval_to_next_sprite(&mut self) {
        self.ppu.sprite_eval_oam_addr =
            (self.ppu.sprite_eval_oam_addr & !0x03).wrapping_add(OAM_ENTRY_BYTES as u8);
        self.ppu.sprite_eval_in_range = false;
        if self.ppu.sprite_eval_oam_addr == 0 {
            self.ppu.sprite_eval_done = true;
        }
    }

    fn clear_sprite_render_state(&mut self) {
        let default_a12 = if self.ppu.regs.tall_sprites() {
            true
        } else {
            self.ppu.regs.sprite_pattern_addr() != 0
        };
        self.sprite_fetch_a12 = [default_a12; 8];
        self.ppu.sprite_count = 0;
        self.ppu.sprite_zero_rendering = false;
        self.ppu.sprite_patterns_lo = [0; 8];
        self.ppu.sprite_patterns_hi = [0; 8];
        self.ppu.sprite_attribs = [0; 8];
        self.ppu.sprite_x_counters = [OAM_EMPTY_X; 8];
    }

    #[inline]
    fn load_evaluated_sprites_for_scanline(&mut self, target: u16) {
        let sprite_height: u16 = if self.ppu.regs.tall_sprites() {
            TALL_SPRITE_HEIGHT
        } else {
            SMALL_SPRITE_HEIGHT
        };
        let pattern_base = self.ppu.regs.sprite_pattern_addr();
        self.clear_sprite_render_state();
        self.ppu.sprite_zero_rendering = self.ppu.sprite_eval_sprite_zero;
        let count =
            (self.ppu.sprite_eval_secondary_addr / OAM_ENTRY_BYTES as u8).min(SPRITES_PER_SCANLINE);

        for i in 0..count as usize {
            let base = i * OAM_ENTRY_BYTES;
            let effective_y =
                u16::from(self.ppu.secondary_oam[base]).wrapping_add(OAM_SPRITE_Y_OFFSET);
            let diff = target.wrapping_sub(effective_y);
            let in_range = diff < sprite_height;

            let tile_index = self.ppu.secondary_oam[base + 1];
            let attributes = self.ppu.secondary_oam[base + 2];
            let sprite_x = self.ppu.secondary_oam[base + 3];
            let flip_h = attributes & SPRITE_ATTR_FLIP_HORIZONTAL != 0;
            let flip_v = attributes & SPRITE_ATTR_FLIP_VERTICAL != 0;

            // Rendering or sprite size can change after secondary OAM was populated.
            let mut row = diff & (sprite_height - 1);
            if flip_v {
                row = sprite_height - 1 - row;
            }

            let lo_addr = if sprite_height == SMALL_SPRITE_HEIGHT {
                pattern_base + (tile_index as u16) * TILE_BYTES + row
            } else {
                let bank = (tile_index as u16 & SPRITE_8X16_BANK_MASK) * SPRITE_8X16_BANK_BYTES;
                let tile = tile_index as u16 & SPRITE_8X16_TILE_MASK;
                if row < SMALL_SPRITE_HEIGHT {
                    bank + tile * TILE_BYTES + row
                } else {
                    bank + (tile + 1) * TILE_BYTES + (row - SMALL_SPRITE_HEIGHT)
                }
            };
            let hi_addr = lo_addr + TILE_PLANE_BYTES;

            let mut lo = self.ppu_bus_read_with_kind(lo_addr, ChrFetchKind::Sprite);
            let mut hi = self.ppu_bus_read_with_kind(hi_addr, ChrFetchKind::Sprite);

            if !in_range {
                lo = 0;
                hi = 0;
            }

            if flip_h {
                lo = lo.reverse_bits();
                hi = hi.reverse_bits();
            }

            self.sprite_fetch_a12[i] = lo_addr & 0x1000 != 0;
            self.ppu.sprite_patterns_lo[i] = lo;
            self.ppu.sprite_patterns_hi[i] = hi;
            self.ppu.sprite_attribs[i] = attributes;
            self.ppu.sprite_x_counters[i] = sprite_x;
        }

        self.ppu.sprite_count = count;
    }
}

#[cfg(test)]
mod tests;
