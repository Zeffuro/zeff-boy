use super::{
    DOTS_PER_LINE, DRAW_DOTS_BASE, LCD_ON_INITIAL_MODE0_DOTS, Lcdc, OAM_DOTS, ObjFetchPhase, PPU,
    SCANLINES_PER_FRAME, SCREEN_H, SCREEN_W, STAT_IRQ_HBLANK_DELAY_DOTS, STAT_IRQ_OAM_DOTS,
    renderer,
};

impl PPU {
    pub(in crate::hardware) fn write_lcdc_bg_enable_with_video(
        &mut self,
        value: u8,
        vram: &[u8],
        oam: &[u8],
    ) -> u8 {
        let next_lcdc = Lcdc::from_bits_truncate(value);
        let retained_pixels = self.dmg_lcdc_bg_enable_retained_pixels();
        let rendered_line = (!self.sgb_enabled && self.lcdc ^ next_lcdc == Lcdc::BG_ENABLE)
            .then(|| self.begin_dmg_mid_scanline_register_change(retained_pixels, vram, oam))
            .flatten();
        let interrupts = self.write_lcdc(value);
        self.finish_dmg_mid_scanline_register_change(rendered_line, vram, oam);
        interrupts
    }

    pub(super) fn dmg_lcdc_bg_enable_retained_pixels(&self) -> u8 {
        let Some(output_x) = self.mode3_obj_fetch_output_x_for_write() else {
            return 1;
        };
        let selection_mask = 1 << self.mode3_obj_fetch_selection;
        let fetch_active_or_complete = self.mode3_obj_fetch_phase != ObjFetchPhase::Idle
            || self.mode3_obj_fetched_mask & selection_mask != 0;
        let trigger_x = self.mode3_obj_fetch_x.saturating_sub(8);
        u8::from(
            !self.lcdc.contains(Lcdc::OBJ_ENABLE)
                || !fetch_active_or_complete
                || output_x != trigger_x,
        )
    }

    pub(in crate::hardware) fn write_wx_with_video(&mut self, value: u8, vram: &[u8], oam: &[u8]) {
        let rendered_line = (value != self.wx
            && self.window_enable_condition()
            && self.window_y_triggered
            && (self.wx <= 166 || value <= 166))
            .then(|| self.begin_dmg_mid_scanline_register_change(0, vram, oam))
            .flatten();
        self.write_wx(value);
        self.finish_dmg_mid_scanline_register_change(rendered_line, vram, oam);
    }

    pub(in crate::hardware) fn cpu_lcdc_write_needs_early_obj_cancel(&self, value: u8) -> bool {
        let next_lcdc = Lcdc::from_bits_truncate(value);
        self.mode3_obj_fetch_timing_active()
            && self.lcdc.contains(Lcdc::OBJ_ENABLE)
            && !next_lcdc.contains(Lcdc::OBJ_ENABLE)
            && self.mode3_obj_fetch_x >= 8
    }

    pub(in crate::hardware) fn prepare_cpu_lcdc_write(&mut self, value: u8) {
        if self.cpu_lcdc_write_needs_early_obj_cancel(value) {
            self.cancel_active_obj_fetch();
        }
    }

    pub(in crate::hardware) fn write_lcdc(&mut self, value: u8) -> u8 {
        let was_enabled = self.lcdc.contains(Lcdc::LCD_ENABLE);
        let next_lcdc = Lcdc::from_bits_truncate(value);
        if self.mode3_obj_fetch_timing_active() {
            let changed = self.lcdc ^ next_lcdc;
            if changed.contains(Lcdc::WINDOW_ENABLE)
                || (changed.contains(Lcdc::OBJ_SIZE) && !self.mode3_obj_fetch_pipeline_enabled())
            {
                self.legacy_obj_fetch_for_line = true;
            } else if changed.contains(Lcdc::OBJ_ENABLE) {
                if next_lcdc.contains(Lcdc::OBJ_ENABLE) {
                    if self.mode3_obj_fetch_dot == 0 {
                        self.legacy_obj_fetch_for_line = true;
                    }
                } else {
                    self.cancel_active_obj_fetch();
                }
            }
        }
        self.lcdc = next_lcdc;
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

    pub(in crate::hardware) fn write_wy(&mut self, value: u8) {
        self.mark_mode3_window_position_change();
        self.wy = value;
    }

    pub(in crate::hardware) fn write_wx(&mut self, value: u8) {
        self.mark_mode3_window_position_change();
        self.wx = value;
    }

    fn mark_mode3_window_position_change(&mut self) {
        if self.mode3_obj_fetch_timing_active() {
            self.legacy_obj_fetch_for_line = true;
        }
    }

    fn mode3_obj_fetch_timing_active(&self) -> bool {
        !self.cgb_mode
            && self.lcdc.contains(Lcdc::LCD_ENABLE)
            && self.lcd_was_enabled
            && self.ly < SCREEN_H as u8
            && self.cycles >= u64::from(self.mode3_obj_fetch_start_dot())
    }

    fn begin_dmg_mid_scanline_register_change(
        &mut self,
        retained_pixels: u8,
        vram: &[u8],
        oam: &[u8],
    ) -> Option<bool> {
        let write_dot = self.cycles.saturating_sub(3);
        if self.cgb_mode
            || self.ly >= SCREEN_H as u8
            || self.blank_first_frame_after_lcd_on
            || write_dot < OAM_DOTS
            || write_dot >= OAM_DOTS + self.draw_dots_for_line
        {
            return None;
        }

        let startup_dots = DRAW_DOTS_BASE - SCREEN_W as u64;
        let output_dot = write_dot + u64::from(self.ly == 0) * 4;
        let x = self
            .mode3_obj_fetch_output_x_for_write()
            .unwrap_or_else(|| {
                output_dot
                    .saturating_sub(OAM_DOTS + startup_dots)
                    .min(SCREEN_W as u64) as u8
            })
            .saturating_add(retained_pixels)
            .min(SCREEN_W as u8);
        let line_was_rendered = self.rendered_current_line;
        if line_was_rendered {
            self.dmg_rendered_x = self.dmg_rendered_x.min(x);
        }
        self.render_dmg_until(x, vram, oam);
        Some(line_was_rendered)
    }

    fn finish_dmg_mid_scanline_register_change(
        &mut self,
        rendered_line: Option<bool>,
        vram: &[u8],
        oam: &[u8],
    ) {
        if rendered_line == Some(true) {
            self.render_dmg_until(SCREEN_W as u8, vram, oam);
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
        self.reset_mode2_selection();
        self.reset_mode3_obj_fetch();
        self.rendered_current_line = false;
        self.dmg_rendered_x = 0;
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
        self.reset_mode2_selection();
        self.reset_mode3_obj_fetch();
        self.rendered_current_line = false;
        self.dmg_rendered_x = 0;
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

    fn reset_mode2_selection(&mut self) {
        self.mode2_cursor = 0;
        self.selected_obj_indices = [0; 10];
        self.selected_obj_count = 0;
        self.legacy_sprite_selection_for_line = false;
    }

    fn reset_mode3_obj_fetch(&mut self) {
        self.mode3_obj_fetch_dot = 0;
        self.mode3_output_x = 0;
        self.mode3_output_history = [0; 4];
        self.mode3_scx_low = 0;
        self.mode3_obj_fetched_mask = 0;
        self.mode3_obj_fetch_selection = 0;
        self.mode3_obj_fetch_x = 0;
        self.mode3_obj_fetch_phase = ObjFetchPhase::Idle;
        self.mode3_obj_fetch_phase_dot = 0;
        self.mode3_obj_tile_row_latched_mask = 0;
        self.mode3_obj_tile_rows = [0; 10];
        self.mode3_obj_completed_mask = 0;
        self.legacy_obj_fetch_for_line = false;
    }

    #[inline]
    pub(in crate::hardware) fn mark_mode2_selection_legacy_for_line(&mut self) {
        self.legacy_sprite_selection_for_line = true;
        self.legacy_obj_fetch_for_line = true;
    }

    #[cfg(test)]
    pub(in crate::hardware) fn mode2_selection_is_legacy_for_line(&self) -> bool {
        self.legacy_sprite_selection_for_line
    }

    #[cfg(test)]
    pub(in crate::hardware) fn obj_fetch_is_legacy_for_line(&self) -> bool {
        self.legacy_obj_fetch_for_line
    }

    fn advance_mode2_selection(&mut self, target_dot: u64, oam: &[u8]) {
        if self.ly >= SCREEN_H as u8 || self.legacy_sprite_selection_for_line {
            return;
        }

        let examined = (target_dot.min(STAT_IRQ_OAM_DOTS) / 2) as u8;
        let sprite_h = if self.lcdc.contains(Lcdc::OBJ_SIZE) {
            16
        } else {
            8
        };

        while self.mode2_cursor < examined {
            let index = self.mode2_cursor;
            self.mode2_cursor += 1;
            if self.selected_obj_count >= 10 {
                continue;
            }

            let base = usize::from(index) * 4;
            let sy = i32::from(oam.get(base).copied().unwrap_or(0)) - 16;
            if i32::from(self.ly) >= sy && i32::from(self.ly) < sy + sprite_h {
                self.selected_obj_indices[usize::from(self.selected_obj_count)] = index;
                self.selected_obj_count += 1;
            }
        }
    }

    pub(super) fn mode3_obj_fetch_start_dot(&self) -> u16 {
        (OAM_DOTS - u64::from(self.ly == 0) * 4) as u16
    }

    pub(super) fn mode3_obj_fetch_pipeline_enabled(&self) -> bool {
        !self.cgb_mode && self.selected_obj_count != 0 && !self.window_visible_on_current_line()
    }

    fn advance_mode3_obj_fetch(&mut self, target_dot: u64, oam: &[u8]) {
        if self.ly >= SCREEN_H as u8
            || self.legacy_sprite_selection_for_line
            || self.legacy_obj_fetch_for_line
            || !self.mode3_obj_fetch_pipeline_enabled()
        {
            return;
        }

        let start_dot = self.mode3_obj_fetch_start_dot();
        let target_dot = target_dot.min(DOTS_PER_LINE) as u16;
        if target_dot <= start_dot {
            return;
        }

        if self.mode3_obj_fetch_dot == 0 {
            self.mode3_obj_fetch_dot = start_dot;
            self.mode3_scx_low = self.scx & 7;
        }

        while self.mode3_obj_fetch_dot < target_dot {
            self.advance_mode3_obj_fetch_one_dot(oam, start_dot);
            self.mode3_obj_fetch_dot += 1;
            self.mode3_output_history.rotate_left(1);
            self.mode3_output_history[3] = self.mode3_output_x;
        }
    }

    fn advance_mode3_obj_fetch_one_dot(&mut self, oam: &[u8], start_dot: u16) {
        let startup_end = start_dot + 12 + u16::from(self.mode3_scx_low);
        if self.mode3_obj_fetch_dot < startup_end || self.mode3_output_x >= SCREEN_W as u8 {
            return;
        }

        if self.mode3_obj_fetch_phase != ObjFetchPhase::Idle {
            self.advance_active_obj_fetch(oam);
            return;
        }

        if !self.lcdc.contains(Lcdc::OBJ_ENABLE) {
            self.skip_disabled_objs(oam);
        } else if let Some(selection) = self.next_obj_fetch_selection(oam) {
            self.begin_obj_fetch(selection, oam);
            self.advance_active_obj_fetch(oam);
            return;
        }

        self.mode3_output_x += 1;
    }

    fn cancel_active_obj_fetch(&mut self) {
        if self.mode3_obj_fetch_phase == ObjFetchPhase::Idle {
            return;
        }

        self.mode3_obj_fetched_mask |= 1 << self.mode3_obj_fetch_selection;
        self.mode3_obj_fetch_phase = ObjFetchPhase::Idle;
        self.mode3_obj_fetch_phase_dot = 0;
    }

    fn skip_disabled_objs(&mut self, oam: &[u8]) {
        for selection in 0..self.selected_obj_count.min(10) {
            let bit = 1 << selection;
            if self.mode3_obj_fetched_mask & bit != 0 {
                continue;
            }

            let index = self.selected_obj_indices[usize::from(selection)];
            let x = oam.get(usize::from(index) * 4 + 1).copied().unwrap_or(0);
            if x >= 168 || x.saturating_sub(8) <= self.mode3_output_x {
                self.mode3_obj_fetched_mask |= bit;
            }
        }
    }

    fn next_obj_fetch_selection(&self, oam: &[u8]) -> Option<u8> {
        let mut next = None::<(u8, u8)>;
        for selection in 0..self.selected_obj_count.min(10) {
            if self.mode3_obj_fetched_mask & (1 << selection) != 0 {
                continue;
            }

            let index = self.selected_obj_indices[usize::from(selection)];
            let x = oam.get(usize::from(index) * 4 + 1).copied().unwrap_or(0);
            if x >= 168 || x.saturating_sub(8) > self.mode3_output_x {
                continue;
            }

            let candidate = (x, selection);
            if next.is_none_or(|current| candidate < current) {
                next = Some(candidate);
            }
        }
        next.map(|(_, selection)| selection)
    }

    fn begin_obj_fetch(&mut self, selection: u8, oam: &[u8]) {
        let index = self.selected_obj_indices[usize::from(selection)];
        let x = oam.get(usize::from(index) * 4 + 1).copied().unwrap_or(0);
        let align_dots =
            5u8.saturating_sub(((u16::from(x) + u16::from(self.mode3_scx_low)) & 7) as u8);
        self.mode3_obj_fetch_selection = selection;
        self.mode3_obj_fetch_x = x;
        self.mode3_obj_fetch_phase = if align_dots == 0 {
            ObjFetchPhase::Tile
        } else {
            ObjFetchPhase::Align
        };
        self.mode3_obj_fetch_phase_dot = 0;
    }

    fn advance_active_obj_fetch(&mut self, oam: &[u8]) {
        self.mode3_obj_fetch_phase_dot += 1;
        let phase_dots = match self.mode3_obj_fetch_phase {
            ObjFetchPhase::Idle => return,
            ObjFetchPhase::Align => 5u8.saturating_sub(
                ((u16::from(self.mode3_obj_fetch_x) + u16::from(self.mode3_scx_low)) & 7) as u8,
            ),
            ObjFetchPhase::Tile | ObjFetchPhase::DataLow | ObjFetchPhase::DataHigh => 2,
        };
        if self.mode3_obj_fetch_phase_dot < phase_dots {
            return;
        }

        self.mode3_obj_fetch_phase_dot = 0;
        self.mode3_obj_fetch_phase = match self.mode3_obj_fetch_phase {
            ObjFetchPhase::Idle => ObjFetchPhase::Idle,
            ObjFetchPhase::Align => ObjFetchPhase::Tile,
            ObjFetchPhase::Tile => ObjFetchPhase::DataLow,
            ObjFetchPhase::DataLow => {
                self.latch_active_obj_tile_row(oam);
                ObjFetchPhase::DataHigh
            }
            ObjFetchPhase::DataHigh => {
                let bit = 1 << self.mode3_obj_fetch_selection;
                self.mode3_obj_fetched_mask |= bit;
                self.mode3_obj_completed_mask |= bit;
                ObjFetchPhase::Idle
            }
        };
    }

    fn latch_active_obj_tile_row(&mut self, oam: &[u8]) {
        let selection = usize::from(self.mode3_obj_fetch_selection);
        let index = usize::from(self.selected_obj_indices[selection]);
        let base = index * 4;
        let sprite_y = oam.get(base).copied().unwrap_or(0);
        let flip_y = oam.get(base + 3).copied().unwrap_or(0) & 0x40 != 0;
        let tall = self.lcdc.contains(Lcdc::OBJ_SIZE);
        let height = if tall { 16 } else { 8 };
        let mut row = self.ly.wrapping_add(16).wrapping_sub(sprite_y) & (height - 1);
        if flip_y {
            row = height - 1 - row;
        }
        self.mode3_obj_tile_rows[selection] = row | (u8::from(tall) << 4);
        self.mode3_obj_tile_row_latched_mask |= 1 << selection;
    }

    fn mode3_obj_fetch_output_x_for_write(&self) -> Option<u8> {
        let pipeline_current = self.mode3_obj_fetch_dot == self.cycles as u16;
        (self.mode3_obj_fetch_pipeline_enabled()
            && pipeline_current
            && !self.legacy_sprite_selection_for_line
            && !self.legacy_obj_fetch_for_line)
            .then_some(self.mode3_output_history[0])
    }

    pub(super) fn dmg_output_x_for_write_dot(&self, write_dot: u64) -> u8 {
        if let Some(x) = self.mode3_obj_fetch_output_x_for_write() {
            return x;
        }

        let startup_dots = DRAW_DOTS_BASE - SCREEN_W as u64;
        let output_dot = write_dot + u64::from(self.ly == 0) * 4;
        let mut x = output_dot
            .saturating_sub(OAM_DOTS + startup_dots + u64::from(self.scx & 7))
            .min(SCREEN_W as u64) as u8;
        if self.window_visible_on_current_line() {
            let window_x = self.wx.saturating_sub(7).min(SCREEN_W as u8);
            if x > window_x {
                let stall = 6 + u8::from(self.wx == 0 && self.scx & 7 != 0);
                x = window_x.max(x.saturating_sub(stall));
            }
        }
        x
    }

    fn compute_draw_dots_for_line(&self, oam: &[u8]) -> u64 {
        if self.ly >= SCREEN_H as u8 {
            return DRAW_DOTS_BASE;
        }

        let scx_penalty = (self.scx & 7) as u64;

        let sprite_penalty = if self.lcdc.contains(Lcdc::OBJ_ENABLE) {
            if self.legacy_sprite_selection_for_line {
                self.legacy_sprite_fetch_penalty_dots(oam)
            } else {
                self.sprite_fetch_penalty_dots(oam)
            }
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
        let scx = u16::from(self.scx & 7);
        let mut penalty = 0u64;
        let mut bucket_stalls = [0u8; 22];
        let selected = &self.selected_obj_indices[..usize::from(self.selected_obj_count.min(10))];
        if selected
            .iter()
            .any(|&index| oam.get(usize::from(index) * 4 + 1).copied() == Some(0))
        {
            penalty += u64::from(scx);
        }

        for &index in selected {
            let x = oam.get(usize::from(index) * 4 + 1).copied().unwrap_or(0);
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

    fn legacy_sprite_fetch_penalty_dots(&self, oam: &[u8]) -> u64 {
        let tall = self.lcdc.contains(Lcdc::OBJ_SIZE);
        let sprite_h: i32 = if tall { 16 } else { 8 };
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

        let scx = u16::from(self.scx & 7);
        let mut penalty = u64::from(selected.contains(&0)) * u64::from(scx);
        let mut bucket_stalls = [0u8; 22];
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

    fn render_dmg_until(&mut self, x: u8, vram: &[u8], oam: &[u8]) {
        let x = x.min(SCREEN_W as u8);
        if x <= self.dmg_rendered_x || self.ly >= SCREEN_H as u8 {
            return;
        }

        if self.dmg_rendered_x == 0 {
            renderer::render_scanline_dmg(self, vram, oam);
            self.dmg_rendered_x = x;
            return;
        }

        let line_start = usize::from(self.ly) * SCREEN_W * 4;
        let prefix_len = usize::from(self.dmg_rendered_x) * 4;
        let mut prefix = [0; SCREEN_W * 4];
        prefix[..prefix_len]
            .copy_from_slice(&self.framebuffer[line_start..line_start + prefix_len]);
        renderer::render_scanline_dmg(self, vram, oam);
        self.framebuffer[line_start..line_start + prefix_len]
            .copy_from_slice(&prefix[..prefix_len]);
        self.dmg_rendered_x = x;
    }

    pub(in crate::hardware) fn write_bgp(&mut self, value: u8, vram: &[u8], oam: &[u8]) {
        let write_dot = self.cycles.saturating_sub(3);
        if self.cgb_mode
            || self.ly >= SCREEN_H as u8
            || self.blank_first_frame_after_lcd_on
            || write_dot < OAM_DOTS
            || write_dot >= OAM_DOTS + self.draw_dots_for_line
        {
            self.bgp = value;
            return;
        }

        let x = self.dmg_output_x_for_write_dot(write_dot);
        let line_was_rendered = self.rendered_current_line;
        if line_was_rendered {
            self.dmg_rendered_x = self.dmg_rendered_x.min(x);
        }
        let previous = self.bgp;
        self.render_dmg_until(x, vram, oam);
        if x != 0 && x < SCREEN_W as u8 && previous != value {
            self.bgp = previous | value;
            self.render_dmg_until(x + 1, vram, oam);
        }
        self.bgp = value;
        if line_was_rendered {
            self.render_dmg_until(SCREEN_W as u8, vram, oam);
        }
    }

    #[cfg(test)]
    #[inline]
    pub(in crate::hardware) fn step(
        &mut self,
        cycles: u64,
        vram: &[u8],
        oam: &[u8],
        cgb_mode: bool,
    ) -> u8 {
        self.step_with_oam_dma(cycles, vram, oam, cgb_mode, false)
    }

    #[inline]
    pub(in crate::hardware) fn step_with_oam_dma(
        &mut self,
        cycles: u64,
        vram: &[u8],
        oam: &[u8],
        cgb_mode: bool,
        oam_dma_active: bool,
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

        if oam_dma_active {
            self.legacy_sprite_selection_for_line = true;
            self.legacy_obj_fetch_for_line = true;
        }

        let target_dot = self.cycles.saturating_add(cycles).min(DOTS_PER_LINE);
        self.advance_mode2_selection(target_dot, oam);
        if self.ly < SCREEN_H as u8 && !self.rendered_current_line {
            self.draw_dots_for_line = self.compute_draw_dots_for_line(oam);
        }
        if !cgb_mode {
            self.advance_mode3_obj_fetch(target_dot, oam);
        }

        let previous_mode = self.stat & 0x03;

        self.cycles += cycles;

        let should_render_output = !self.blank_first_frame_after_lcd_on;
        let draw_dots = self.draw_dots_for_line;

        if !self.rendered_current_line && self.cycles >= OAM_DOTS + draw_dots {
            if self.ly < SCREEN_H as u8 && should_render_output {
                if cgb_mode {
                    renderer::render_scanline_cgb(self, vram, oam);
                } else {
                    self.render_dmg_until(SCREEN_W as u8, vram, oam);
                }
            }
            self.rendered_current_line = true;
        }

        while self.cycles >= DOTS_PER_LINE {
            self.cycles -= DOTS_PER_LINE;

            if !self.rendered_current_line && self.ly < SCREEN_H as u8 && should_render_output {
                if cgb_mode {
                    renderer::render_scanline_cgb(self, vram, oam);
                } else {
                    self.render_dmg_until(SCREEN_W as u8, vram, oam);
                }
            }

            if self.ly < SCREEN_H as u8 {
                self.increment_window_line_counter_after_scanline();
            }

            self.ly += 1;
            self.rendered_current_line = false;
            self.dmg_rendered_x = 0;
            self.reset_mode2_selection();
            self.reset_mode3_obj_fetch();
            if oam_dma_active {
                self.legacy_sprite_selection_for_line = true;
                self.legacy_obj_fetch_for_line = true;
            }

            if self.ly == SCREEN_H as u8 {
                interrupts |= 0x01;
            }

            if self.ly >= SCANLINES_PER_FRAME {
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

            self.advance_mode2_selection(self.cycles.min(DOTS_PER_LINE), oam);
            if self.ly < SCREEN_H as u8 {
                self.draw_dots_for_line = self.compute_draw_dots_for_line(oam);
            }
            if !cgb_mode {
                self.advance_mode3_obj_fetch(self.cycles.min(DOTS_PER_LINE), oam);
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
        if self.ly >= SCREEN_H as u8 {
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
        if self.ly >= SCREEN_H as u8 {
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
        if !self.lcd_enabled() || self.ly >= SCREEN_H as u8 {
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
        if !self.lcd_enabled() || self.ly >= SCREEN_H as u8 {
            return false;
        }

        let (_, end_dot, _, first_line_end_dot) = self.cpu_access_block_window();

        if self.cgb_double_speed && self.cycles == 0 {
            return false;
        }

        if self.blank_first_frame_after_lcd_on && self.ly == 0 {
            return self.cycles >= LCD_ON_INITIAL_MODE0_DOTS && self.cycles < first_line_end_dot;
        }

        if self.blank_first_frame_after_lcd_on
            && self.ly > 0
            && self.ly < SCREEN_H as u8
            && self.cycles < 4
        {
            return true;
        }

        self.cycles < end_dot
    }

    fn cpu_oam_write_blocked_by_ppu(&self) -> bool {
        if !self.lcd_enabled() || self.ly >= SCREEN_H as u8 {
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

        if self.blank_first_frame_after_lcd_on
            && self.ly > 0
            && self.ly < SCREEN_H as u8
            && self.cycles < 4
        {
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
            || (self.stat & 0x20 != 0 && (interrupt_mode == 2 || self.ly == SCREEN_H as u8))
            || (self.stat & 0x10 != 0 && interrupt_mode == 1)
            || (self.stat & 0x08 != 0 && interrupt_mode == 0);

        let rising_edge = stat_line && !self.prev_stat_line;
        self.prev_stat_line = stat_line;
        rising_edge
    }

    fn update_cpu_stat_mode0_interrupt_before_if(&mut self, cycles: u64, draw_dots: u64) -> bool {
        let mode0_line = self.ly < SCREEN_H as u8
            && cycles >= STAT_IRQ_OAM_DOTS + draw_dots
            && cycles < DOTS_PER_LINE
            && self.stat & 0x08 != 0;
        let rising_edge = mode0_line && !self.prev_cpu_stat_mode0_line && !self.prev_stat_line;
        self.prev_cpu_stat_mode0_line = mode0_line;
        rising_edge
    }
}
