use super::*;

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

    ppu.stat = (ppu.stat & !0x03) | 0x00;
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
    assert!(!draw_ppu.cpu_vram_accessible());
    assert!(!draw_ppu.cpu_oam_accessible());
    let mut hblank_ppu = PPU::new();
    hblank_ppu.stat = (hblank_ppu.stat & !0x03) | 0;
    assert!(hblank_ppu.cpu_vram_accessible());
    assert!(hblank_ppu.cpu_oam_accessible());
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

    ppu.step(1, &vram, &oam, false);

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

    ppu.step(1, &vram, &oam, false);

    assert_eq!(
        ppu.draw_dots_for_line,
        DRAW_DOTS_BASE + 3 * 6,
        "3 sprites should add 18 penalty dots (3 * 6)"
    );
}

#[test]
fn draw_dots_base_with_no_sprites_and_zero_scx() {
    let mut ppu = PPU::new();
    let vram = [0u8; 0x4000];
    let oam = [0u8; 160];

    ppu.lcdc = Lcdc::LCD_ENABLE;
    ppu.scx = 0;

    ppu.step(1, &vram, &oam, false);

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

    ppu.step(1, &vram, &oam, false);

    assert_eq!(
        ppu.draw_dots_for_line,
        DRAW_DOTS_BASE + 10 * 6,
        "Sprite penalty should cap at 10 sprites (10 * 6 = 60)"
    );
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
    assert_eq!(ppu.sgb_border_palettes[4][0], 0x7FFF, "palette should be mirrored to index 4");
    assert_eq!(ppu.sgb_border_palettes[4][1], 0x001F, "palette should be mirrored to index 4");
}

#[test]
fn sgb_attr_blk_sets_inside_border_outside_palettes() {
    let mut ppu = PPU::new();

    let data: Vec<u8> = vec![
        0x00,
        0x01,
        0x07,
        0x39,
        5,
        5,
        10,
        10,
    ];

    ppu.sgb_apply_attr_blk(&data);
    assert_eq!(ppu.sgb_attr_map[0], 3, "outside should be palette 3");
    assert_eq!(ppu.sgb_attr_map[5 * SGB_ATTR_BLOCKS_W + 5], 2, "border should be palette 2");
    assert_eq!(ppu.sgb_attr_map[7 * SGB_ATTR_BLOCKS_W + 7], 1, "inside should be palette 1");
    assert_eq!(ppu.sgb_attr_map[10 * SGB_ATTR_BLOCKS_W + 10], 2, "corner border should be palette 2");
}

#[test]
fn sgb_attr_blk_border_inherits_inside_when_b_unset() {
    let mut ppu = PPU::new();

    let data: Vec<u8> = vec![
        0x00, 0x01,
        0x01,
        0x02,
        3, 3, 8, 8,
    ];

    ppu.sgb_apply_attr_blk(&data);

    assert_eq!(ppu.sgb_attr_map[3 * SGB_ATTR_BLOCKS_W + 3], 2, "border should inherit inside palette");
    assert_eq!(ppu.sgb_attr_map[5 * SGB_ATTR_BLOCKS_W + 5], 2, "inside should be palette 2");
    assert_eq!(ppu.sgb_attr_map[0], 0, "outside should be unchanged");
}

#[test]
fn sgb_attr_lin_sets_horizontal_and_vertical_lines() {
    let mut ppu = PPU::new();

    let data: Vec<u8> = vec![
        0x00,
        0x02,
        0x23,
        0xC5,
    ];

    ppu.sgb_apply_attr_lin(&data);

    assert_eq!(ppu.sgb_attr_map[3 * SGB_ATTR_BLOCKS_W], 1, "row 3 should be palette 1");
    assert_eq!(ppu.sgb_attr_map[3 * SGB_ATTR_BLOCKS_W + 10], 1, "row 3, col 10 should be palette 1");

    assert_eq!(ppu.sgb_attr_map[5], 2, "col 5 should be palette 2");
    assert_eq!(ppu.sgb_attr_map[3 * SGB_ATTR_BLOCKS_W + 5], 2, "col 5, row 3 overridden to palette 2");
    assert_eq!(ppu.sgb_attr_map[17 * SGB_ATTR_BLOCKS_W + 5], 2, "col 5, last row should be palette 2");
}

#[test]
fn sgb_attr_div_horizontal_split() {
    let mut ppu = PPU::new();
    let mut packet = [0u8; 16];
    packet[1] = (2 << 4) | (1 << 2);
    packet[2] = 9;

    ppu.sgb_apply_attr_div(&packet);

    assert_eq!(ppu.sgb_attr_map[0], 0, "above line should be palette 0");
    assert_eq!(ppu.sgb_attr_map[8 * SGB_ATTR_BLOCKS_W], 0, "just above line should be palette 0");
    assert_eq!(ppu.sgb_attr_map[9 * SGB_ATTR_BLOCKS_W], 1, "on line should be palette 1");
    assert_eq!(ppu.sgb_attr_map[10 * SGB_ATTR_BLOCKS_W], 2, "below line should be palette 2");
    assert_eq!(ppu.sgb_attr_map[17 * SGB_ATTR_BLOCKS_W], 2, "last row should be palette 2");
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

    let data: Vec<u8> = vec![
        0x00,
        2,
        1,
        8, 0,
        0,
        0b_00_01_10_11,
        0b_11_10_01_00,
    ];

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
