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

    ppu.lcdc = LCDC_LCD_ENABLE | LCDC_WINDOW_ENABLE;
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

    ppu.lcdc = LCDC_LCD_ENABLE | LCDC_WINDOW_ENABLE;
    ppu.lcd_was_enabled = true;
    ppu.wy = 0;
    ppu.wx = 7;

    ppu.step(DOTS_PER_LINE, &vram, &oam, false);
    ppu.step(DOTS_PER_LINE, &vram, &oam, false);
    assert_eq!(ppu.window_line_counter, 2);

    ppu.lcdc &= !LCDC_WINDOW_ENABLE;
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

    ppu.lcdc = LCDC_LCD_ENABLE | LCDC_WINDOW_ENABLE;
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

    ppu.lcdc = LCDC_LCD_ENABLE;
    ppu.lcd_was_enabled = true;
    ppu.ly = 0;
    ppu.cycles = 0;

    ppu.step(OAM_DOTS - 1, &vram, &oam, false);
    assert_eq!(ppu.mode(), 2, "should still be OAM scan at dot {}", OAM_DOTS - 1);

    ppu.step(1, &vram, &oam, false);
    assert_eq!(ppu.mode(), 3, "should enter pixel transfer at dot {}", OAM_DOTS);

    ppu.step(DRAW_DOTS_BASE - 1, &vram, &oam, false);
    assert_eq!(ppu.mode(), 3, "should still be pixel transfer");

    ppu.step(1, &vram, &oam, false);
    assert_eq!(ppu.mode(), 0, "should enter HBlank at dot {}", OAM_DOTS + DRAW_DOTS_BASE);
}

#[test]
fn vblank_interrupt_fires_at_line_144() {
    let mut ppu = PPU::new();
    let vram = [0u8; 0x4000];
    let oam = [0u8; 160];

    ppu.lcdc = LCDC_LCD_ENABLE;

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

    ppu.lcdc = LCDC_LCD_ENABLE;

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

    ppu.lcdc = LCDC_LCD_ENABLE;
    for _ in 0..50 {
        ppu.step(DOTS_PER_LINE, &vram, &oam, false);
    }
    assert!(ppu.ly > 0);

    // Disable LCD.
    ppu.lcdc = 0;
    ppu.step(4, &vram, &oam, false);
    assert_eq!(ppu.ly, 0);
    assert_eq!(ppu.mode(), 0);
    assert_eq!(ppu.cycles, 0);
}

#[test]
fn vram_accessible_outside_mode3() {
    let _ppu = PPU::new();

    let mut off_ppu = PPU::new();
    off_ppu.lcdc = 0;
    assert!(off_ppu.cpu_vram_accessible());
    assert!(off_ppu.cpu_oam_accessible());

    let mut draw_ppu = PPU::new();
    draw_ppu.stat = (draw_ppu.stat & !0x03) | 3;
    assert!(!draw_ppu.cpu_vram_accessible());
    assert!(!draw_ppu.cpu_oam_accessible());

    // Mode 0 (HBlank) → accessible.
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

    ppu.lcdc = LCDC_LCD_ENABLE;
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

    ppu.lcdc = LCDC_LCD_ENABLE | LCDC_OBJ_ENABLE;
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

    ppu.lcdc = LCDC_LCD_ENABLE;
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

    ppu.lcdc = LCDC_LCD_ENABLE | LCDC_OBJ_ENABLE;
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

