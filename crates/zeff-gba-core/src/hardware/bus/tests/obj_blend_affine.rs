use super::*;

fn hide_oam_from(bus: &mut Bus, first_hidden_obj: usize) {
    for obj in first_hidden_obj..128 {
        bus.write16(0x0700_0000 + (obj * 8) as u32, 1 << 9);
    }
}

#[test]
fn obj_render_draws_sprite_pixels() {
    let mut bus = Bus::new(cartridge(), 48_000);
    hide_oam_except_first(&mut bus);
    bus.write16(0x0400_0000, (1 << 6) | (1 << 12));
    bus.write16(0x0500_0202, 0x7C00);
    poke_vram8(&mut bus, 0x0601_0000, 0x11);
    bus.write16(0x0700_0000, 0);
    bus.write16(0x0700_0002, 0);
    bus.write16(0x0700_0004, 0);

    bus.render_frame();

    assert_eq!(&bus.ppu.framebuffer()[0..4], &[0x00, 0x00, 0xFF, 0xFF]);
}

#[test]
fn obj_y_128_to_159_renders_at_bottom_of_screen() {
    let mut bus = Bus::new(cartridge(), 48_000);
    hide_oam_except_first(&mut bus);
    bus.write16(0x0400_0000, (1 << 6) | (1 << 12));
    bus.write16(0x0500_0202, 0x7C00);
    fill_obj_tiles(&mut bus, 1, 0x11);
    bus.write16(0x0700_0002, 0);
    bus.write16(0x0700_0004, 0);

    bus.write16(0x0700_0000, 128);
    bus.render_frame();
    assert_eq!(framebuffer_pixel(&bus, 0, 128), &[0x00, 0x00, 0xFF, 0xFF]);

    bus.write16(0x0700_0000, 159);
    bus.render_frame();
    assert_eq!(framebuffer_pixel(&bus, 0, 159), &[0x00, 0x00, 0xFF, 0xFF]);
}

#[test]
fn scanline_obj_y_128_renders_at_bottom_of_screen() {
    let mut bus = Bus::new(cartridge(), 48_000);
    hide_oam_except_first(&mut bus);
    bus.write16(0x0400_0000, (1 << 6) | (1 << 12));
    bus.write16(0x0500_0202, 0x7C00);
    fill_obj_tiles(&mut bus, 1, 0x11);
    bus.write16(0x0700_0000, 128);
    bus.write16(0x0700_0002, 0);
    bus.write16(0x0700_0004, 0);

    bus.step_cycles(1232 * 128 + 1006);

    assert_eq!(framebuffer_pixel(&bus, 0, 128), &[0x00, 0x00, 0xFF, 0xFF]);
}

#[test]
fn obj_y_240_wraps_to_top_of_screen() {
    let mut bus = Bus::new(cartridge(), 48_000);
    hide_oam_except_first(&mut bus);
    bus.write16(0x0400_0000, (1 << 6) | (1 << 12));
    bus.write16(0x0500_0202, 0x7C00);
    fill_obj_tiles(&mut bus, 16, 0x11);
    bus.write16(0x0700_0000, 240);
    bus.write16(0x0700_0002, 2 << 14);
    bus.write16(0x0700_0004, 0);

    bus.render_frame();

    assert_eq!(framebuffer_pixel(&bus, 0, 0), &[0x00, 0x00, 0xFF, 0xFF]);
    assert_eq!(framebuffer_pixel(&bus, 0, 16), &[0x00, 0x00, 0x00, 0xFF]);
}

#[test]
fn obj_mosaic_repeats_upper_left_pixel() {
    let mut bus = Bus::new(cartridge(), 48_000);
    hide_oam_except_first(&mut bus);
    bus.write16(0x0400_0000, (1 << 6) | (1 << 12));
    bus.write16(0x0400_004C, 1 << 8);
    bus.write16(0x0500_0202, 0x001F);
    bus.write16(0x0500_0204, 0x03E0);
    poke_vram8(&mut bus, 0x0601_0000, 0x21);
    bus.write16(0x0700_0000, 1 << 12);
    bus.write16(0x0700_0002, 0);
    bus.write16(0x0700_0004, 0);

    bus.render_frame();

    assert_eq!(&bus.ppu.framebuffer()[0..4], &[0xFF, 0x00, 0x00, 0xFF]);
    assert_eq!(&bus.ppu.framebuffer()[4..8], &[0xFF, 0x00, 0x00, 0xFF]);
}

#[test]
fn obj_affine_identity_render_draws_sprite_pixels() {
    let mut bus = Bus::new(cartridge(), 48_000);
    hide_oam_except_first(&mut bus);
    bus.write16(0x0400_0000, (1 << 6) | (1 << 12));
    bus.write16(0x0500_0202, 0x7C00);
    poke_vram8(&mut bus, 0x0601_0000, 0x11);
    bus.write16(0x0700_0000, 1 << 8);
    bus.write16(0x0700_0002, 0);
    bus.write16(0x0700_0004, 0);
    bus.write16(0x0700_0006, 0x0100);
    bus.write16(0x0700_000E, 0);
    bus.write16(0x0700_0016, 0);
    bus.write16(0x0700_001E, 0x0100);

    bus.render_frame();

    assert_eq!(&bus.ppu.framebuffer()[0..4], &[0x00, 0x00, 0xFF, 0xFF]);
}

#[test]
fn obj_256_color_render_uses_obj_palette() {
    let mut bus = Bus::new(cartridge(), 48_000);
    hide_oam_except_first(&mut bus);
    bus.write16(0x0400_0000, (1 << 6) | (1 << 12));
    bus.write16(0x0500_0002, 0x001F);
    bus.write16(0x0500_0202, 0x7C00);
    poke_vram8(&mut bus, 0x0601_0000, 0x01);
    bus.write16(0x0700_0000, 1 << 13);
    bus.write16(0x0700_0002, 0);
    bus.write16(0x0700_0004, 0);

    bus.render_frame();

    assert_eq!(&bus.ppu.framebuffer()[0..4], &[0x00, 0x00, 0xFF, 0xFF]);
}

#[test]
fn obj_2d_256_color_uses_32_tile_row_stride_and_ignores_low_tile_bit() {
    let mut bus = Bus::new(cartridge(), 48_000);
    hide_oam_except_first(&mut bus);
    bus.write16(0x0400_0000, 1 << 12);
    bus.write16(0x0500_0202, 0x001F);
    bus.write16(0x0500_0204, 0x03E0);
    poke_vram8(&mut bus, 0x0601_0480, 1);
    poke_vram8(&mut bus, 0x0601_0880, 2);
    bus.write16(0x0700_0000, 1 << 13);
    bus.write16(0x0700_0002, 1 << 14);
    bus.write16(0x0700_0004, 5);

    bus.render_frame();

    let dst = (8 * 240) * 4;
    assert_eq!(
        &bus.ppu.framebuffer()[dst..dst + 4],
        &[0xFF, 0x00, 0x00, 0xFF]
    );
}

#[test]
fn obj_priority_sits_behind_higher_priority_bg() {
    let mut bus = Bus::new(cartridge(), 48_000);
    hide_oam_except_first(&mut bus);
    bus.write16(0x0400_0000, (1 << 6) | (1 << 8) | (1 << 12));
    bus.write16(0x0400_0008, 1 << 8);
    bus.write16(0x0500_0002, 0x001F);
    bus.write16(0x0500_0202, 0x7C00);
    poke_vram8(&mut bus, 0x0600_0000, 0x11);
    bus.write16(0x0600_0800, 0);
    poke_vram8(&mut bus, 0x0601_0000, 0x11);
    bus.write16(0x0700_0000, 0);
    bus.write16(0x0700_0002, 0);
    bus.write16(0x0700_0004, 3 << 10);

    bus.render_frame();

    assert_eq!(&bus.ppu.framebuffer()[0..4], &[0xFF, 0x00, 0x00, 0xFF]);
}

#[test]
fn obj_priority_draws_over_equal_priority_bg() {
    let mut bus = Bus::new(cartridge(), 48_000);
    hide_oam_except_first(&mut bus);
    bus.write16(0x0400_0000, (1 << 6) | (1 << 8) | (1 << 12));
    bus.write16(0x0400_0008, (1 << 8) | 3);
    bus.write16(0x0500_0002, 0x001F);
    bus.write16(0x0500_0202, 0x7C00);
    poke_vram8(&mut bus, 0x0600_0000, 0x11);
    bus.write16(0x0600_0800, 0);
    poke_vram8(&mut bus, 0x0601_0000, 0x11);
    bus.write16(0x0700_0000, 0);
    bus.write16(0x0700_0002, 0);
    bus.write16(0x0700_0004, 3 << 10);

    bus.render_frame();

    assert_eq!(&bus.ppu.framebuffer()[0..4], &[0x00, 0x00, 0xFF, 0xFF]);
}

#[test]
fn lower_oam_index_obj_draws_over_equal_priority_obj() {
    let mut bus = Bus::new(cartridge(), 48_000);
    hide_oam_from(&mut bus, 2);
    bus.write16(0x0400_0000, (1 << 6) | (1 << 12));
    bus.write16(0x0500_0202, 0x001F);
    bus.write16(0x0500_0204, 0x03E0);
    poke_vram8(&mut bus, 0x0601_0000, 0x11);
    poke_vram8(&mut bus, 0x0601_0020, 0x22);
    bus.write16(0x0700_0000, 0);
    bus.write16(0x0700_0002, 0);
    bus.write16(0x0700_0004, 0);
    bus.write16(0x0700_0008, 0);
    bus.write16(0x0700_000A, 0);
    bus.write16(0x0700_000C, 1);

    bus.render_frame();

    assert_eq!(&bus.ppu.framebuffer()[0..4], &[0xFF, 0x00, 0x00, 0xFF]);
}

#[test]
fn obj_priority_bits_draw_over_oam_index_order() {
    let mut bus = Bus::new(cartridge(), 48_000);
    hide_oam_from(&mut bus, 2);
    bus.write16(0x0400_0000, (1 << 6) | (1 << 12));
    bus.write16(0x0500_0202, 0x001F);
    bus.write16(0x0500_0204, 0x03E0);
    poke_vram8(&mut bus, 0x0601_0000, 0x11);
    poke_vram8(&mut bus, 0x0601_0020, 0x22);
    bus.write16(0x0700_0000, 0);
    bus.write16(0x0700_0002, 0);
    bus.write16(0x0700_0004, 1 << 10);
    bus.write16(0x0700_0008, 0);
    bus.write16(0x0700_000A, 0);
    bus.write16(0x0700_000C, 1);

    bus.render_frame();

    assert_eq!(&bus.ppu.framebuffer()[0..4], &[0x00, 0xFF, 0x00, 0xFF]);
}

#[test]
fn bg_alpha_blends_over_obj_when_obj_is_next_lower_target() {
    let mut bus = Bus::new(cartridge(), 48_000);
    hide_oam_except_first(&mut bus);
    bus.write16(0x0400_0000, (1 << 6) | (1 << 9) | (1 << 10) | (1 << 12));
    bus.write16(0x0400_000A, (1 << 8) | 1);
    bus.write16(0x0400_000C, (2 << 8) | 2);
    bus.write16(0x0400_0050, (1 << 1) | (1 << 12) | (1 << 6));
    bus.write16(0x0400_0052, 8 | (8 << 8));
    bus.write16(0x0500_0002, 0x001F);
    bus.write16(0x0500_0004, 0x03E0);
    bus.write16(0x0500_0202, 0x7C00);
    poke_vram8(&mut bus, 0x0600_0000, 0x11);
    poke_vram8(&mut bus, 0x0600_0020, 0x22);
    poke_vram8(&mut bus, 0x0601_0000, 0x11);
    bus.write16(0x0600_0800, 0);
    bus.write16(0x0600_1000, 1);
    bus.write16(0x0700_0000, 0);
    bus.write16(0x0700_0002, 0);
    bus.write16(0x0700_0004, 2 << 10);

    bus.render_frame();

    assert_eq!(&bus.ppu.framebuffer()[0..4], &[0x7B, 0x00, 0x7B, 0xFF]);
}

#[test]
fn scanline_bg_alpha_blends_over_obj_when_obj_is_next_lower_target() {
    let mut bus = Bus::new(cartridge(), 48_000);
    hide_oam_except_first(&mut bus);
    bus.write16(0x0400_0000, (1 << 6) | (1 << 9) | (1 << 10) | (1 << 12));
    bus.write16(0x0400_000A, (1 << 8) | 1);
    bus.write16(0x0400_000C, (2 << 8) | 2);
    bus.write16(0x0400_0050, (1 << 1) | (1 << 12) | (1 << 6));
    bus.write16(0x0400_0052, 8 | (8 << 8));
    bus.write16(0x0500_0002, 0x001F);
    bus.write16(0x0500_0004, 0x03E0);
    bus.write16(0x0500_0202, 0x7C00);
    poke_vram8(&mut bus, 0x0600_0000, 0x11);
    poke_vram8(&mut bus, 0x0600_0020, 0x22);
    poke_vram8(&mut bus, 0x0601_0000, 0x11);
    bus.write16(0x0600_0800, 0);
    bus.write16(0x0600_1000, 1);
    bus.write16(0x0700_0000, 0);
    bus.write16(0x0700_0002, 0);
    bus.write16(0x0700_0004, 2 << 10);

    bus.step_cycles(1006);

    assert_eq!(&bus.ppu.framebuffer()[0..4], &[0x7B, 0x00, 0x7B, 0xFF]);
}

#[test]
fn hblank_bg_priority_only_write_recomposes_current_scanline() {
    let mut bus = Bus::new(cartridge(), 48_000);
    hide_oam_except_first(&mut bus);
    bus.write16(0x0400_0000, (1 << 6) | (1 << 8) | (1 << 12));
    bus.write16(0x0400_0008, (1 << 8) | 3);
    bus.write16(0x0500_0002, 0x001F);
    bus.write16(0x0500_0202, 0x7C00);
    poke_vram8(&mut bus, 0x0600_0000, 0x11);
    bus.write16(0x0600_0800, 0);
    poke_vram8(&mut bus, 0x0601_0000, 0x11);
    bus.write16(0x0700_0000, 0);
    bus.write16(0x0700_0002, 0);
    bus.write16(0x0700_0004, 1 << 10);

    bus.step_cycles(1006);
    assert_eq!(&bus.ppu.framebuffer()[0..4], &[0x00, 0x00, 0xFF, 0xFF]);

    bus.write16(0x0400_0008, 1 << 8);

    assert_eq!(&bus.ppu.framebuffer()[0..4], &[0xFF, 0x00, 0x00, 0xFF]);
}

#[test]
fn early_line_bg_priority_raise_recomposes_previous_scanline() {
    let mut bus = Bus::new(cartridge(), 48_000);
    hide_oam_except_first(&mut bus);
    bus.write16(0x0400_0000, (1 << 6) | (1 << 8) | (1 << 12));
    bus.write16(0x0400_0008, (1 << 8) | 3);
    bus.write16(0x0500_0002, 0x001F);
    bus.write16(0x0500_0202, 0x7C00);
    poke_vram8(&mut bus, 0x0600_0000, 0x11);
    bus.write16(0x0600_0800, 0);
    poke_vram8(&mut bus, 0x0601_0000, 0x11);
    bus.write16(0x0700_0000, 0);
    bus.write16(0x0700_0002, 0);
    bus.write16(0x0700_0004, 1 << 10);

    bus.step_cycles(1232 + 20);
    assert_eq!(framebuffer_pixel(&bus, 0, 0), &[0x00, 0x00, 0xFF, 0xFF]);

    bus.write16(0x0400_0008, 1 << 8);

    assert_eq!(framebuffer_pixel(&bus, 0, 0), &[0xFF, 0x00, 0x00, 0xFF]);
}

#[test]
fn mode2_affine_bg_render_reads_tiles() {
    let mut bus = Bus::new(cartridge(), 48_000);
    bus.write16(0x0400_0000, 2 | (1 << 10));
    bus.write16(0x0400_000C, 1 << 8);
    bus.write16(0x0400_0020, 0x0100);
    bus.write16(0x0400_0026, 0x0100);
    bus.write16(0x0500_0002, 0x001F);
    poke_vram8(&mut bus, 0x0600_0000, 1);
    poke_vram8(&mut bus, 0x0600_0800, 0);

    bus.render_frame();

    assert_eq!(&bus.ppu.framebuffer()[0..4], &[0xFF, 0x00, 0x00, 0xFF]);
}

#[test]
fn mode2_affine_bg_uses_screen_y_for_pd_transform() {
    let mut bus = Bus::new(cartridge(), 48_000);
    bus.write16(0x0400_0000, 2 | (1 << 10));
    bus.write16(0x0400_000C, 1 << 8);
    bus.write16(0x0400_0020, 0x0100);
    bus.write16(0x0400_0026, 0x0100);
    bus.write16(0x0500_0002, 0x001F);
    bus.write16(0x0500_0004, 0x03E0);
    poke_vram8(&mut bus, 0x0600_0000, 1);
    poke_vram8(&mut bus, 0x0600_0008, 2);
    poke_vram8(&mut bus, 0x0600_0800, 0);

    bus.render_frame();

    assert_eq!(framebuffer_pixel(&bus, 0, 0), &[0xFF, 0x00, 0x00, 0xFF]);
    assert_eq!(framebuffer_pixel(&bus, 0, 1), &[0x00, 0xFF, 0x00, 0xFF]);
}

#[test]
fn mode1_affine_bg_respects_text_bg_priority() {
    let mut bus = Bus::new(cartridge(), 48_000);
    bus.write16(0x0400_0000, 1 | (1 << 8) | (1 << 10));
    bus.write16(0x0400_0008, 3 << 8);
    bus.write16(0x0400_000C, (2 << 8) | 3);
    bus.write16(0x0400_0020, 0x0100);
    bus.write16(0x0400_0026, 0x0100);
    bus.write16(0x0500_0002, 0x001F);
    bus.write16(0x0500_0004, 0x03E0);
    poke_vram8(&mut bus, 0x0600_0000, 0x11);
    poke_vram8(&mut bus, 0x0600_0040, 2);
    bus.write16(0x0600_1800, 0);
    poke_vram8(&mut bus, 0x0600_1000, 1);

    bus.render_frame();

    assert_eq!(&bus.ppu.framebuffer()[0..4], &[0xFF, 0x00, 0x00, 0xFF]);
}

#[test]
fn mode2_bg2_priority_draws_over_bg3() {
    let mut bus = Bus::new(cartridge(), 48_000);
    bus.write16(0x0400_0000, 2 | (1 << 10) | (1 << 11));
    bus.write16(0x0400_000C, 1 << 8);
    bus.write16(0x0400_000E, (2 << 8) | 3);
    bus.write16(0x0400_0020, 0x0100);
    bus.write16(0x0400_0026, 0x0100);
    bus.write16(0x0400_0030, 0x0100);
    bus.write16(0x0400_0036, 0x0100);
    bus.write16(0x0500_0002, 0x001F);
    bus.write16(0x0500_0004, 0x03E0);
    poke_vram8(&mut bus, 0x0600_0040, 1);
    poke_vram8(&mut bus, 0x0600_0080, 2);
    poke_vram8(&mut bus, 0x0600_0800, 1);
    poke_vram8(&mut bus, 0x0600_1000, 2);

    bus.render_frame();

    assert_eq!(&bus.ppu.framebuffer()[0..4], &[0xFF, 0x00, 0x00, 0xFF]);
}
