use super::{
    DOTS_PER_LINE, Lcdc, OAM_DOTS, ObjFetchPhase, PPU, SCANLINES_PER_FRAME, SCREEN_H, SCREEN_W,
    STAT_IRQ_OAM_DOTS, default_framebuffer,
};
use crate::save_state::{StateReader, StateWriter};
use anyhow::{Result, bail};

impl PPU {
    pub fn write_state(&self, writer: &mut StateWriter) {
        writer.write_u8(self.lcdc.bits());
        writer.write_u8(self.stat);
        writer.write_u8(self.scy);
        writer.write_u8(self.scx);
        writer.write_u8(self.ly);
        writer.write_u8(self.lyc);
        writer.write_u8(self.wy);
        writer.write_u8(self.wx);
        writer.write_u8(self.bgp);
        writer.write_u8(self.obp0);
        writer.write_u8(self.obp1);
        writer.write_bytes(&self.bg_palette_ram);
        writer.write_bytes(&self.obj_palette_ram);
        writer.write_u8(self.bcps);
        writer.write_u8(self.ocps);
        writer.write_u64(self.cycles);
        writer.write_bool(self.sgb_enabled);
        writer.write_u8(self.sgb_mask_mode);
        writer.write_u8(self.sgb_active_palette);
        for palette in &self.sgb_palettes {
            for color in palette {
                writer.write_u16(*color);
            }
        }
        writer.write_bool(self.sgb_border_enabled);
        writer.write_bytes(&self.sgb_border_tile_data);
        for entry in &self.sgb_border_tilemap {
            writer.write_u16(*entry);
        }
        for palette in &self.sgb_border_palettes {
            for color in palette {
                writer.write_u16(*color);
            }
        }
        writer.write_bytes(&self.sgb_pal_trn_data);
        writer.write_bytes(&self.sgb_attr_trn_data);
        writer.write_bytes(&self.sgb_attr_map);
        writer.write_bytes(&self.sgb_composite_buffer);
        writer.write_u8(self.window_line_counter);
        writer.write_bool(self.window_was_active_this_frame);
        writer.write_bool(self.window_y_triggered);
        writer.write_bool(self.cgb_mode);
        writer.write_bool(self.rendered_current_line);
        writer.write_bool(self.prev_stat_line);
        writer.write_bool(self.cgb_double_speed);
        writer.write_bool(self.lcd_was_enabled);
        writer.write_bool(self.blank_first_frame_after_lcd_on);
        writer.write_bool(self.prev_cpu_stat_mode0_line);
        writer.write_bool(self.cpu_stat_mode0_pending_before_if);
        writer.write_u64(self.draw_dots_for_line);
        writer.write_u8(self.mode2_cursor);
        writer.write_bytes(&self.selected_obj_indices);
        writer.write_u8(self.selected_obj_count);
        writer.write_bool(self.legacy_sprite_selection_for_line);
        writer.write_u16(self.mode3_obj_fetch_dot);
        writer.write_u8(self.mode3_output_x);
        writer.write_bytes(&self.mode3_output_history);
        writer.write_u8(self.mode3_scx_low);
        writer.write_u16(self.mode3_obj_fetched_mask);
        writer.write_u8(self.mode3_obj_fetch_selection);
        writer.write_u8(self.mode3_obj_fetch_x);
        writer.write_u8(self.mode3_obj_fetch_phase.tag());
        writer.write_u8(self.mode3_obj_fetch_phase_dot);
        writer.write_bool(self.legacy_obj_fetch_for_line);
        writer.write_u16(self.mode3_obj_tile_row_latched_mask);
        writer.write_bytes(&self.mode3_obj_tile_rows);
        writer.write_u16(self.mode3_obj_completed_mask);
    }

    pub fn read_state(reader: &mut StateReader<'_>, format_version: u32) -> Result<Self> {
        let mut ppu = Self::new();
        ppu.lcdc = Lcdc::from_bits_truncate(reader.read_u8()?);
        ppu.stat = reader.read_u8()?;
        ppu.scy = reader.read_u8()?;
        ppu.scx = reader.read_u8()?;
        ppu.ly = reader.read_u8()?;
        ppu.lyc = reader.read_u8()?;
        ppu.wy = reader.read_u8()?;
        ppu.wx = reader.read_u8()?;
        ppu.bgp = reader.read_u8()?;
        ppu.obp0 = reader.read_u8()?;
        ppu.obp1 = reader.read_u8()?;
        reader.read_exact(&mut ppu.bg_palette_ram)?;
        reader.read_exact(&mut ppu.obj_palette_ram)?;
        ppu.bcps = reader.read_u8()?;
        ppu.ocps = reader.read_u8()?;
        ppu.cycles = reader.read_u64()?;
        ppu.sgb_enabled = reader.read_bool()?;
        ppu.sgb_mask_mode = reader.read_u8()?;
        ppu.sgb_active_palette = reader.read_u8()?;
        for palette in &mut ppu.sgb_palettes {
            for color in palette {
                *color = reader.read_u16()?;
            }
        }
        ppu.sgb_border_enabled = reader.read_bool()?;
        reader.read_exact(&mut ppu.sgb_border_tile_data)?;
        for entry in &mut ppu.sgb_border_tilemap {
            *entry = reader.read_u16()?;
        }
        for palette in &mut ppu.sgb_border_palettes {
            for color in palette {
                *color = reader.read_u16()?;
            }
        }
        reader.read_exact(&mut ppu.sgb_pal_trn_data)?;
        reader.read_exact(&mut ppu.sgb_attr_trn_data)?;
        reader.read_exact(&mut ppu.sgb_attr_map)?;
        reader.read_exact(&mut ppu.sgb_composite_buffer)?;
        ppu.window_line_counter = reader.read_u8()?;
        ppu.window_was_active_this_frame = reader.read_bool()?;
        ppu.window_y_triggered = reader.read_bool()?;
        ppu.cgb_mode = reader.read_bool()?;
        ppu.rendered_current_line = reader.read_bool()?;
        ppu.prev_stat_line = reader.read_bool()?;
        ppu.cgb_double_speed = reader.read_bool()?;
        ppu.lcd_was_enabled = reader.read_bool()?;
        ppu.blank_first_frame_after_lcd_on = reader.read_bool()?;
        ppu.prev_cpu_stat_mode0_line = reader.read_bool()?;
        ppu.cpu_stat_mode0_pending_before_if = reader.read_bool()?;
        ppu.draw_dots_for_line = reader.read_u64()?;
        if format_version >= 10 {
            ppu.mode2_cursor = reader.read_u8()?;
            reader.read_exact(&mut ppu.selected_obj_indices)?;
            ppu.selected_obj_count = reader.read_u8()?;
            ppu.legacy_sprite_selection_for_line = reader.read_bool()?;
            let selected = &ppu.selected_obj_indices[..usize::from(ppu.selected_obj_count.min(10))];
            let lcd_timing_active = ppu.lcdc.contains(Lcdc::LCD_ENABLE) && ppu.lcd_was_enabled;
            let lcd_timing_valid =
                lcd_timing_active || (!ppu.lcd_was_enabled && ppu.ly == 0 && ppu.cycles == 0);
            let expected_cursor = if lcd_timing_active && ppu.ly < SCREEN_H as u8 {
                (ppu.cycles.min(STAT_IRQ_OAM_DOTS) / 2) as u8
            } else {
                0
            };
            let just_enabled = lcd_timing_active
                && ppu.blank_first_frame_after_lcd_on
                && ppu.ly == 0
                && ppu.cycles == 4
                && ppu.mode2_cursor == 0;
            let cursor_matches_progress = if ppu.legacy_sprite_selection_for_line {
                ppu.mode2_cursor <= expected_cursor
            } else {
                ppu.mode2_cursor == expected_cursor || just_enabled
            };
            if ppu.ly >= SCANLINES_PER_FRAME
                || ppu.cycles >= DOTS_PER_LINE
                || !lcd_timing_valid
                || ppu.mode2_cursor > 40
                || !cursor_matches_progress
                || ppu.selected_obj_count > 10
                || selected.iter().any(|&index| index >= ppu.mode2_cursor)
                || selected.windows(2).any(|pair| pair[0] >= pair[1])
            {
                bail!("invalid PPU Mode 2 selection state");
            }
        } else {
            ppu.mode2_cursor = 40;
            ppu.selected_obj_count = 0;
            ppu.legacy_sprite_selection_for_line = true;
        }
        if format_version >= 11 {
            ppu.mode3_obj_fetch_dot = reader.read_u16()?;
            ppu.mode3_output_x = reader.read_u8()?;
            reader.read_exact(&mut ppu.mode3_output_history)?;
            ppu.mode3_scx_low = reader.read_u8()?;
            ppu.mode3_obj_fetched_mask = reader.read_u16()?;
            ppu.mode3_obj_fetch_selection = reader.read_u8()?;
            ppu.mode3_obj_fetch_x = reader.read_u8()?;
            ppu.mode3_obj_fetch_phase = ObjFetchPhase::from_tag(reader.read_u8()?)
                .ok_or_else(|| anyhow::anyhow!("invalid PPU Mode 3 OBJ fetch phase"))?;
            ppu.mode3_obj_fetch_phase_dot = reader.read_u8()?;
            ppu.legacy_obj_fetch_for_line = reader.read_bool()?;
            if format_version >= 12 {
                ppu.mode3_obj_tile_row_latched_mask = reader.read_u16()?;
                reader.read_exact(&mut ppu.mode3_obj_tile_rows)?;
                ppu.mode3_obj_completed_mask = reader.read_u16()?;
            } else if ppu.mode3_obj_fetch_dot != 0 {
                ppu.legacy_obj_fetch_for_line = true;
            }
            ppu.validate_mode3_obj_fetch_state()?;
        } else {
            let mode3_start = OAM_DOTS - u64::from(ppu.ly == 0) * 4;
            if ppu.ly < SCREEN_H as u8 && ppu.cycles > mode3_start {
                ppu.legacy_obj_fetch_for_line = true;
            }
        }
        ppu.framebuffer = default_framebuffer();
        Ok(ppu)
    }

    fn validate_mode3_obj_fetch_state(&self) -> Result<()> {
        let selection_count = self.selected_obj_count.min(10);
        let valid_mask = (1u16 << selection_count).wrapping_sub(1);
        let history_valid = self.mode3_output_history[3] == self.mode3_output_x
            && self
                .mode3_output_history
                .windows(2)
                .all(|pair| pair[0] <= pair[1] && pair[1] - pair[0] <= 1);
        let active_phase = self.mode3_obj_fetch_phase != ObjFetchPhase::Idle;
        let active_latched_mask = if self.mode3_obj_fetch_phase == ObjFetchPhase::DataHigh {
            1 << self.mode3_obj_fetch_selection
        } else {
            0
        };
        let allowed_latched_mask = self.mode3_obj_fetched_mask | active_latched_mask;
        let tile_rows_valid = self.mode3_obj_tile_row_latched_mask & !allowed_latched_mask == 0
            && self.mode3_obj_completed_mask & !self.mode3_obj_fetched_mask == 0
            && self.mode3_obj_completed_mask & !self.mode3_obj_tile_row_latched_mask == 0
            && self
                .mode3_obj_tile_rows
                .iter()
                .enumerate()
                .all(|(selection, &row)| {
                    if self.mode3_obj_tile_row_latched_mask & (1 << selection) != 0 {
                        row & !0x1F == 0 && (row & 0x10 != 0 || row & 0x08 == 0)
                    } else {
                        row == 0
                    }
                });
        let phase_limit = match self.mode3_obj_fetch_phase {
            ObjFetchPhase::Idle => 1,
            ObjFetchPhase::Align => 5u8.saturating_sub(
                ((u16::from(self.mode3_obj_fetch_x) + u16::from(self.mode3_scx_low)) & 7) as u8,
            ),
            ObjFetchPhase::Tile | ObjFetchPhase::DataLow | ObjFetchPhase::DataHigh => 2,
        };
        let start_dot = self.mode3_obj_fetch_start_dot();
        let fallback_line = self.legacy_sprite_selection_for_line || self.legacy_obj_fetch_for_line;
        let pipeline_not_required = !self.lcdc.contains(Lcdc::LCD_ENABLE)
            || !self.lcd_was_enabled
            || self.ly >= SCREEN_H as u8
            || self.cycles <= u64::from(start_dot)
            || fallback_line
            || !self.mode3_obj_fetch_pipeline_enabled();
        let not_started = self.mode3_obj_fetch_dot == 0
            && self.mode3_output_x == 0
            && self.mode3_output_history == [0; 4]
            && self.mode3_scx_low == 0
            && self.mode3_obj_fetched_mask == 0
            && self.mode3_obj_fetch_selection == 0
            && self.mode3_obj_fetch_x == 0
            && self.mode3_obj_fetch_phase == ObjFetchPhase::Idle
            && self.mode3_obj_fetch_phase_dot == 0
            && self.mode3_obj_tile_row_latched_mask == 0
            && self.mode3_obj_tile_rows == [0; 10]
            && self.mode3_obj_completed_mask == 0
            && pipeline_not_required;
        let fetch_identity_valid = if self.mode3_obj_fetched_mask == 0 && !active_phase {
            self.mode3_obj_fetch_selection == 0 && self.mode3_obj_fetch_x == 0
        } else {
            self.mode3_obj_fetch_selection < selection_count
        };
        let started = self.mode3_obj_fetch_dot >= start_dot
            && u64::from(self.mode3_obj_fetch_dot) <= self.cycles
            && (fallback_line
                || self.mode3_output_x == SCREEN_W as u8
                || u64::from(self.mode3_obj_fetch_dot) == self.cycles)
            && self.mode3_output_x <= SCREEN_W as u8
            && self.mode3_scx_low <= 7
            && self.mode3_obj_fetched_mask & !valid_mask == 0
            && self.mode3_obj_tile_row_latched_mask & !valid_mask == 0
            && self.mode3_obj_completed_mask & !valid_mask == 0
            && tile_rows_valid
            && history_valid
            && fetch_identity_valid
            && (!active_phase
                || (self.mode3_obj_fetch_selection < selection_count
                    && self.mode3_obj_fetched_mask & (1 << self.mode3_obj_fetch_selection) == 0))
            && self.mode3_obj_fetch_phase_dot < phase_limit;
        if !not_started && !started {
            bail!("invalid PPU Mode 3 OBJ fetch state");
        }
        Ok(())
    }
}
