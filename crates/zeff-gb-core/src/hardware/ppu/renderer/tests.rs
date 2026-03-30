use super::cgb_sprite_hidden_by_bg;
use crate::hardware::ppu::Lcdc;

#[test]
fn cgb_bg_attr_priority_blocks_sprite_on_non_zero_bg() {
    assert!(cgb_sprite_hidden_by_bg(Lcdc::from_bits_truncate(0x91), false, 2, true));
}

#[test]
fn cgb_sprite_priority_flag_blocks_sprite_on_non_zero_bg() {
    assert!(cgb_sprite_hidden_by_bg(Lcdc::from_bits_truncate(0x91), true, 1, false));
}

#[test]
fn cgb_allows_sprite_when_bg_color_zero() {
    assert!(!cgb_sprite_hidden_by_bg(Lcdc::from_bits_truncate(0x91), true, 0, true));
}

#[test]
fn cgb_lcdc_bg_priority_disable_allows_sprite_over_bg() {
    assert!(!cgb_sprite_hidden_by_bg(Lcdc::from_bits_truncate(0x90), true, 3, true));
}

