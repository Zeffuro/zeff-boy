use super::{SpriteRenderContext, cgb_sprite_hidden_by_bg, render_sprites};
use crate::hardware::ppu::{DmgPalettePreset, Lcdc, SCREEN_H, SCREEN_W};

#[test]
fn cgb_bg_attr_priority_blocks_sprite_on_non_zero_bg() {
    assert!(cgb_sprite_hidden_by_bg(
        Lcdc::from_bits_truncate(0x91),
        false,
        2,
        true
    ));
}

#[test]
fn cgb_sprite_priority_flag_blocks_sprite_on_non_zero_bg() {
    assert!(cgb_sprite_hidden_by_bg(
        Lcdc::from_bits_truncate(0x91),
        true,
        1,
        false
    ));
}

#[test]
fn cgb_allows_sprite_when_bg_color_zero() {
    assert!(!cgb_sprite_hidden_by_bg(
        Lcdc::from_bits_truncate(0x91),
        true,
        0,
        true
    ));
}

#[test]
fn cgb_lcdc_bg_priority_disable_allows_sprite_over_bg() {
    assert!(!cgb_sprite_hidden_by_bg(
        Lcdc::from_bits_truncate(0x90),
        true,
        3,
        true
    ));
}

#[test]
fn cached_selection_with_changed_y_falls_back_without_underflow() {
    let mut vram = [0u8; 0x2000];
    vram[0] = 0xFF;
    let mut oam = [0u8; 160];
    oam[4] = 16;
    oam[5] = 8;
    let mut cached_framebuffer = vec![0; SCREEN_W * SCREEN_H * 4];
    let mut legacy_framebuffer = vec![0; SCREEN_W * SCREEN_H * 4];
    let render = |framebuffer: &mut [u8], selected_obj_indices| {
        render_sprites(SpriteRenderContext {
            cgb_mode: false,
            lcdc: Lcdc::OBJ_ENABLE,
            obp0: 0xE4,
            obp1: 0xE4,
            vram: &vram,
            oam: &oam,
            ly: 0,
            framebuffer,
            cgb_obj_palette_ram: None,
            bg_color_ids: None,
            cgb_bg_priority_flags: None,
            dmg_palette_preset: DmgPalettePreset::default(),
            selected_obj_indices,
        });
    };

    render(&mut cached_framebuffer, Some(([0; 10], 1)));
    render(&mut legacy_framebuffer, None);

    assert_eq!(cached_framebuffer, legacy_framebuffer);
    assert_ne!(&cached_framebuffer[..4], &[0; 4]);
}
