use super::*;

#[test]
fn dmg_lcdc_bg_enable_write_preserves_output_prefix() {
    let mut ppu = PPU::new();
    let mut vram = [0u8; 0x4000];
    let oam = [0u8; 160];
    for row in 0..8 {
        vram[row * 2] = 0xFF;
    }
    ppu.lcdc = Lcdc::LCD_ENABLE | Lcdc::TILE_DATA | Lcdc::BG_ENABLE;
    ppu.lcd_was_enabled = true;
    ppu.ly = 1;
    ppu.bgp = 0xE4;
    ppu.dmg_palette_preset = DmgPalettePreset::Gray;
    ppu.step(
        OAM_DOTS + DRAW_DOTS_BASE - SCREEN_W as u64 + 8 + 3,
        &vram,
        &oam,
        false,
    );

    ppu.write_lcdc_bg_enable_with_video((ppu.lcdc - Lcdc::BG_ENABLE).bits(), &vram, &oam);
    ppu.step(8, &vram, &oam, false);
    ppu.write_lcdc_bg_enable_with_video((ppu.lcdc | Lcdc::BG_ENABLE).bits(), &vram, &oam);
    ppu.step(DOTS_PER_LINE - 1 - ppu.cycles, &vram, &oam, false);

    let line_start = SCREEN_W * 4;
    let pixel = |x: usize| &ppu.framebuffer[line_start + x * 4..line_start + x * 4 + 4];
    assert_eq!(
        pixel(7),
        apply_dmg_palette(DmgPalettePreset::Gray, ppu.bgp, 1)
    );
    assert_eq!(
        pixel(8),
        apply_dmg_palette(DmgPalettePreset::Gray, ppu.bgp, 1)
    );
    assert_eq!(
        pixel(9),
        apply_dmg_palette(DmgPalettePreset::Gray, ppu.bgp, 0)
    );
    assert_eq!(
        pixel(16),
        apply_dmg_palette(DmgPalettePreset::Gray, ppu.bgp, 0)
    );
    assert_eq!(
        pixel(17),
        apply_dmg_palette(DmgPalettePreset::Gray, ppu.bgp, 1)
    );
}

#[test]
fn dmg_lcdc_bg_enable_obj_trigger_has_no_extra_pixel_delay() {
    let mut ppu = PPU::new();
    ppu.lcdc = Lcdc::LCD_ENABLE | Lcdc::BG_ENABLE | Lcdc::OBJ_ENABLE;
    ppu.selected_obj_count = 1;
    ppu.selected_obj_indices[0] = 0;
    ppu.cycles = 100;
    ppu.mode3_obj_fetch_dot = 100;
    ppu.mode3_output_x = 4;
    ppu.mode3_output_history = [4; 4];
    ppu.mode3_obj_fetch_x = 12;
    ppu.mode3_obj_fetch_phase = ObjFetchPhase::DataHigh;

    assert_eq!(ppu.dmg_lcdc_bg_enable_retained_pixels(), 0);

    ppu.mode3_obj_fetch_phase = ObjFetchPhase::Idle;
    ppu.mode3_obj_fetched_mask = 1;
    assert_eq!(ppu.dmg_lcdc_bg_enable_retained_pixels(), 0);

    ppu.mode3_output_history = [5; 4];
    assert_eq!(ppu.dmg_lcdc_bg_enable_retained_pixels(), 1);
}

#[test]
fn dmg_window_entry_stall_maps_write_dot_to_output() {
    let mut ppu = PPU::new();
    ppu.lcdc = Lcdc::LCD_ENABLE | Lcdc::WINDOW_ENABLE;
    ppu.lcd_was_enabled = true;
    ppu.window_y_triggered = true;
    ppu.ly = 1;
    ppu.wx = 11;
    let mode3_output_start = OAM_DOTS + DRAW_DOTS_BASE - SCREEN_W as u64;

    assert_eq!(ppu.dmg_output_x_for_write_dot(mode3_output_start + 9), 4);

    ppu.wx = 0;
    ppu.scx = 1;
    assert_eq!(ppu.dmg_output_x_for_write_dot(mode3_output_start + 17), 9);
}

#[test]
fn dmg_wx_write_preserves_rendered_prefix_and_state_converges() {
    let mut ppu = PPU::new();
    let mut vram = [0u8; 0x4000];
    let oam = [0u8; 160];
    vram[0x1800] = 0;
    vram[0x1C00] = 1;
    vram[2] = 0xFF;
    vram[16] = 0;
    vram[17] = 0xFF;
    ppu.lcdc = Lcdc::LCD_ENABLE
        | Lcdc::TILE_DATA
        | Lcdc::BG_ENABLE
        | Lcdc::WINDOW_ENABLE
        | Lcdc::WINDOW_TILEMAP;
    ppu.lcd_was_enabled = true;
    ppu.ly = 1;
    ppu.window_y_triggered = true;
    ppu.wx = 167;
    ppu.bgp = 0xE4;
    ppu.dmg_palette_preset = DmgPalettePreset::Gray;
    ppu.step(
        OAM_DOTS + (DRAW_DOTS_BASE - SCREEN_W as u64) + 5 + 3,
        &vram,
        &oam,
        false,
    );

    ppu.write_wx_with_video(7, &vram, &oam);

    let mut writer = crate::save_state::StateWriter::new();
    ppu.write_state(&mut writer);
    let bytes = writer.into_bytes();
    let mut reader = crate::save_state::StateReader::new(&bytes);
    let mut restored =
        PPU::read_state(&mut reader, crate::save_state::SAVE_STATE_FORMAT_VERSION).unwrap();
    let remaining = DOTS_PER_LINE - ppu.cycles;
    ppu.step(remaining, &vram, &oam, false);
    restored.step(remaining, &vram, &oam, false);

    let line_start = SCREEN_W * 4;
    assert_eq!(
        &ppu.framebuffer[line_start + 16..line_start + 20],
        apply_dmg_palette(DmgPalettePreset::Gray, ppu.bgp, 1)
    );
    assert_eq!(
        &ppu.framebuffer[line_start + 20..line_start + 24],
        apply_dmg_palette(DmgPalettePreset::Gray, ppu.bgp, 2)
    );
    let mut expected = crate::save_state::StateWriter::new();
    ppu.write_state(&mut expected);
    let mut actual = crate::save_state::StateWriter::new();
    restored.write_state(&mut actual);
    assert_eq!(actual.into_bytes(), expected.into_bytes());
}

#[test]
fn dmg_bgp_write_during_mode3_keeps_prior_pixels_and_ors_transition_pixel() {
    let mut ppu = PPU::new();
    let mut vram = [0u8; 0x4000];
    let oam = [0u8; 160];
    for row in 0..8 {
        vram[row * 2] = 0xFF;
    }

    let old_bgp = 0xE4;
    let new_bgp = 0xE8;
    ppu.lcdc = Lcdc::LCD_ENABLE | Lcdc::TILE_DATA | Lcdc::BG_ENABLE;
    ppu.lcd_was_enabled = true;
    ppu.ly = 1;
    ppu.bgp = old_bgp;
    ppu.dmg_palette_preset = DmgPalettePreset::Gray;
    ppu.cycles = OAM_DOTS + (DRAW_DOTS_BASE - SCREEN_W as u64) + 8 + 3;

    ppu.write_bgp(new_bgp, &vram, &oam);
    ppu.step(OAM_DOTS + DRAW_DOTS_BASE - ppu.cycles, &vram, &oam, false);

    let line_start = SCREEN_W * 4;
    let pixel = |x: usize| &ppu.framebuffer[line_start + x * 4..line_start + x * 4 + 4];
    assert_eq!(
        pixel(7),
        apply_dmg_palette(DmgPalettePreset::Gray, old_bgp, 1)
    );
    assert_eq!(
        pixel(8),
        apply_dmg_palette(DmgPalettePreset::Gray, old_bgp | new_bgp, 1)
    );
    assert_eq!(
        pixel(9),
        apply_dmg_palette(DmgPalettePreset::Gray, new_bgp, 1)
    );
}

#[test]
fn dmg_bgp_write_finishing_at_hblank_updates_the_last_pixels() {
    let mut ppu = PPU::new();
    let mut vram = [0u8; 0x4000];
    let oam = [0u8; 160];
    for row in 0..8 {
        vram[row * 2] = 0xFF;
    }

    let old_bgp = 0xE4;
    let new_bgp = 0xE8;
    ppu.lcdc = Lcdc::LCD_ENABLE | Lcdc::TILE_DATA | Lcdc::BG_ENABLE;
    ppu.lcd_was_enabled = true;
    ppu.ly = 1;
    ppu.bgp = old_bgp;
    ppu.dmg_palette_preset = DmgPalettePreset::Gray;
    ppu.cycles = OAM_DOTS + DRAW_DOTS_BASE - 4;

    ppu.step(4, &vram, &oam, false);
    ppu.write_bgp(new_bgp, &vram, &oam);

    let line_start = SCREEN_W * 4;
    let pixel = |x: usize| &ppu.framebuffer[line_start + x * 4..line_start + x * 4 + 4];
    assert_eq!(
        pixel(156),
        apply_dmg_palette(DmgPalettePreset::Gray, old_bgp, 1)
    );
    assert_eq!(
        pixel(157),
        apply_dmg_palette(DmgPalettePreset::Gray, old_bgp | new_bgp, 1)
    );
    assert_eq!(
        pixel(158),
        apply_dmg_palette(DmgPalettePreset::Gray, new_bgp, 1)
    );
}

#[test]
fn dmg_bgp_write_on_line_zero_uses_the_later_output_phase() {
    let mut ppu = PPU::new();
    let mut vram = [0u8; 0x4000];
    let oam = [0u8; 160];
    for row in 0..8 {
        vram[row * 2] = 0xFF;
    }

    let old_bgp = 0xE4;
    let new_bgp = 0xE8;
    ppu.lcd_was_enabled = true;
    ppu.blank_first_frame_after_lcd_on = false;
    ppu.bgp = old_bgp;
    ppu.dmg_palette_preset = DmgPalettePreset::Gray;
    ppu.cycles = OAM_DOTS + (DRAW_DOTS_BASE - SCREEN_W as u64) + 8 + 3;

    ppu.write_bgp(new_bgp, &vram, &oam);
    ppu.step(OAM_DOTS + DRAW_DOTS_BASE - ppu.cycles, &vram, &oam, false);

    let pixel = |x: usize| &ppu.framebuffer[x * 4..x * 4 + 4];
    assert_eq!(
        pixel(11),
        apply_dmg_palette(DmgPalettePreset::Gray, old_bgp, 1)
    );
    assert_eq!(
        pixel(12),
        apply_dmg_palette(DmgPalettePreset::Gray, old_bgp | new_bgp, 1)
    );
    assert_eq!(
        pixel(13),
        apply_dmg_palette(DmgPalettePreset::Gray, new_bgp, 1)
    );
}

#[test]
fn dmg_partial_line_state_restore_converges_at_the_next_scanline() {
    let mut ppu = PPU::new();
    let mut vram = [0u8; 0x4000];
    let oam = [0u8; 160];
    for row in 0..8 {
        vram[row * 2] = 0xFF;
    }

    ppu.lcd_was_enabled = true;
    ppu.blank_first_frame_after_lcd_on = false;
    ppu.ly = 1;
    ppu.bgp = 0xE4;
    ppu.step(
        OAM_DOTS + (DRAW_DOTS_BASE - SCREEN_W as u64) + 8 + 3,
        &vram,
        &oam,
        false,
    );
    ppu.write_bgp(0xE8, &vram, &oam);

    let mut writer = crate::save_state::StateWriter::new();
    ppu.write_state(&mut writer);
    let bytes = writer.into_bytes();
    let mut reader = crate::save_state::StateReader::new(&bytes);
    let mut restored =
        PPU::read_state(&mut reader, crate::save_state::SAVE_STATE_FORMAT_VERSION).unwrap();
    assert!(reader.is_exhausted());

    let remaining = DOTS_PER_LINE - ppu.cycles;
    ppu.step(remaining, &vram, &oam, false);
    restored.step(remaining, &vram, &oam, false);

    let mut expected = crate::save_state::StateWriter::new();
    ppu.write_state(&mut expected);
    let mut actual = crate::save_state::StateWriter::new();
    restored.write_state(&mut actual);
    assert_eq!(actual.into_bytes(), expected.into_bytes());
}

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

    ppu.stat &= !0x03;
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

    ppu.lcdc = Lcdc::LCD_ENABLE | Lcdc::WINDOW_ENABLE;
    ppu.lcd_was_enabled = true;
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

    ppu.lcdc = Lcdc::LCD_ENABLE | Lcdc::WINDOW_ENABLE;
    ppu.lcd_was_enabled = true;
    ppu.wy = 0;
    ppu.wx = 7;

    ppu.step(DOTS_PER_LINE, &vram, &oam, false);
    ppu.step(DOTS_PER_LINE, &vram, &oam, false);
    assert_eq!(ppu.window_line_counter, 2);

    ppu.lcdc &= !Lcdc::WINDOW_ENABLE;
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

    ppu.lcdc = Lcdc::LCD_ENABLE | Lcdc::WINDOW_ENABLE;
    ppu.wy = 0;
    ppu.wx = 167;

    for _ in 0..8 {
        ppu.step(DOTS_PER_LINE, &vram, &oam, false);
    }

    assert_eq!(ppu.window_line_counter, 0);
    assert!(!ppu.window_was_active_this_frame);
}

#[test]
fn mode_sequence_during_active_scanline() {
    let mut ppu = PPU::new();
    let vram = [0u8; 0x4000];
    let oam = [0u8; 160];

    ppu.lcdc = Lcdc::LCD_ENABLE;
    ppu.lcd_was_enabled = true;
    ppu.ly = 0;
    ppu.cycles = 0;

    ppu.step(OAM_DOTS - 1, &vram, &oam, false);
    assert_eq!(
        ppu.mode(),
        2,
        "should still be OAM scan at dot {}",
        OAM_DOTS - 1
    );

    ppu.step(1, &vram, &oam, false);
    assert_eq!(
        ppu.mode(),
        3,
        "should enter pixel transfer at dot {}",
        OAM_DOTS
    );

    ppu.step(DRAW_DOTS_BASE - 1, &vram, &oam, false);
    assert_eq!(ppu.mode(), 3, "should still be pixel transfer");

    ppu.step(1, &vram, &oam, false);
    assert_eq!(
        ppu.mode(),
        0,
        "should enter HBlank at dot {}",
        OAM_DOTS + DRAW_DOTS_BASE
    );
}

#[test]
fn stat_mode0_interrupt_is_delayed_after_visible_hblank() {
    let mut ppu = PPU::new();
    let vram = [0u8; 0x4000];
    let oam = [0u8; 160];

    ppu.lcdc = Lcdc::LCD_ENABLE;
    ppu.lcd_was_enabled = true;
    ppu.cycles = OAM_DOTS + DRAW_DOTS_BASE - 1;
    ppu.stat = (ppu.stat & !0x0B) | 0x0B;
    ppu.draw_dots_for_line = DRAW_DOTS_BASE;

    assert_eq!(
        ppu.step(1, &vram, &oam, false) & 0x02,
        0,
        "visible HBlank should not assert STAT mode0 IRQ immediately"
    );
    assert_eq!(ppu.mode(), 0);

    assert_eq!(
        ppu.step(STAT_IRQ_HBLANK_DELAY_DOTS, &vram, &oam, false) & 0x02,
        0x02,
        "STAT mode0 IRQ should assert after the HBlank delay"
    );
}

#[test]
fn stat_mode0_has_cpu_early_edge_before_if_visible() {
    let mut ppu = PPU::new();
    let vram = [0u8; 0x4000];
    let oam = [0u8; 160];

    ppu.lcdc = Lcdc::LCD_ENABLE;
    ppu.lcd_was_enabled = true;
    ppu.cycles = STAT_IRQ_OAM_DOTS + DRAW_DOTS_BASE - 1;
    ppu.stat = (ppu.stat & !0x0B) | 0x0B;
    ppu.draw_dots_for_line = DRAW_DOTS_BASE;

    assert_eq!(ppu.step(1, &vram, &oam, false) & 0x02, 0);
    assert!(ppu.drain_cpu_stat_interrupt_pending_before_if());
    assert!(!ppu.drain_cpu_stat_interrupt_pending_before_if());
}

#[test]
fn vblank_interrupt_fires_at_line_144() {
    let mut ppu = PPU::new();
    let vram = [0u8; 0x4000];
    let oam = [0u8; 160];

    ppu.lcdc = Lcdc::LCD_ENABLE;

    for _ in 0..143 {
        let irq = ppu.step(DOTS_PER_LINE, &vram, &oam, false);
        assert_eq!(irq & 0x01, 0, "VBlank should not fire before line 144");
    }
    assert_eq!(ppu.ly, 143);

    let irq = ppu.step(DOTS_PER_LINE, &vram, &oam, false);
    assert_ne!(irq & 0x01, 0, "VBlank should fire at line 144");
    assert_eq!(ppu.ly, 144);
    assert_eq!(ppu.mode(), 1);
}

#[test]
fn ly_wraps_to_zero_after_line_153() {
    let mut ppu = PPU::new();
    let vram = [0u8; 0x4000];
    let oam = [0u8; 160];

    ppu.lcdc = Lcdc::LCD_ENABLE;

    for _ in 0..154 {
        ppu.step(DOTS_PER_LINE, &vram, &oam, false);
    }
    assert_eq!(ppu.ly, 0);
}

#[test]
fn lcd_disabled_clears_mode_and_ly() {
    let mut ppu = PPU::new();
    let vram = [0u8; 0x4000];
    let oam = [0u8; 160];

    ppu.lcdc = Lcdc::LCD_ENABLE;
    for _ in 0..50 {
        ppu.step(DOTS_PER_LINE, &vram, &oam, false);
    }
    assert!(ppu.ly > 0);
    ppu.lcdc = Lcdc::empty();
    ppu.step(4, &vram, &oam, false);
    assert_eq!(ppu.ly, 0);
    assert_eq!(ppu.mode(), 0);
    assert_eq!(ppu.cycles, 0);
}

#[test]
fn vram_accessible_outside_mode3() {
    let _ppu = PPU::new();

    let mut off_ppu = PPU::new();
    off_ppu.lcdc = Lcdc::empty();
    assert!(off_ppu.cpu_vram_accessible());
    assert!(off_ppu.cpu_oam_accessible());

    let mut draw_ppu = PPU::new();
    draw_ppu.stat = (draw_ppu.stat & !0x03) | 3;
    draw_ppu.cycles = STAT_IRQ_OAM_DOTS;
    assert!(!draw_ppu.cpu_vram_accessible());
    assert!(!draw_ppu.cpu_oam_accessible());
    let mut hblank_ppu = PPU::new();
    hblank_ppu.stat &= !0x03;
    hblank_ppu.cycles = 300;
    assert!(hblank_ppu.cpu_vram_accessible());
    assert!(hblank_ppu.cpu_oam_accessible());
}

#[test]
fn vram_cpu_access_blocks_before_visible_mode3_status() {
    let mut ppu = PPU::new();

    ppu.lcdc = Lcdc::LCD_ENABLE;
    ppu.lcd_was_enabled = true;
    ppu.ly = 10;
    ppu.cycles = STAT_IRQ_OAM_DOTS - 1;
    ppu.draw_dots_for_line = DRAW_DOTS_BASE;
    ppu.stat = (ppu.stat & !0x03) | 2;

    assert!(ppu.cpu_vram_accessible());

    ppu.cycles = STAT_IRQ_OAM_DOTS;

    assert_eq!(
        ppu.mode(),
        2,
        "STAT mode is still OAM search at the CPU access edge"
    );
    assert!(!ppu.cpu_vram_accessible());
}

#[test]
fn cgb_vram_cpu_access_leaves_dot80_readable() {
    let mut ppu = PPU::new();

    ppu.lcdc = Lcdc::LCD_ENABLE;
    ppu.lcd_was_enabled = true;
    ppu.cgb_mode = true;
    ppu.ly = 10;
    ppu.draw_dots_for_line = DRAW_DOTS_BASE;
    ppu.cycles = STAT_IRQ_OAM_DOTS;

    assert!(ppu.cpu_vram_accessible());

    ppu.cycles = STAT_IRQ_OAM_DOTS + 1;

    assert!(!ppu.cpu_vram_accessible());
}

#[test]
fn cgb_double_speed_lcd_on_line_uses_dot84_block_edge() {
    let mut ppu = PPU::new();

    ppu.lcdc = Lcdc::LCD_ENABLE;
    ppu.lcd_was_enabled = true;
    ppu.blank_first_frame_after_lcd_on = true;
    ppu.cgb_mode = true;
    ppu.cgb_double_speed = true;
    ppu.ly = 0;
    ppu.draw_dots_for_line = DRAW_DOTS_BASE;
    ppu.cycles = LCD_ON_INITIAL_MODE0_DOTS;

    assert!(!ppu.cpu_vram_accessible());
}

#[test]
fn cgb_double_speed_oam_write_blocks_lcd_on_dot82_edge() {
    let mut ppu = PPU::new();

    ppu.lcdc = Lcdc::LCD_ENABLE;
    ppu.lcd_was_enabled = true;
    ppu.blank_first_frame_after_lcd_on = true;
    ppu.cgb_mode = true;
    ppu.cgb_double_speed = true;
    ppu.ly = 0;
    ppu.draw_dots_for_line = DRAW_DOTS_BASE;

    ppu.cycles = LCD_ON_INITIAL_MODE0_DOTS - 6;
    assert!(ppu.cpu_oam_write_accessible());

    ppu.cycles = LCD_ON_INITIAL_MODE0_DOTS - 4;
    assert!(ppu.cpu_oam_read_accessible());
    assert!(!ppu.cpu_oam_write_accessible());
}

#[test]
fn cgb_double_speed_oam_read_allows_dot0_search_edge() {
    let mut ppu = PPU::new();

    ppu.lcdc = Lcdc::LCD_ENABLE;
    ppu.lcd_was_enabled = true;
    ppu.cgb_mode = true;
    ppu.cgb_double_speed = true;
    ppu.ly = 1;
    ppu.cycles = 0;
    ppu.draw_dots_for_line = DRAW_DOTS_BASE;

    assert!(ppu.cpu_oam_read_accessible());
    assert!(!ppu.cpu_oam_write_accessible());
}

#[test]
fn lyc_coincidence_sets_stat_flag() {
    let mut ppu = PPU::new();
    ppu.stat = (ppu.stat & !0x03) | 0x40;
    ppu.ly = 5;
    ppu.lyc = 5;

    ppu.update_stat_interrupt();
    assert_ne!(ppu.stat & 0x04, 0, "LYC coincidence flag should be set");

    ppu.ly = 6;
    ppu.update_stat_interrupt();
    assert_eq!(ppu.stat & 0x04, 0, "LYC coincidence flag should be cleared");
}

#[test]
fn draw_dots_increases_with_scx_fine_scroll() {
    let mut ppu = PPU::new();
    let vram = [0u8; 0x4000];
    let oam = [0u8; 160];

    ppu.lcdc = Lcdc::LCD_ENABLE;
    ppu.scx = 5;

    ppu.step(OAM_DOTS, &vram, &oam, false);

    assert_eq!(
        ppu.draw_dots_for_line,
        DRAW_DOTS_BASE + 5,
        "SCX fine scroll of 5 should add 5 penalty dots"
    );
}

#[test]
fn draw_dots_increases_with_sprites_on_line() {
    let mut ppu = PPU::new();
    let vram = [0u8; 0x4000];
    let mut oam = [0u8; 160];

    ppu.lcdc = Lcdc::LCD_ENABLE | Lcdc::OBJ_ENABLE;
    ppu.ly = 0;
    ppu.scx = 0;

    for i in 0..3 {
        oam[i * 4] = 16;
        oam[i * 4 + 1] = (10 + i * 20) as u8;
    }

    ppu.step(OAM_DOTS, &vram, &oam, false);

    assert_eq!(
        ppu.draw_dots_for_line,
        DRAW_DOTS_BASE + 24,
        "3 sprites should include per-fetch and bucket-stall penalty dots"
    );
}

#[test]
fn draw_dots_base_with_no_sprites_and_zero_scx() {
    let mut ppu = PPU::new();
    let vram = [0u8; 0x4000];
    let oam = [0u8; 160];

    ppu.lcdc = Lcdc::LCD_ENABLE;
    ppu.scx = 0;

    ppu.step(OAM_DOTS, &vram, &oam, false);

    assert_eq!(
        ppu.draw_dots_for_line, DRAW_DOTS_BASE,
        "No sprites and SCX=0 should give base draw dots"
    );
}

#[test]
fn draw_dots_caps_at_10_sprites() {
    let mut ppu = PPU::new();
    let vram = [0u8; 0x4000];
    let mut oam = [0u8; 160];

    ppu.lcdc = Lcdc::LCD_ENABLE | Lcdc::OBJ_ENABLE;
    ppu.scx = 0;

    for i in 0..15 {
        oam[i * 4] = 16;
        oam[i * 4 + 1] = (i * 10) as u8;
    }

    ppu.step(OAM_DOTS, &vram, &oam, false);

    assert_eq!(
        ppu.draw_dots_for_line,
        DRAW_DOTS_BASE + 84,
        "Sprite penalty should cap at 10 selected sprites and include bucket stalls"
    );
}

#[test]
fn mode2_selection_does_not_revisit_scanned_oam_entries() {
    let mut ppu = PPU::new();
    let vram = [0u8; 0x4000];
    let mut oam = [0u8; 160];

    ppu.lcdc = Lcdc::LCD_ENABLE | Lcdc::OBJ_ENABLE;
    ppu.lcd_was_enabled = true;
    ppu.ly = 0;
    oam[0] = 0;

    ppu.step(2, &vram, &oam, false);
    oam[0] = 16;
    ppu.step(OAM_DOTS - 2, &vram, &oam, false);

    assert_eq!(ppu.mode2_cursor, 40);
    assert_eq!(ppu.selected_obj_count, 0);
}

#[test]
fn mode2_selection_keeps_first_ten_y_matches_regardless_of_x() {
    let mut ppu = PPU::new();
    let vram = [0u8; 0x4000];
    let mut oam = [0u8; 160];

    ppu.lcdc = Lcdc::LCD_ENABLE | Lcdc::OBJ_ENABLE;
    ppu.lcd_was_enabled = true;
    ppu.ly = 0;
    for index in 0..11 {
        oam[index * 4] = 16;
        oam[index * 4 + 1] = if index == 0 { 0 } else { 200 };
    }

    ppu.step(OAM_DOTS, &vram, &oam, false);

    assert_eq!(ppu.selected_obj_count, 10);
    assert_eq!(ppu.selected_obj_indices, [0, 1, 2, 3, 4, 5, 6, 7, 8, 9]);
}

#[test]
fn mode3_selected_obj_fetch_stalls_output_through_each_fetch_phase() {
    let mut ppu = PPU::new();
    let vram = [0u8; 0x4000];
    let mut oam = [0u8; 160];
    ppu.lcdc = Lcdc::LCD_ENABLE | Lcdc::OBJ_ENABLE;
    ppu.lcd_was_enabled = true;
    ppu.ly = 1;
    oam[0] = 17;
    oam[1] = 1;

    ppu.step(OAM_DOTS + 12, &vram, &oam, false);
    assert_eq!(ppu.mode3_output_x, 0);
    assert_eq!(ppu.mode3_obj_fetch_phase, ObjFetchPhase::Idle);

    ppu.step(1, &vram, &oam, false);
    assert_eq!(ppu.mode3_obj_fetch_phase, ObjFetchPhase::Align);
    assert_eq!(ppu.mode3_obj_fetch_phase_dot, 1);
    ppu.step(3, &vram, &oam, false);
    assert_eq!(ppu.mode3_obj_fetch_phase, ObjFetchPhase::Tile);
    assert_eq!(ppu.mode3_obj_fetch_phase_dot, 0);
    ppu.step(2, &vram, &oam, false);
    assert_eq!(ppu.mode3_obj_fetch_phase, ObjFetchPhase::DataLow);
    ppu.step(2, &vram, &oam, false);
    assert_eq!(ppu.mode3_obj_fetch_phase, ObjFetchPhase::DataHigh);
    ppu.step(2, &vram, &oam, false);
    assert_eq!(ppu.mode3_obj_fetch_phase, ObjFetchPhase::Idle);
    assert_eq!(ppu.mode3_obj_fetched_mask, 1);
    assert_eq!(ppu.mode3_output_x, 0);
    ppu.step(1, &vram, &oam, false);
    assert_eq!(ppu.mode3_output_x, 1);
}

#[test]
fn mode3_obj_tile_row_samples_size_at_data_low_completion() {
    let mut ppu = PPU::new();
    let mut vram = [0u8; 0x4000];
    let mut oam = [0u8; 160];
    ppu.lcdc = Lcdc::LCD_ENABLE | Lcdc::OBJ_ENABLE | Lcdc::OBJ_SIZE;
    ppu.lcd_was_enabled = true;
    ppu.ly = 9;
    vram[0] = 0x11;
    vram[1] = 0x22;
    vram[16] = 0x33;
    vram[17] = 0x44;
    oam[0] = 17;
    oam[1] = 1;

    ppu.step(OAM_DOTS, &vram, &oam, false);
    for _ in 0..64 {
        if ppu.mode3_obj_fetch_phase == ObjFetchPhase::DataLow && ppu.mode3_obj_fetch_phase_dot == 1
        {
            break;
        }
        ppu.step(1, &vram, &oam, false);
    }
    assert_eq!(ppu.mode3_obj_fetch_phase, ObjFetchPhase::DataLow);
    assert_eq!(ppu.mode3_obj_fetch_phase_dot, 1);
    assert_eq!(ppu.mode3_obj_tile_row_latched_mask, 0);

    ppu.write_lcdc((ppu.lcdc - Lcdc::OBJ_SIZE).bits());
    assert!(!ppu.legacy_obj_fetch_for_line);
    ppu.step(1, &vram, &oam, false);
    assert_eq!(ppu.mode3_obj_fetch_phase, ObjFetchPhase::DataHigh);
    assert_eq!(ppu.mode3_obj_tile_row_latched_mask, 1);
    assert_eq!(ppu.mode3_obj_tile_rows[0], 0);

    ppu.write_lcdc((ppu.lcdc | Lcdc::OBJ_SIZE).bits());
    ppu.step(1, &vram, &oam, false);
    ppu.step(1, &vram, &oam, false);
    assert_eq!(ppu.mode3_obj_tile_rows[0], 0);
    assert_eq!(ppu.mode3_obj_completed_mask, 1);
}

#[test]
fn canceled_obj_fetch_does_not_render_latched_partial_data() {
    let mut ppu = PPU::new();
    let mut vram = [0u8; 0x4000];
    let mut oam = [0u8; 160];
    ppu.lcdc = Lcdc::LCD_ENABLE | Lcdc::OBJ_ENABLE;
    ppu.lcd_was_enabled = true;
    ppu.ly = 1;
    ppu.bgp = 0xE4;
    ppu.obp0 = 0xE4;
    ppu.dmg_palette_preset = DmgPalettePreset::Gray;
    vram[0] = 0xFF;
    oam[0] = 17;
    oam[1] = 1;

    ppu.step(OAM_DOTS, &vram, &oam, false);
    for _ in 0..64 {
        if ppu.mode3_obj_fetch_phase == ObjFetchPhase::DataHigh {
            break;
        }
        ppu.step(1, &vram, &oam, false);
    }
    assert_eq!(ppu.mode3_obj_fetch_phase, ObjFetchPhase::DataHigh);
    assert_eq!(ppu.mode3_obj_tile_row_latched_mask, 1);
    assert_eq!(ppu.mode3_obj_tile_rows[0], 0);
    assert_eq!(ppu.mode3_obj_completed_mask, 0);

    ppu.write_lcdc((ppu.lcdc - Lcdc::OBJ_ENABLE).bits());
    ppu.write_lcdc((ppu.lcdc | Lcdc::OBJ_ENABLE).bits());
    assert_eq!(ppu.mode3_obj_fetched_mask, 1);
    assert_eq!(ppu.mode3_obj_completed_mask, 0);
    ppu.step(DOTS_PER_LINE - ppu.cycles, &vram, &oam, false);

    let line_start = SCREEN_W * 4;
    assert_eq!(
        &ppu.framebuffer[line_start..line_start + 4],
        apply_dmg_palette(DmgPalettePreset::Gray, ppu.bgp, 0)
    );
}

#[test]
fn mode3_selected_obj_fetch_state_continues_exactly_after_restore() {
    let vram = [0u8; 0x4000];
    let mut oam = [0u8; 160];
    oam[0] = 17;
    oam[1] = 1;

    for checkpoint in [13, 16, 18, 20, 22, 23] {
        let mut ppu = PPU::new();
        ppu.lcdc = Lcdc::LCD_ENABLE | Lcdc::OBJ_ENABLE;
        ppu.lcd_was_enabled = true;
        ppu.ly = 1;
        ppu.step(OAM_DOTS + checkpoint, &vram, &oam, false);

        let mut writer = crate::save_state::StateWriter::new();
        ppu.write_state(&mut writer);
        let bytes = writer.into_bytes();
        let mut reader = crate::save_state::StateReader::new(&bytes);
        let mut restored =
            PPU::read_state(&mut reader, crate::save_state::SAVE_STATE_FORMAT_VERSION).unwrap();
        ppu.step(17, &vram, &oam, false);
        restored.step(17, &vram, &oam, false);
        let mut expected = crate::save_state::StateWriter::new();
        ppu.write_state(&mut expected);
        let mut actual = crate::save_state::StateWriter::new();
        restored.write_state(&mut actual);
        assert_eq!(actual.into_bytes(), expected.into_bytes());
    }
}

#[test]
fn mode3_window_position_writes_fallback_and_restore_exactly() {
    let vram = [0u8; 0x4000];
    let mut oam = [0u8; 160];
    oam[0] = 17;
    oam[1] = 1;

    for (initial_wx, write_wx) in [(167, Some(7)), (7, Some(167)), (167, None)] {
        let mut ppu = PPU::new();
        ppu.lcdc = Lcdc::LCD_ENABLE | Lcdc::OBJ_ENABLE | Lcdc::WINDOW_ENABLE;
        ppu.lcd_was_enabled = true;
        ppu.window_y_triggered = true;
        ppu.ly = 1;
        ppu.wx = initial_wx;
        ppu.step(OAM_DOTS + 20, &vram, &oam, false);

        if let Some(wx) = write_wx {
            ppu.write_wx(wx);
        } else {
            ppu.write_wy(2);
        }
        assert!(ppu.legacy_obj_fetch_for_line);

        let mut writer = crate::save_state::StateWriter::new();
        ppu.write_state(&mut writer);
        let bytes = writer.into_bytes();
        let mut reader = crate::save_state::StateReader::new(&bytes);
        let mut restored =
            PPU::read_state(&mut reader, crate::save_state::SAVE_STATE_FORMAT_VERSION).unwrap();
        assert!(reader.is_exhausted());

        ppu.step(17, &vram, &oam, false);
        restored.step(17, &vram, &oam, false);
        let mut expected = crate::save_state::StateWriter::new();
        ppu.write_state(&mut expected);
        let mut actual = crate::save_state::StateWriter::new();
        restored.write_state(&mut actual);
        assert_eq!(actual.into_bytes(), expected.into_bytes());
    }

    for write_window_position in [true, false] {
        let mut ppu = PPU::new();
        ppu.lcdc = Lcdc::LCD_ENABLE | Lcdc::OBJ_ENABLE | Lcdc::WINDOW_ENABLE;
        ppu.lcd_was_enabled = true;
        ppu.blank_first_frame_after_lcd_on = true;
        ppu.window_y_triggered = true;
        ppu.wx = 167;
        oam[0] = 16;
        ppu.step(81, &vram, &oam, false);
        assert_eq!(ppu.mode(), 0);
        assert_eq!(ppu.mode3_obj_fetch_dot, 81);

        if write_window_position {
            ppu.write_wx(7);
            assert!(ppu.legacy_obj_fetch_for_line);
        } else {
            ppu.write_lcdc((ppu.lcdc - Lcdc::OBJ_ENABLE).bits());
            assert!(!ppu.legacy_obj_fetch_for_line);
        }
        let mut writer = crate::save_state::StateWriter::new();
        ppu.write_state(&mut writer);
        let bytes = writer.into_bytes();
        let mut reader = crate::save_state::StateReader::new(&bytes);
        let mut restored =
            PPU::read_state(&mut reader, crate::save_state::SAVE_STATE_FORMAT_VERSION).unwrap();
        ppu.step(17, &vram, &oam, false);
        restored.step(17, &vram, &oam, false);
        let mut expected = crate::save_state::StateWriter::new();
        ppu.write_state(&mut expected);
        let mut actual = crate::save_state::StateWriter::new();
        restored.write_state(&mut actual);
        assert_eq!(actual.into_bytes(), expected.into_bytes());
    }
}

#[test]
fn mode3_obj_disable_cancels_active_fetch_without_legacy_fallback() {
    let mut ppu = PPU::new();
    let vram = [0u8; 0x4000];
    let mut oam = [0u8; 160];
    ppu.lcdc = Lcdc::LCD_ENABLE | Lcdc::OBJ_ENABLE;
    ppu.lcd_was_enabled = true;
    ppu.ly = 1;
    oam[0] = 17;
    oam[1] = 8;

    ppu.step(OAM_DOTS, &vram, &oam, false);
    for _ in 0..64 {
        if ppu.mode3_obj_fetch_phase == ObjFetchPhase::Tile {
            break;
        }
        ppu.step(1, &vram, &oam, false);
    }
    assert_eq!(ppu.mode3_obj_fetch_phase, ObjFetchPhase::Tile);

    ppu.write_lcdc((ppu.lcdc - Lcdc::OBJ_ENABLE).bits());

    assert_eq!(ppu.mode3_obj_fetch_phase, ObjFetchPhase::Idle);
    assert_eq!(ppu.mode3_obj_fetch_phase_dot, 0);
    assert_eq!(ppu.mode3_obj_fetched_mask, 1);
    assert!(!ppu.legacy_obj_fetch_for_line);
}

#[test]
fn mode3_obj_enable_after_disabled_progress_fetches_only_future_objects() {
    let mut ppu = PPU::new();
    let vram = [0u8; 0x4000];
    let mut oam = [0u8; 160];
    ppu.lcdc = Lcdc::LCD_ENABLE;
    ppu.lcd_was_enabled = true;
    ppu.ly = 1;
    oam[0] = 17;
    oam[1] = 8;
    oam[4] = 17;
    oam[5] = 40;

    ppu.step(OAM_DOTS, &vram, &oam, false);
    for _ in 0..96 {
        if ppu.mode3_output_x == 16 {
            break;
        }
        ppu.step(1, &vram, &oam, false);
    }
    assert_eq!(ppu.mode3_obj_fetched_mask & 1, 1);
    assert_eq!(ppu.mode3_output_x, 16);

    ppu.write_lcdc((ppu.lcdc | Lcdc::OBJ_ENABLE).bits());
    for _ in 0..32 {
        if ppu.mode3_output_x == 32 {
            break;
        }
        ppu.step(1, &vram, &oam, false);
    }

    assert_eq!(ppu.mode3_output_x, 32);
    assert_eq!(ppu.mode3_obj_fetch_phase, ObjFetchPhase::Idle);
    ppu.step(1, &vram, &oam, false);
    assert_ne!(ppu.mode3_obj_fetch_phase, ObjFetchPhase::Idle);
    assert_eq!(ppu.mode3_obj_fetch_selection, 1);
}

#[test]
fn oam_dma_keeps_legacy_selection_for_the_remainder_of_the_line() {
    let mut ppu = PPU::new();
    let vram = [0u8; 0x4000];
    let mut oam = [0u8; 160];

    ppu.lcdc = Lcdc::LCD_ENABLE | Lcdc::OBJ_ENABLE;
    ppu.lcd_was_enabled = true;
    ppu.ly = 0;
    ppu.step_with_oam_dma(2, &vram, &oam, false, true);
    oam[0] = 16;
    ppu.step(OAM_DOTS - 2, &vram, &oam, false);

    assert!(ppu.legacy_sprite_selection_for_line);
    assert!(ppu.legacy_obj_fetch_for_line);
    assert_eq!(ppu.selected_obj_count, 0);
}

#[test]
fn oam_dma_keeps_legacy_selection_across_a_line_boundary() {
    let mut ppu = PPU::new();
    let vram = [0u8; 0x4000];
    let oam = [0u8; 160];
    ppu.lcdc = Lcdc::LCD_ENABLE | Lcdc::OBJ_ENABLE;
    ppu.lcd_was_enabled = true;
    ppu.ly = 4;
    ppu.cycles = DOTS_PER_LINE - 1;

    ppu.step_with_oam_dma(4, &vram, &oam, false, true);

    assert_eq!(ppu.ly, 5);
    assert!(ppu.legacy_sprite_selection_for_line);
    assert!(ppu.legacy_obj_fetch_for_line);
}

#[test]
fn version_ten_mid_mode3_state_uses_legacy_obj_fetch_for_one_line() {
    let mut ppu = PPU::new();
    let vram = [0u8; 0x4000];
    let mut oam = [0u8; 160];
    ppu.lcdc = Lcdc::LCD_ENABLE | Lcdc::OBJ_ENABLE;
    ppu.lcd_was_enabled = true;
    ppu.ly = 1;
    oam[0] = 17;
    oam[1] = 1;
    ppu.step(OAM_DOTS + 13, &vram, &oam, false);

    let mut writer = crate::save_state::StateWriter::new();
    ppu.write_state(&mut writer);
    let mut bytes = writer.into_bytes();
    bytes.truncate(bytes.len() - 29);
    let mut reader = crate::save_state::StateReader::new(&bytes);
    let restored = PPU::read_state(&mut reader, 10).unwrap();

    assert!(reader.is_exhausted());
    assert!(restored.legacy_obj_fetch_for_line);
}

#[test]
fn version_eleven_ppu_state_rejects_invalid_obj_fetch_phase() {
    let ppu = PPU::new();
    let mut writer = crate::save_state::StateWriter::new();
    ppu.write_state(&mut writer);
    let mut bytes = writer.into_bytes();
    bytes.truncate(bytes.len() - 14);
    let phase_offset = bytes.len() - 3;
    bytes[phase_offset] = 0xFF;
    let mut reader = crate::save_state::StateReader::new(&bytes);

    assert!(PPU::read_state(&mut reader, 11).is_err());
}

#[test]
fn version_eleven_mid_mode3_state_uses_legacy_obj_fetch_for_one_line() {
    let mut ppu = PPU::new();
    let vram = [0u8; 0x4000];
    let mut oam = [0u8; 160];
    ppu.lcdc = Lcdc::LCD_ENABLE | Lcdc::OBJ_ENABLE | Lcdc::OBJ_SIZE;
    ppu.lcd_was_enabled = true;
    ppu.ly = 1;
    oam[0] = 17;
    oam[1] = 1;
    ppu.step(OAM_DOTS + 18, &vram, &oam, false);

    let mut writer = crate::save_state::StateWriter::new();
    ppu.write_state(&mut writer);
    let mut bytes = writer.into_bytes();
    bytes.truncate(bytes.len() - 14);
    let mut reader = crate::save_state::StateReader::new(&bytes);
    let restored = PPU::read_state(&mut reader, 11).unwrap();

    assert!(reader.is_exhausted());
    assert!(restored.legacy_obj_fetch_for_line);
    assert_eq!(restored.mode3_obj_tile_row_latched_mask, 0);
    assert_eq!(restored.mode3_obj_tile_rows, [0; 10]);
    assert_eq!(restored.mode3_obj_completed_mask, 0);
}

#[test]
fn version_twelve_ppu_state_rejects_invalid_obj_tile_row_latches() {
    let ppu = PPU::new();
    let mut writer = crate::save_state::StateWriter::new();
    ppu.write_state(&mut writer);
    let bytes = writer.into_bytes();
    let mut invalid_mask = bytes.clone();
    let mask_offset = invalid_mask.len() - 14;
    invalid_mask[mask_offset] = 1;
    let mut reader = crate::save_state::StateReader::new(&invalid_mask);
    assert!(PPU::read_state(&mut reader, 12).is_err());

    let mut invalid_row = bytes;
    let row_offset = invalid_row.len() - 12;
    invalid_row[row_offset] = 1;
    let mut reader = crate::save_state::StateReader::new(&invalid_row);
    assert!(PPU::read_state(&mut reader, 12).is_err());

    let mut invalid_completed = invalid_row;
    invalid_completed[row_offset] = 0;
    let completed_offset = invalid_completed.len() - 2;
    invalid_completed[completed_offset] = 1;
    let mut reader = crate::save_state::StateReader::new(&invalid_completed);
    assert!(PPU::read_state(&mut reader, 12).is_err());
}

#[test]
fn version_eleven_ppu_state_rejects_unstarted_fetch_after_mode3_begins() {
    for cycles in [OAM_DOTS + 12, DOTS_PER_LINE - 1] {
        let mut ppu = PPU::new();
        ppu.lcdc = Lcdc::LCD_ENABLE | Lcdc::OBJ_ENABLE;
        ppu.lcd_was_enabled = true;
        ppu.ly = 1;
        ppu.cycles = cycles;
        ppu.mode2_cursor = 40;
        ppu.selected_obj_count = 1;
        let mut writer = crate::save_state::StateWriter::new();
        ppu.write_state(&mut writer);
        let mut bytes = writer.into_bytes();
        bytes.truncate(bytes.len() - 14);
        let mut reader = crate::save_state::StateReader::new(&bytes);

        assert!(PPU::read_state(&mut reader, 11).is_err());
    }
}

#[test]
fn version_nine_ppu_state_uses_legacy_selection_for_one_line() {
    let ppu = PPU::new();
    let mut writer = crate::save_state::StateWriter::new();
    ppu.write_state(&mut writer);
    let mut bytes = writer.into_bytes();
    bytes.truncate(bytes.len() - 42);
    let mut reader = crate::save_state::StateReader::new(&bytes);

    let restored = PPU::read_state(&mut reader, 9).unwrap();

    assert!(reader.is_exhausted());
    assert!(restored.legacy_sprite_selection_for_line);
}

#[test]
fn mode2_selection_state_continues_exactly_after_restore() {
    let mut ppu = PPU::new();
    let vram = [0u8; 0x4000];
    let mut oam = [0u8; 160];
    ppu.lcdc = Lcdc::LCD_ENABLE | Lcdc::OBJ_ENABLE;
    ppu.lcd_was_enabled = true;
    ppu.ly = 7;
    for index in 0..24 {
        oam[index * 4] = if index % 3 == 0 { 23 } else { 0 };
        oam[index * 4 + 1] = (index * 7) as u8;
    }
    ppu.step(37, &vram, &oam, false);

    let mut writer = crate::save_state::StateWriter::new();
    ppu.write_state(&mut writer);
    let bytes = writer.into_bytes();
    let mut reader = crate::save_state::StateReader::new(&bytes);
    let mut restored =
        PPU::read_state(&mut reader, crate::save_state::SAVE_STATE_FORMAT_VERSION).unwrap();

    for cycles in [1, 42, 211, DOTS_PER_LINE] {
        ppu.step(cycles, &vram, &oam, false);
        restored.step(cycles, &vram, &oam, false);
        let mut expected = crate::save_state::StateWriter::new();
        ppu.write_state(&mut expected);
        let mut actual = crate::save_state::StateWriter::new();
        restored.write_state(&mut actual);
        assert_eq!(actual.into_bytes(), expected.into_bytes());
        assert_eq!(restored.framebuffer, ppu.framebuffer);
    }
}

#[test]
fn version_ten_ppu_state_rejects_cursor_inconsistent_with_mode2_progress() {
    let ppu = PPU::new();
    let mut writer = crate::save_state::StateWriter::new();
    ppu.write_state(&mut writer);
    let mut bytes = writer.into_bytes();
    bytes.truncate(bytes.len() - 29);
    let cursor_offset = bytes.len() - 13;
    bytes[cursor_offset] = 1;
    let mut reader = crate::save_state::StateReader::new(&bytes);

    assert!(PPU::read_state(&mut reader, 10).is_err());
}

#[test]
fn version_ten_ppu_state_rejects_selected_indices_out_of_order() {
    let mut ppu = PPU::new();
    ppu.cycles = 8;
    ppu.mode2_cursor = 4;
    ppu.selected_obj_indices[..2].copy_from_slice(&[0, 2]);
    ppu.selected_obj_count = 2;
    let mut writer = crate::save_state::StateWriter::new();
    ppu.write_state(&mut writer);
    let mut bytes = writer.into_bytes();
    bytes.truncate(bytes.len() - 29);
    let selected_offset = bytes.len() - 12;
    bytes[selected_offset + 1] = 0;
    let mut reader = crate::save_state::StateReader::new(&bytes);

    assert!(PPU::read_state(&mut reader, 10).is_err());
}

#[test]
fn version_ten_ppu_state_rejects_an_unscanned_selected_index() {
    let mut ppu = PPU::new();
    ppu.cycles = 8;
    ppu.mode2_cursor = 4;
    ppu.selected_obj_indices[0] = 3;
    ppu.selected_obj_count = 1;
    let mut writer = crate::save_state::StateWriter::new();
    ppu.write_state(&mut writer);
    let mut bytes = writer.into_bytes();
    bytes.truncate(bytes.len() - 29);
    let selected_offset = bytes.len() - 12;
    bytes[selected_offset] = 4;
    let mut reader = crate::save_state::StateReader::new(&bytes);

    assert!(PPU::read_state(&mut reader, 10).is_err());
}

#[test]
fn version_nine_selection_fallback_converges_after_the_current_line() {
    let mut ppu = PPU::new();
    let vram = [0u8; 0x4000];
    let mut oam = [0u8; 160];
    ppu.lcdc = Lcdc::LCD_ENABLE | Lcdc::OBJ_ENABLE;
    ppu.lcd_was_enabled = true;
    ppu.ly = 3;
    ppu.cycles = 120;
    ppu.legacy_sprite_selection_for_line = true;
    for index in 0..10 {
        oam[index * 4] = 19;
        oam[index * 4 + 1] = (index * 12) as u8;
    }

    let mut writer = crate::save_state::StateWriter::new();
    ppu.write_state(&mut writer);
    let mut bytes = writer.into_bytes();
    bytes.truncate(bytes.len() - 42);
    let mut reader = crate::save_state::StateReader::new(&bytes);
    let mut restored = PPU::read_state(&mut reader, 9).unwrap();

    let remaining = DOTS_PER_LINE - ppu.cycles;
    ppu.step(remaining, &vram, &oam, false);
    restored.step(remaining, &vram, &oam, false);
    let mut expected = crate::save_state::StateWriter::new();
    ppu.write_state(&mut expected);
    let mut actual = crate::save_state::StateWriter::new();
    restored.write_state(&mut actual);
    assert_eq!(actual.into_bytes(), expected.into_bytes());
}

#[test]
fn sgb_pct_trn_populates_border_palettes() {
    let mut ppu = PPU::new();
    ppu.lcdc = Lcdc::LCD_ENABLE | Lcdc::TILE_DATA | Lcdc::BG_ENABLE;

    let mut vram = vec![0u8; 0x4000];

    for idx in 0..256usize {
        let ty = idx / 20;
        let tx = idx % 20;
        let map_addr = 0x1800 + ty * 32 + tx;
        vram[map_addr] = idx as u8;
    }

    let tile_128_base = 128 * 16;
    vram[tile_128_base] = 0xFF;
    vram[tile_128_base + 1] = 0x7F;
    vram[tile_128_base + 2] = 0x1F;
    vram[tile_128_base + 3] = 0x00;

    ppu.sgb_pct_trn(&vram, 0);

    assert_eq!(ppu.sgb_border_palettes[0][0], 0x7FFF);
    assert_eq!(ppu.sgb_border_palettes[0][1], 0x001F);
    assert_eq!(
        ppu.sgb_border_palettes[4][0], 0x7FFF,
        "palette should be mirrored to index 4"
    );
    assert_eq!(
        ppu.sgb_border_palettes[4][1], 0x001F,
        "palette should be mirrored to index 4"
    );
}

#[test]
fn sgb_attr_blk_sets_inside_border_outside_palettes() {
    let mut ppu = PPU::new();

    let data: Vec<u8> = vec![0x00, 0x01, 0x07, 0x39, 5, 5, 10, 10];

    ppu.sgb_apply_attr_blk(&data);
    assert_eq!(ppu.sgb_attr_map[0], 3, "outside should be palette 3");
    assert_eq!(
        ppu.sgb_attr_map[5 * SGB_ATTR_BLOCKS_W + 5],
        2,
        "border should be palette 2"
    );
    assert_eq!(
        ppu.sgb_attr_map[7 * SGB_ATTR_BLOCKS_W + 7],
        1,
        "inside should be palette 1"
    );
    assert_eq!(
        ppu.sgb_attr_map[10 * SGB_ATTR_BLOCKS_W + 10],
        2,
        "corner border should be palette 2"
    );
}

#[test]
fn sgb_attr_blk_border_inherits_inside_when_b_unset() {
    let mut ppu = PPU::new();

    let data: Vec<u8> = vec![0x00, 0x01, 0x01, 0x02, 3, 3, 8, 8];

    ppu.sgb_apply_attr_blk(&data);

    assert_eq!(
        ppu.sgb_attr_map[3 * SGB_ATTR_BLOCKS_W + 3],
        2,
        "border should inherit inside palette"
    );
    assert_eq!(
        ppu.sgb_attr_map[5 * SGB_ATTR_BLOCKS_W + 5],
        2,
        "inside should be palette 2"
    );
    assert_eq!(ppu.sgb_attr_map[0], 0, "outside should be unchanged");
}

#[test]
fn sgb_attr_lin_sets_horizontal_and_vertical_lines() {
    let mut ppu = PPU::new();

    let data: Vec<u8> = vec![0x00, 0x02, 0x23, 0xC5];

    ppu.sgb_apply_attr_lin(&data);

    assert_eq!(
        ppu.sgb_attr_map[3 * SGB_ATTR_BLOCKS_W],
        1,
        "row 3 should be palette 1"
    );
    assert_eq!(
        ppu.sgb_attr_map[3 * SGB_ATTR_BLOCKS_W + 10],
        1,
        "row 3, col 10 should be palette 1"
    );

    assert_eq!(ppu.sgb_attr_map[5], 2, "col 5 should be palette 2");
    assert_eq!(
        ppu.sgb_attr_map[3 * SGB_ATTR_BLOCKS_W + 5],
        2,
        "col 5, row 3 overridden to palette 2"
    );
    assert_eq!(
        ppu.sgb_attr_map[17 * SGB_ATTR_BLOCKS_W + 5],
        2,
        "col 5, last row should be palette 2"
    );
}

#[test]
fn sgb_attr_div_horizontal_split() {
    let mut ppu = PPU::new();
    let mut packet = [0u8; 16];
    packet[1] = (2 << 4) | (1 << 2);
    packet[2] = 9;

    ppu.sgb_apply_attr_div(&packet);

    assert_eq!(ppu.sgb_attr_map[0], 0, "above line should be palette 0");
    assert_eq!(
        ppu.sgb_attr_map[8 * SGB_ATTR_BLOCKS_W],
        0,
        "just above line should be palette 0"
    );
    assert_eq!(
        ppu.sgb_attr_map[9 * SGB_ATTR_BLOCKS_W],
        1,
        "on line should be palette 1"
    );
    assert_eq!(
        ppu.sgb_attr_map[10 * SGB_ATTR_BLOCKS_W],
        2,
        "below line should be palette 2"
    );
    assert_eq!(
        ppu.sgb_attr_map[17 * SGB_ATTR_BLOCKS_W],
        2,
        "last row should be palette 2"
    );
}

#[test]
fn sgb_attr_div_vertical_split() {
    let mut ppu = PPU::new();
    let mut packet = [0u8; 16];
    packet[1] = 0x40 | (1 << 4) | (2 << 2) | 3;
    packet[2] = 10;

    ppu.sgb_apply_attr_div(&packet);

    assert_eq!(ppu.sgb_attr_map[0], 3, "left of line should be palette 3");
    assert_eq!(ppu.sgb_attr_map[10], 2, "on line should be palette 2");
    assert_eq!(ppu.sgb_attr_map[15], 1, "right of line should be palette 1");
}

#[test]
fn sgb_attr_chr_horizontal_assignment() {
    let mut ppu = PPU::new();

    let data: Vec<u8> = vec![0x00, 2, 1, 8, 0, 0, 0b_00_01_10_11, 0b_11_10_01_00];

    ppu.sgb_apply_attr_chr(&data);

    let base = SGB_ATTR_BLOCKS_W + 2;
    assert_eq!(ppu.sgb_attr_map[base], 0);
    assert_eq!(ppu.sgb_attr_map[base + 1], 1);
    assert_eq!(ppu.sgb_attr_map[base + 2], 2);
    assert_eq!(ppu.sgb_attr_map[base + 3], 3);
    assert_eq!(ppu.sgb_attr_map[base + 4], 3);
    assert_eq!(ppu.sgb_attr_map[base + 5], 2);
    assert_eq!(ppu.sgb_attr_map[base + 6], 1);
    assert_eq!(ppu.sgb_attr_map[base + 7], 0);
}

#[test]
fn sgb_attr_set_applies_attribute_file_from_trn_data() {
    let mut ppu = PPU::new();
    for i in 0..90 {
        ppu.sgb_attr_trn_data[i] = 0xAA;
    }

    ppu.sgb_mask_mode = 1;
    ppu.sgb_attr_set(0, true);
    assert_eq!(ppu.sgb_attr_map[0], 2);
    assert_eq!(ppu.sgb_attr_map[100], 2);
    assert_eq!(ppu.sgb_attr_map[359], 2);
    assert_eq!(ppu.sgb_mask_mode, 0);
}

#[test]
fn sgb_attr_set_file_index_offset_is_correct() {
    let mut ppu = PPU::new();
    for i in 90..180 {
        ppu.sgb_attr_trn_data[i] = 0xFF;
    }

    ppu.sgb_attr_set(1, false);

    assert_eq!(ppu.sgb_attr_map[0], 3);
    assert_eq!(ppu.sgb_attr_map[359], 3);
}
