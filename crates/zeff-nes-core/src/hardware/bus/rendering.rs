use super::Bus;
use crate::hardware::cartridge::ChrFetchKind;
use crate::hardware::constants::{ATTRIBUTE_TABLE_BASE, NAMETABLE_BASE};
use crate::hardware::ppu::{PRE_RENDER_SCANLINE, Ppu, SCREEN_H, SCREEN_W};

const FIRST_VISIBLE_DOT: u16 = 1;
const LAST_VISIBLE_DOT: u16 = SCREEN_W as u16;
const LAST_VISIBLE_SCANLINE: u16 = SCREEN_H as u16 - 1;
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
const SPRITE_COUNT: usize = 64;
const SPRITES_PER_SCANLINE: u8 = 8;
const OAM_ENTRY_BYTES: usize = 4;
const OAM_INDEX_MASK: usize = 0xFF;
const OAM_OVERFLOW_BUG_M_MASK: u8 = 0x03;
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
        let pre_render = scanline == PRE_RENDER_SCANLINE;
        let render_line = visible_line || pre_render;

        if rendering && render_line {
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
                    self.evaluate_sprites_for_scanline(scanline + 1);
                } else if pre_render {
                    self.evaluate_sprites_for_scanline(0);
                }
            }

            if pre_render && (VERTICAL_COPY_START_DOT..=VERTICAL_COPY_END_DOT).contains(&dot) {
                self.ppu.copy_vertical_bits();
            }
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
        let offset = (y * SCREEN_W + x) * 4;
        ppu.framebuffer[offset..offset + 4].copy_from_slice(&[r, g, b, RGB_ALPHA]);
    }

    #[inline]
    fn evaluate_sprites_for_scanline(&mut self, target: u16) {
        let sprite_height: u16 = if self.ppu.regs.tall_sprites() {
            TALL_SPRITE_HEIGHT
        } else {
            SMALL_SPRITE_HEIGHT
        };
        let pattern_base = self.ppu.regs.sprite_pattern_addr();

        self.ppu.sprite_count = 0;
        self.ppu.sprite_zero_rendering = false;
        self.ppu.sprite_patterns_lo = [0; 8];
        self.ppu.sprite_patterns_hi = [0; 8];
        self.ppu.sprite_attribs = [0; 8];
        self.ppu.sprite_x_counters = [OAM_EMPTY_X; 8];
        self.ppu.overflow_bug_m = 0;

        let mut count: u8 = 0;

        for i in 0..SPRITE_COUNT {
            let base = i * OAM_ENTRY_BYTES;

            let oam_y = if count >= SPRITES_PER_SCANLINE {
                self.ppu.oam[(base + self.ppu.overflow_bug_m as usize) & OAM_INDEX_MASK] as u16
            } else {
                self.ppu.oam[base] as u16
            };

            let effective_y = oam_y.wrapping_add(OAM_SPRITE_Y_OFFSET);
            let diff = target.wrapping_sub(effective_y);
            if diff >= sprite_height {
                if count >= SPRITES_PER_SCANLINE {
                    self.ppu.overflow_bug_m =
                        self.ppu.overflow_bug_m.wrapping_add(1) & OAM_OVERFLOW_BUG_M_MASK;
                }
                continue;
            }

            if count >= SPRITES_PER_SCANLINE {
                self.ppu.regs.set_sprite_overflow();
                break;
            }

            if i == 0 {
                self.ppu.sprite_zero_rendering = true;
            }

            let tile_index = self.ppu.oam[base + 1];
            let attributes = self.ppu.oam[base + 2];
            let sprite_x = self.ppu.oam[base + 3];
            let flip_h = attributes & SPRITE_ATTR_FLIP_HORIZONTAL != 0;
            let flip_v = attributes & SPRITE_ATTR_FLIP_VERTICAL != 0;

            let mut row = diff;
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

            if flip_h {
                lo = lo.reverse_bits();
                hi = hi.reverse_bits();
            }

            let idx = count as usize;
            self.ppu.sprite_patterns_lo[idx] = lo;
            self.ppu.sprite_patterns_hi[idx] = hi;
            self.ppu.sprite_attribs[idx] = attributes;
            self.ppu.sprite_x_counters[idx] = sprite_x;

            count += 1;
        }

        self.ppu.sprite_count = count;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hardware::cartridge::Cartridge;

    fn test_bus() -> Bus {
        let mut rom = vec![0u8; 16 + 0x4000 + 0x2000];
        rom[0..4].copy_from_slice(b"NES\x1A");
        rom[4] = 1;
        rom[5] = 1;

        let cart = Cartridge::load(&rom).expect("test ROM should load");
        Bus::new(cart, 44_100.0)
    }

    #[test]
    fn sprite_evaluation_prepares_next_visible_scanline_at_dot_257() {
        let mut bus = test_bus();
        bus.ppu.regs.mask = 0x18;
        bus.ppu.oam = [0xFF; 256];
        bus.ppu.oam[0] = 4;
        bus.ppu.oam[1] = 0x12;
        bus.ppu.oam[2] = 0x01;
        bus.ppu.oam[3] = 24;

        bus.ppu.scanline = 5;
        bus.ppu.dot = 0;
        bus.ppu_render_dot();
        assert_eq!(bus.ppu.sprite_count, 0);

        bus.ppu.scanline = 4;
        bus.ppu.dot = 257;
        bus.ppu_render_dot();
        assert_eq!(bus.ppu.sprite_count, 1);
        assert_eq!(bus.ppu.sprite_attribs[0], 0x01);
        assert_eq!(bus.ppu.sprite_x_counters[0], 24);

        bus.ppu.dot = 321;
        bus.ppu_render_dot();
        assert_eq!(bus.ppu.sprite_x_counters[0], 24);
    }

    #[test]
    fn background_prefetch_dot_337_aligns_first_visible_pixel() {
        let mut bus = test_bus();
        bus.ppu.regs.mask = 0x0A;
        bus.ppu.palette_ram[0] = 0x0F;
        bus.ppu.palette_ram[1] = 0x12;
        bus.ppu.scanline = PRE_RENDER_SCANLINE;
        bus.ppu.dot = 337;
        bus.ppu.bg_shift_pattern_lo = 0x4000;
        bus.ppu.bg_shift_pattern_hi = 0;
        bus.ppu.bg_shift_attrib_lo = 0;
        bus.ppu.bg_shift_attrib_hi = 0;
        bus.ppu.bg_next_tile_lo = 0;
        bus.ppu.bg_next_tile_hi = 0;
        bus.ppu.bg_next_tile_attrib = 0;

        bus.ppu_render_dot();

        bus.ppu.scanline = 0;
        bus.ppu.dot = 1;
        bus.ppu_render_dot();

        assert_eq!(&bus.ppu.framebuffer[0..4], &[48, 50, 236, 0xFF]);
    }

    #[test]
    fn background_reload_dot_aligns_loaded_tile_left_edge() {
        let mut bus = test_bus();
        bus.ppu.regs.mask = 0x0A;
        bus.ppu.palette_ram[0] = 0x0F;
        bus.ppu.palette_ram[1] = 0x12;
        bus.ppu.scanline = 0;
        bus.ppu.bg_next_tile_lo = 0x80;
        bus.ppu.bg_next_tile_hi = 0;
        bus.ppu.bg_next_tile_attrib = 0;

        for dot in 121..=129 {
            bus.ppu.dot = dot;
            bus.ppu_render_dot();
        }

        let pixel_127 = 127 * 4;
        let pixel_128 = 128 * 4;
        assert_eq!(
            &bus.ppu.framebuffer[pixel_127..pixel_127 + 4],
            &[0, 0, 0, 0xFF]
        );
        assert_eq!(
            &bus.ppu.framebuffer[pixel_128..pixel_128 + 4],
            &[48, 50, 236, 0xFF]
        );
    }
}
