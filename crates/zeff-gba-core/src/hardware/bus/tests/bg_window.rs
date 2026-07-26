use super::*;

#[test]
fn mode0_text_bg_render_reads_tiles() {
    let mut bus = Bus::new(cartridge(), 48_000);
    bus.write16(0x0400_0000, 1 << 8);
    bus.write16(0x0400_0008, 1 << 8);
    bus.write16(0x0500_0002, 0x03E0);
    poke_vram8(&mut bus, 0x0600_0000, 0x11);
    bus.write16(0x0600_0800, 0);

    bus.render_frame();

    assert_eq!(&bus.ppu.framebuffer()[0..4], &[0x00, 0xFF, 0x00, 0xFF]);
}

#[test]
fn mode0_bg_mosaic_repeats_upper_left_pixel() {
    let mut bus = Bus::new(cartridge(), 48_000);
    bus.write16(0x0400_0000, 1 << 8);
    bus.write16(0x0400_0008, (1 << 6) | (1 << 8));
    bus.write16(0x0400_004C, 1);
    bus.write16(0x0500_0002, 0x001F);
    bus.write16(0x0500_0004, 0x03E0);
    poke_vram8(&mut bus, 0x0600_0000, 0x21);
    bus.write16(0x0600_0800, 0);

    bus.render_frame();

    assert_eq!(&bus.ppu.framebuffer()[0..4], &[0xFF, 0x00, 0x00, 0xFF]);
    assert_eq!(&bus.ppu.framebuffer()[4..8], &[0xFF, 0x00, 0x00, 0xFF]);
}

#[test]
fn window0_masks_bg_outside_rectangle() {
    let mut bus = Bus::new(cartridge(), 48_000);
    bus.write16(0x0400_0000, (1 << 8) | (1 << 13));
    bus.write16(0x0400_0008, 1 << 8);
    bus.write16(0x0400_0040, (1 << 8) | 2);
    bus.write16(0x0400_0044, 1);
    bus.write16(0x0400_0048, 1);
    bus.write16(0x0400_004A, 0);
    bus.write16(0x0500_0000, 0x03E0);
    bus.write16(0x0500_0002, 0x001F);
    poke_vram8(&mut bus, 0x0600_0000, 0x11);
    bus.write16(0x0600_0800, 0);

    bus.render_frame();

    assert_eq!(&bus.ppu.framebuffer()[0..4], &[0x00, 0xFF, 0x00, 0xFF]);
    assert_eq!(&bus.ppu.framebuffer()[4..8], &[0xFF, 0x00, 0x00, 0xFF]);
}

#[test]
fn debug_layer_toggle_can_hide_gba_backgrounds() {
    let mut bus = Bus::new(cartridge(), 48_000);
    bus.write16(0x0400_0000, 1 << 8);
    bus.write16(0x0400_0008, 1 << 8);
    bus.write16(0x0500_0000, 0x03E0);
    bus.write16(0x0500_0002, 0x001F);
    poke_vram8(&mut bus, 0x0600_0000, 0x11);
    bus.write16(0x0600_0800, 0);
    bus.set_ppu_debug_flags(false, true, true);

    bus.render_frame();

    assert_eq!(&bus.ppu.framebuffer()[0..4], &[0x00, 0xFF, 0x00, 0xFF]);
    assert!(!bus.ppu_debug_snapshot().debug_flags.bg);
}

#[test]
fn debug_layer_toggle_can_hide_individual_gba_background() {
    let mut bus = Bus::new(cartridge(), 48_000);
    bus.write16(0x0400_0000, (1 << 8) | (1 << 9));
    bus.write16(0x0400_0008, 1 << 8);
    bus.write16(0x0400_000A, (2 << 8) | 1);
    bus.write16(0x0500_0002, 0x001F);
    bus.write16(0x0500_0004, 0x03E0);
    poke_vram8(&mut bus, 0x0600_0000, 0x11);
    poke_vram8(&mut bus, 0x0600_0020, 0x22);
    bus.write16(0x0600_0800, 0);
    bus.write16(0x0600_1000, 1);
    bus.set_ppu_debug_bg_layers([false, true, true, true]);

    bus.render_frame();

    assert_eq!(&bus.ppu.framebuffer()[0..4], &[0x00, 0xFF, 0x00, 0xFF]);
    assert!(!bus.ppu_debug_snapshot().debug_flags.bg_layers[0]);
    assert!(bus.ppu_debug_snapshot().debug_flags.bg_layers[1]);
}

#[test]
fn debug_layer_toggle_can_hide_gba_objs() {
    let mut bus = Bus::new(cartridge(), 48_000);
    hide_oam_except_first(&mut bus);
    bus.write16(0x0400_0000, (1 << 6) | (1 << 12));
    bus.write16(0x0500_0000, 0x03E0);
    bus.write16(0x0500_0202, 0x001F);
    poke_vram8(&mut bus, 0x0601_0000, 0x11);
    bus.write16(0x0700_0000, 0);
    bus.write16(0x0700_0002, 0);
    bus.write16(0x0700_0004, 0);
    bus.set_ppu_debug_flags(true, true, false);

    bus.render_frame();

    assert_eq!(&bus.ppu.framebuffer()[0..4], &[0x00, 0xFF, 0x00, 0xFF]);
    assert!(!bus.ppu_debug_snapshot().debug_flags.sprites);
}

#[test]
fn debug_layer_toggle_can_disable_gba_windows() {
    let mut bus = Bus::new(cartridge(), 48_000);
    bus.write16(0x0400_0000, (1 << 8) | (1 << 13));
    bus.write16(0x0400_0008, 1 << 8);
    bus.write16(0x0400_0040, (1 << 8) | 2);
    bus.write16(0x0400_0044, 1);
    bus.write16(0x0400_0048, 1);
    bus.write16(0x0400_004A, 0);
    bus.write16(0x0500_0000, 0x03E0);
    bus.write16(0x0500_0002, 0x001F);
    poke_vram8(&mut bus, 0x0600_0000, 0x11);
    bus.write16(0x0600_0800, 0);
    bus.set_ppu_debug_flags(true, false, true);

    bus.render_frame();

    assert_eq!(&bus.ppu.framebuffer()[0..4], &[0xFF, 0x00, 0x00, 0xFF]);
    assert_eq!(&bus.ppu.framebuffer()[4..8], &[0xFF, 0x00, 0x00, 0xFF]);
    assert!(!bus.ppu_debug_snapshot().debug_flags.window);
}

#[test]
fn window0_effect_bit_masks_brightness_inside_rectangle() {
    let mut bus = Bus::new(cartridge(), 48_000);
    bus.write16(0x0400_0000, (1 << 8) | (1 << 13));
    bus.write16(0x0400_0008, 1 << 8);
    bus.write16(0x0400_0040, 1);
    bus.write16(0x0400_0044, 1);
    bus.write16(0x0400_0048, 1);
    bus.write16(0x0400_004A, 1 | (1 << 5));
    bus.write16(0x0400_0050, 1 | (2 << 6));
    bus.write16(0x0400_0054, 16);
    bus.write16(0x0500_0002, 0x001F);
    poke_vram8(&mut bus, 0x0600_0000, 0x11);
    bus.write16(0x0600_0800, 0);

    bus.render_frame();

    assert_eq!(&bus.ppu.framebuffer()[0..4], &[0xFF, 0x00, 0x00, 0xFF]);
    assert_eq!(&bus.ppu.framebuffer()[4..8], &[0xFF, 0xFF, 0xFF, 0xFF]);
}

#[test]
fn mode0_text_bg_priority_draws_lower_priority_numbers_on_top() {
    let mut bus = Bus::new(cartridge(), 48_000);
    bus.write16(0x0400_0000, (1 << 8) | (1 << 9));
    bus.write16(0x0400_0008, 1 << 8);
    bus.write16(0x0400_000A, (2 << 8) | 3);
    bus.write16(0x0500_0002, 0x001F);
    bus.write16(0x0500_0004, 0x03E0);
    poke_vram8(&mut bus, 0x0600_0000, 0x11);
    poke_vram8(&mut bus, 0x0600_0020, 0x22);
    bus.write16(0x0600_0800, 0);
    bus.write16(0x0600_1000, 1);

    bus.render_frame();

    assert_eq!(&bus.ppu.framebuffer()[0..4], &[0xFF, 0x00, 0x00, 0xFF]);
}

#[test]
fn mode0_alpha_blends_first_target_over_second_target() {
    let mut bus = Bus::new(cartridge(), 48_000);
    bus.write16(0x0400_0000, (1 << 8) | (1 << 9));
    bus.write16(0x0400_0008, 1 << 8);
    bus.write16(0x0400_000A, (2 << 8) | 1);
    bus.write16(0x0400_0050, 1 | (1 << 9) | (1 << 6));
    bus.write16(0x0400_0052, 8 | (8 << 8));
    bus.write16(0x0500_0002, 0x001F);
    bus.write16(0x0500_0004, 0x03E0);
    poke_vram8(&mut bus, 0x0600_0000, 0x11);
    poke_vram8(&mut bus, 0x0600_0020, 0x22);
    bus.write16(0x0600_0800, 0);
    bus.write16(0x0600_1000, 1);

    bus.render_frame();

    assert_eq!(&bus.ppu.framebuffer()[0..4], &[0x7B, 0x7B, 0x00, 0xFF]);
}

#[test]
fn mode0_text_bg_transparent_zero_ignores_palette_bank() {
    let mut bus = Bus::new(cartridge(), 48_000);
    bus.write16(0x0400_0000, (1 << 8) | (1 << 9));
    bus.write16(0x0400_0008, 1 << 8);
    bus.write16(0x0400_000A, (2 << 8) | 1);
    bus.write16(0x0500_0004, 0x03E0);
    bus.write16(0x0500_0020, 0x001F);
    poke_vram8(&mut bus, 0x0600_0020, 0x22);
    bus.write16(0x0600_0800, 1 << 12);
    bus.write16(0x0600_1000, 1);

    bus.render_frame();

    assert_eq!(&bus.ppu.framebuffer()[0..4], &[0x00, 0xFF, 0x00, 0xFF]);
}

#[test]
fn obj_window_masks_background_using_winout_obj_bits() {
    let mut bus = Bus::new(cartridge(), 48_000);
    hide_oam_except_first(&mut bus);
    bus.write16(0x0400_0000, (1 << 8) | (1 << 12) | (1 << 15));
    bus.write16(0x0400_0008, 1 << 8);
    bus.write16(0x0400_004A, 1 << 8);
    bus.write16(0x0500_0002, 0x001F);
    poke_vram8(&mut bus, 0x0600_0000, 1);
    bus.write16(0x0600_0800, 0);
    bus.write16(0x0700_0000, 2 << 10);
    bus.write16(0x0700_0002, 0);
    bus.write16(0x0700_0004, 0);
    for offset in 0..32 {
        poke_vram8(&mut bus, 0x0601_0000 + offset, 0x11);
    }

    bus.render_frame();

    assert_eq!(&bus.ppu.framebuffer()[0..4], &[0xFF, 0x00, 0x00, 0xFF]);
    assert_eq!(
        &bus.ppu.framebuffer()[8 * 4..9 * 4],
        &[0x00, 0x00, 0x00, 0xFF]
    );
}

#[test]
fn scanline_renderer_uses_line_obj_window_mask() {
    let mut bus = Bus::new(cartridge(), 48_000);
    hide_oam_except_first(&mut bus);
    bus.write16(0x0400_0000, (1 << 8) | (1 << 12) | (1 << 15));
    bus.write16(0x0400_0008, 1 << 8);
    bus.write16(0x0400_004A, 1 << 8);
    bus.write16(0x0500_0002, 0x001F);
    poke_vram8(&mut bus, 0x0600_0000, 1);
    bus.write16(0x0600_0800, 0);
    bus.write16(0x0700_0000, 2 << 10);
    bus.write16(0x0700_0002, 0);
    bus.write16(0x0700_0004, 0);
    for offset in 0..32 {
        poke_vram8(&mut bus, 0x0601_0000 + offset, 0x11);
    }

    bus.step_cycles(1006);

    assert_eq!(&bus.ppu.framebuffer()[0..4], &[0xFF, 0x00, 0x00, 0xFF]);
    assert_eq!(
        &bus.ppu.framebuffer()[8 * 4..9 * 4],
        &[0x00, 0x00, 0x00, 0xFF]
    );
}
