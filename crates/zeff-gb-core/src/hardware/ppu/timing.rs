use super::{
    DOTS_PER_LINE, DRAW_DOTS_BASE, LCD_ON_INITIAL_MODE0_DOTS, Lcdc, OAM_DOTS, PPU, SCREEN_H,
    STAT_IRQ_HBLANK_DELAY_DOTS, STAT_IRQ_OAM_DOTS, renderer,
};

impl PPU {
    pub(in crate::hardware) fn write_lcdc(&mut self, value: u8) -> u8 {
        let was_enabled = self.lcdc.contains(Lcdc::LCD_ENABLE);
        self.lcdc = Lcdc::from_bits_truncate(value);
        let enabled = self.lcdc.contains(Lcdc::LCD_ENABLE);

        match (was_enabled, enabled) {
            (false, true) => self.enable_lcd_after_lcdc_write(),
            (true, false) => {
                self.disable_lcd_after_lcdc_write();
                0
            }
            _ => 0,
        }
    }

    fn enable_lcd_after_lcdc_write(&mut self) -> u8 {
        self.lcd_was_enabled = true;
        self.blank_first_frame_after_lcd_on = true;

        self.cycles = 4;
        self.ly = 0;
        self.stat &= !0x03;
        self.window_line_counter = 0;
        self.window_was_active_this_frame = false;
        self.window_y_triggered = false;
        self.rendered_current_line = false;
        self.draw_dots_for_line = DRAW_DOTS_BASE;
        self.prev_cpu_stat_mode0_line = false;
        self.cpu_stat_mode0_pending_before_if = false;
        self.reset_framebuffer_for_rendering();

        u8::from(self.update_stat_interrupt_for_mode(0)) << 1
    }

    fn disable_lcd_after_lcdc_write(&mut self) {
        self.lcd_was_enabled = false;
        self.blank_first_frame_after_lcd_on = false;

        self.cycles = 0;
        self.ly = 0;
        self.stat &= !0x03;
        self.window_line_counter = 0;
        self.window_was_active_this_frame = false;
        self.window_y_triggered = false;
        self.rendered_current_line = false;
        self.draw_dots_for_line = DRAW_DOTS_BASE;
        self.prev_cpu_stat_mode0_line = false;
        self.cpu_stat_mode0_pending_before_if = false;
    }

    pub(super) fn window_enable_condition(&self) -> bool {
        self.lcdc.contains(Lcdc::WINDOW_ENABLE)
    }

    pub(super) fn window_visible_on_current_line(&self) -> bool {
        self.ly < SCREEN_H as u8
            && self.window_enable_condition()
            && self.window_y_triggered
            && self.wx <= 166
    }

    pub(super) fn increment_window_line_counter_after_scanline(&mut self) {
        if self.window_visible_on_current_line() {
            self.window_line_counter = self.window_line_counter.saturating_add(1);
            self.window_was_active_this_frame = true;
        }
    }

    fn compute_draw_dots_for_line(&self, oam: &[u8]) -> u64 {
        if self.ly >= 144 {
            return DRAW_DOTS_BASE;
        }

        let scx_penalty = (self.scx & 7) as u64;

        let sprite_penalty = if self.lcdc.contains(Lcdc::OBJ_ENABLE) {
            self.sprite_fetch_penalty_dots(oam)
        } else {
            0
        };

        let window_penalty = if self.window_visible_on_current_line() {
            6
        } else {
            0
        };

        DRAW_DOTS_BASE + scx_penalty + sprite_penalty + window_penalty
    }

    fn sprite_fetch_penalty_dots(&self, oam: &[u8]) -> u64 {
        let tall = self.lcdc.contains(Lcdc::OBJ_SIZE);
        let sprite_h: i32 = if tall { 16 } else { 8 };
        let scx = u16::from(self.scx & 7);
        let mut selected = arrayvec::ArrayVec::<u8, 10>::new();

        for i in 0..40usize {
            let base = i * 4;
            if base + 3 >= oam.len() {
                break;
            }

            let sy = i32::from(oam[base]) - 16;
            if i32::from(self.ly) >= sy && i32::from(self.ly) < sy + sprite_h {
                selected.push(oam[base + 1]);
                if selected.is_full() {
                    break;
                }
            }
        }

        let mut penalty = 0u64;
        let mut bucket_stalls = [0u8; 22];
        if selected.iter().any(|&x| x == 0) {
            penalty += u64::from(scx);
        }

        for &x in &selected {
            if x >= 168 {
                continue;
            }

            let adjusted_x = u16::from(x) + scx;
            let bucket = usize::from(adjusted_x >> 3);
            let stall = 5u8.saturating_sub((adjusted_x & 7) as u8);
            bucket_stalls[bucket] = bucket_stalls[bucket].max(stall);
            penalty += 6;
        }

        let total = penalty
            + bucket_stalls
                .iter()
                .map(|&stall| u64::from(stall))
                .sum::<u64>();

        total & !3
    }

    #[inline]
    pub(in crate::hardware) fn step(
        &mut self,
        cycles: u64,
        vram: &[u8],
        oam: &[u8],
        cgb_mode: bool,
    ) -> u8 {
        self.cgb_mode = cgb_mode;

        let lcd_enabled = self.lcdc.contains(Lcdc::LCD_ENABLE);
        let mut interrupts = 0u8;

        if !lcd_enabled {
            if self.lcd_was_enabled {
                self.disable_lcd_after_lcdc_write();
            }
            return 0;
        }

        if !self.lcd_was_enabled {
            interrupts |= self.enable_lcd_after_lcdc_write();
        }

        if self.ly == self.wy {
            self.window_y_triggered = true;
        }

        if self.ly < 144 && !self.rendered_current_line {
            self.draw_dots_for_line = self.compute_draw_dots_for_line(oam);
        }

        let previous_mode = self.stat & 0x03;

        self.cycles += cycles;

        let should_render_output = !self.blank_first_frame_after_lcd_on;
        let draw_dots = self.draw_dots_for_line;

        if !self.rendered_current_line && self.cycles >= OAM_DOTS + draw_dots {
            if self.ly < 144 && should_render_output {
                if cgb_mode {
                    renderer::render_scanline_cgb(self, vram, oam);
                } else {
                    renderer::render_scanline_dmg(self, vram, oam);
                }
            }
            self.rendered_current_line = true;
        }

        while self.cycles >= DOTS_PER_LINE {
            self.cycles -= DOTS_PER_LINE;

            if !self.rendered_current_line && self.ly < 144 && should_render_output {
                if cgb_mode {
                    renderer::render_scanline_cgb(self, vram, oam);
                } else {
                    renderer::render_scanline_dmg(self, vram, oam);
                }
            }

            if self.ly < 144 {
                self.increment_window_line_counter_after_scanline();
            }

            self.ly += 1;
            self.rendered_current_line = false;

            if self.ly == 144 {
                interrupts |= 0x01;
            }

            if self.ly >= 154 {
                self.ly = 0;
                self.window_line_counter = 0;
                self.window_was_active_this_frame = false;
                self.window_y_triggered = false;

                if self.blank_first_frame_after_lcd_on {
                    self.blank_first_frame_after_lcd_on = false;
                }

                self.reset_framebuffer_for_rendering();

                if self.sgb_border_enabled && self.sgb_enabled {
                    self.render_sgb_border_framebuffer();
                }
            }

            if self.ly == self.wy {
                self.window_y_triggered = true;
            }

            if self.ly < 144 {
                self.draw_dots_for_line = self.compute_draw_dots_for_line(oam);
            }
        }

        let draw_dots = self.draw_dots_for_line;
        let current_mode = self.mode_for_cycles(self.cycles, draw_dots, OAM_DOTS);

        if current_mode != previous_mode {
            self.stat = (self.stat & !0x03) | current_mode;
        }

        let interrupt_mode = self.stat_interrupt_mode_for_cycles(self.cycles, draw_dots);

        if self.update_cpu_stat_mode0_interrupt_before_if(self.cycles, draw_dots) {
            self.cpu_stat_mode0_pending_before_if = true;
        }

        if self.update_stat_interrupt_for_mode(interrupt_mode) {
            interrupts |= 0x02;
        }

        interrupts
    }

    #[inline]
    pub(in crate::hardware) fn mode(&self) -> u8 {
        self.stat & 0x03
    }

    fn mode_for_cycles(&self, cycles: u64, draw_dots: u64, oam_dots: u64) -> u8 {
        if self.ly >= 144 {
            1
        } else if self.blank_first_frame_after_lcd_on && self.ly == 0 {
            if cycles < LCD_ON_INITIAL_MODE0_DOTS {
                0
            } else if cycles < LCD_ON_INITIAL_MODE0_DOTS + draw_dots {
                3
            } else {
                0
            }
        } else if self.blank_first_frame_after_lcd_on && cycles < 4 {
            0
        } else if cycles < oam_dots {
            2
        } else if cycles < oam_dots + draw_dots {
            3
        } else {
            0
        }
    }

    fn stat_interrupt_mode_for_cycles(&self, cycles: u64, draw_dots: u64) -> u8 {
        if self.ly >= 144 {
            1
        } else if self.blank_first_frame_after_lcd_on && self.ly == 0 {
            if cycles < LCD_ON_INITIAL_MODE0_DOTS {
                0
            } else if cycles < LCD_ON_INITIAL_MODE0_DOTS + draw_dots + STAT_IRQ_HBLANK_DELAY_DOTS {
                3
            } else {
                0
            }
        } else if self.blank_first_frame_after_lcd_on && cycles < 4 {
            0
        } else if cycles < STAT_IRQ_OAM_DOTS {
            2
        } else if cycles < OAM_DOTS + draw_dots + STAT_IRQ_HBLANK_DELAY_DOTS {
            3
        } else {
            0
        }
    }

    #[inline]
    pub(in crate::hardware) fn drain_cpu_stat_interrupt_pending_before_if(&mut self) -> bool {
        let pending = self.cpu_stat_mode0_pending_before_if;
        self.cpu_stat_mode0_pending_before_if = false;
        pending
    }

    #[inline]
    pub(in crate::hardware) fn lcd_enabled(&self) -> bool {
        self.lcdc.contains(Lcdc::LCD_ENABLE)
    }

    #[cfg(test)]
    pub(in crate::hardware) fn cpu_vram_accessible(&self) -> bool {
        self.cpu_vram_read_accessible()
    }

    #[inline]
    pub(in crate::hardware) fn cpu_vram_read_accessible(&self) -> bool {
        !self.cpu_vram_blocked_by_ppu()
    }

    #[inline]
    pub(in crate::hardware) fn cpu_vram_write_accessible(&self) -> bool {
        !self.cpu_vram_blocked_by_ppu()
    }

    fn cpu_access_block_window(&self) -> (u64, u64, u64, u64) {
        let start_dot = if self.cgb_mode {
            STAT_IRQ_OAM_DOTS + 1
        } else {
            STAT_IRQ_OAM_DOTS
        };
        let end_dot = if self.cgb_double_speed {
            start_dot + self.draw_dots_for_line + 1
        } else if self.cgb_mode {
            start_dot + self.draw_dots_for_line
        } else {
            start_dot + self.draw_dots_for_line + 1
        };
        let first_line_end_dot = if self.cgb_double_speed {
            start_dot + self.draw_dots_for_line + 2 + u64::from(self.scx & 1)
        } else if self.cgb_mode {
            start_dot + self.draw_dots_for_line + 2
        } else {
            end_dot + 2
        };
        let cgb_normal_speed_first_line_edge_grace = self.cgb_mode && !self.cgb_double_speed;
        let first_line_start_dot = if cgb_normal_speed_first_line_edge_grace {
            LCD_ON_INITIAL_MODE0_DOTS + 1
        } else {
            LCD_ON_INITIAL_MODE0_DOTS
        };

        (start_dot, end_dot, first_line_start_dot, first_line_end_dot)
    }

    fn cpu_vram_blocked_by_ppu(&self) -> bool {
        if !self.lcd_enabled() || self.ly >= 144 {
            return false;
        }

        let (start_dot, end_dot, first_line_start_dot, first_line_end_dot) =
            self.cpu_access_block_window();

        if self.blank_first_frame_after_lcd_on && self.ly == 0 {
            return self.cycles >= first_line_start_dot && self.cycles < first_line_end_dot;
        }

        if self.blank_first_frame_after_lcd_on && self.cycles < 4 {
            return false;
        }

        self.cycles >= start_dot && self.cycles < end_dot
    }

    #[cfg(test)]
    pub(in crate::hardware) fn cpu_oam_accessible(&self) -> bool {
        self.cpu_oam_read_accessible()
    }

    #[inline]
    pub(in crate::hardware) fn cpu_oam_read_accessible(&self) -> bool {
        !self.cpu_oam_read_blocked_by_ppu()
    }

    #[inline]
    pub(in crate::hardware) fn cpu_oam_write_accessible(&self) -> bool {
        !self.cpu_oam_write_blocked_by_ppu()
    }

    fn cpu_oam_read_blocked_by_ppu(&self) -> bool {
        if !self.lcd_enabled() || self.ly >= 144 {
            return false;
        }

        let (_, end_dot, _, first_line_end_dot) = self.cpu_access_block_window();

        if self.cgb_double_speed && self.cycles == 0 {
            return false;
        }

        if self.blank_first_frame_after_lcd_on && self.ly == 0 {
            return self.cycles >= LCD_ON_INITIAL_MODE0_DOTS && self.cycles < first_line_end_dot;
        }

        if self.blank_first_frame_after_lcd_on && self.ly > 0 && self.ly < 144 && self.cycles < 4 {
            return true;
        }

        self.cycles < end_dot
    }

    fn cpu_oam_write_blocked_by_ppu(&self) -> bool {
        if !self.lcd_enabled() || self.ly >= 144 {
            return false;
        }

        let (_, end_dot, _, first_line_end_dot) = self.cpu_access_block_window();

        if self.blank_first_frame_after_lcd_on && self.ly == 0 {
            let first_line_start_dot = if self.cgb_double_speed {
                LCD_ON_INITIAL_MODE0_DOTS.saturating_sub(4)
            } else {
                LCD_ON_INITIAL_MODE0_DOTS
            };
            return self.cycles >= first_line_start_dot && self.cycles < first_line_end_dot;
        }

        if self.blank_first_frame_after_lcd_on && self.ly > 0 && self.ly < 144 && self.cycles < 4 {
            return true;
        }

        self.cycles < end_dot
    }

    #[inline]
    pub(in crate::hardware::ppu) fn cpu_palette_accessible(&self) -> bool {
        if !self.lcd_enabled() {
            return true;
        }

        self.mode() != 3
    }

    #[cfg(test)]
    pub(super) fn update_stat_interrupt(&mut self) -> bool {
        self.update_stat_interrupt_for_mode(self.stat & 0x03)
    }

    pub(super) fn update_stat_interrupt_for_mode(&mut self, interrupt_mode: u8) -> bool {
        let ly_match = self.ly == self.lyc
            && !(self.blank_first_frame_after_lcd_on && self.ly > 0 && self.cycles < 4);
        if ly_match {
            self.stat |= 0x04;
        } else {
            self.stat &= !0x04;
        }

        let stat_line = (self.stat & 0x40 != 0 && ly_match)
            || (self.stat & 0x20 != 0 && (interrupt_mode == 2 || self.ly == 144))
            || (self.stat & 0x10 != 0 && interrupt_mode == 1)
            || (self.stat & 0x08 != 0 && interrupt_mode == 0);

        let rising_edge = stat_line && !self.prev_stat_line;
        self.prev_stat_line = stat_line;
        rising_edge
    }

    fn update_cpu_stat_mode0_interrupt_before_if(&mut self, cycles: u64, draw_dots: u64) -> bool {
        let mode0_line = self.ly < 144
            && cycles >= STAT_IRQ_OAM_DOTS + draw_dots
            && cycles < DOTS_PER_LINE
            && self.stat & 0x08 != 0;
        let rising_edge = mode0_line && !self.prev_cpu_stat_mode0_line && !self.prev_stat_line;
        self.prev_cpu_stat_mode0_line = mode0_line;
        rising_edge
    }
}
