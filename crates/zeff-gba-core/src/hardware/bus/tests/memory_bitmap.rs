use super::*;

#[test]
fn ewram_mirrors() {
    let mut bus = Bus::new(cartridge(), 48_000);
    bus.write8(0x0204_0000, 0x42);
    assert_eq!(bus.read8(0x0200_0000), 0x42);
}

#[test]
fn video_byte_writes_match_gba_bus_rules() {
    let mut bus = Bus::new(cartridge(), 48_000);

    bus.write8(0x0500_0001, 0x12);
    assert_eq!(bus.read16(0x0500_0000), 0x1212);

    bus.write8(0x0600_0001, 0x34);
    assert_eq!(bus.read16(0x0600_0000), 0x3434);

    bus.write16(0x0601_0000, 0xABCD);
    bus.write8(0x0601_0000, 0x56);
    assert_eq!(
        bus.read16(0x0601_0000),
        0xABCD,
        "OBJ VRAM byte writes are ignored in tile modes"
    );

    bus.write16(0x0700_0000, 0xCAFE);
    bus.write8(0x0700_0000, 0x78);
    assert_eq!(bus.read16(0x0700_0000), 0xCAFE);
}

#[test]
fn bitmap_bg_vram_byte_writes_extend_through_framebuffer_area() {
    let mut bus = Bus::new(cartridge(), 48_000);
    bus.write16(0x0400_0000, 3);

    bus.write8(0x0601_2001, 0x9A);
    assert_eq!(bus.read16(0x0601_2000), 0x9A9A);

    bus.write16(0x0601_4000, 0x1357);
    bus.write8(0x0601_4000, 0xBC);
    assert_eq!(bus.read16(0x0601_4000), 0x1357);
}

#[test]
fn rom_reads_from_cartridge() {
    let bus = Bus::new(cartridge(), 48_000);
    assert_eq!(bus.read8(0x0800_00B2), 0x96);
    let _ = RomHeader::parse(bus.cartridge.rom()).unwrap();
}

#[test]
fn mode3_render_reads_vram_pixels() {
    let mut bus = Bus::new(cartridge(), 48_000);
    bus.write16(0x0400_0000, 3);
    bus.write16(0x0600_0000, 0x001F);

    bus.render_frame();

    assert_eq!(&bus.ppu.framebuffer()[0..4], &[0xFF, 0x00, 0x00, 0xFF]);
}

#[test]
fn forced_blank_overrides_rendered_layers_with_white() {
    let mut bus = Bus::new(cartridge(), 48_000);
    bus.write16(0x0400_0000, 3 | (1 << 7));
    bus.write16(0x0600_0000, 0x001F);

    bus.render_frame();

    assert_eq!(&bus.ppu.framebuffer()[0..4], &[0xFF, 0xFF, 0xFF, 0xFF]);
}

#[test]
fn scanline_backdrop_matches_frame_renderer_for_all_effect_modes() {
    for first_target in [false, true] {
        for second_target in [false, true] {
            for blend_mode in 0..4 {
                let mut frame_bus = Bus::new(cartridge(), 48_000);
                let mut scanline_bus = Bus::new(cartridge(), 48_000);
                for bus in [&mut frame_bus, &mut scanline_bus] {
                    bus.write16(0x0500_0000, 0x421F);
                    bus.write16(
                        0x0400_0050,
                        (u16::from(first_target) << 5)
                            | (u16::from(second_target) << 13)
                            | (blend_mode << 6),
                    );
                    bus.write16(0x0400_0052, 0x1010);
                    bus.write16(0x0400_0054, 9);
                }

                frame_bus.render_frame();
                scanline_bus.step_cycles(1006);
                assert_eq!(
                    &scanline_bus.ppu.framebuffer()[..SCREEN_WIDTH * 4],
                    &frame_bus.ppu.framebuffer()[..SCREEN_WIDTH * 4],
                    "first target {first_target}, second target {second_target}, mode {blend_mode}",
                );
            }
        }
    }
}

#[test]
fn scanline_backdrop_keeps_per_pixel_window_effects() {
    let mut frame_bus = Bus::new(cartridge(), 48_000);
    let mut scanline_bus = Bus::new(cartridge(), 48_000);
    for bus in [&mut frame_bus, &mut scanline_bus] {
        bus.write16(0x0400_0000, 1 << 13);
        bus.write16(0x0400_0040, 2);
        bus.write16(0x0400_0044, 1);
        bus.write16(0x0400_0048, 0);
        bus.write16(0x0400_004A, 1 << 5);
        bus.write16(0x0400_0050, (1 << 5) | (2 << 6));
        bus.write16(0x0400_0054, 16);
        bus.write16(0x0500_0000, 0x001F);
    }

    frame_bus.render_frame();
    scanline_bus.step_cycles(1006);
    assert_eq!(
        &scanline_bus.ppu.framebuffer()[..SCREEN_WIDTH * 4],
        &frame_bus.ppu.framebuffer()[..SCREEN_WIDTH * 4],
    );
    assert_ne!(
        framebuffer_pixel(&scanline_bus, 0, 0),
        framebuffer_pixel(&scanline_bus, 2, 0),
    );
}

#[test]
fn mode3_brightness_increase_applies_to_selected_bg2_target() {
    let mut bus = Bus::new(cartridge(), 48_000);
    bus.write16(0x0400_0000, 3);
    bus.write16(0x0400_0050, (1 << 2) | (2 << 6));
    bus.write16(0x0400_0054, 16);
    bus.write16(0x0600_0000, 0x001F);

    bus.render_frame();

    assert_eq!(&bus.ppu.framebuffer()[0..4], &[0xFF, 0xFF, 0xFF, 0xFF]);
}

#[test]
fn mode3_brightness_decrease_applies_to_selected_bg2_target() {
    let mut bus = Bus::new(cartridge(), 48_000);
    bus.write16(0x0400_0000, 3);
    bus.write16(0x0400_0050, (1 << 2) | (3 << 6));
    bus.write16(0x0400_0054, 16);
    bus.write16(0x0600_0000, 0x7FFF);

    bus.render_frame();

    assert_eq!(&bus.ppu.framebuffer()[0..4], &[0x00, 0x00, 0x00, 0xFF]);
}

#[test]
fn mode3_obj_tiles_below_512_are_ignored() {
    let mut bus = Bus::new(cartridge(), 48_000);
    hide_oam_except_first(&mut bus);
    bus.write16(0x0400_0000, 3 | (1 << 12));
    bus.write16(0x0600_0000, 0x03E0);
    bus.write16(0x0500_0202, 0x001F);
    poke_vram8(&mut bus, 0x0601_0000, 0x11);
    bus.write16(0x0700_0000, 1 << 13);
    bus.write16(0x0700_0002, 0);
    bus.write16(0x0700_0004, 0);

    bus.render_frame();

    assert_eq!(&bus.ppu.framebuffer()[0..4], &[0x00, 0xFF, 0x00, 0xFF]);
}

#[test]
fn mode3_obj_tile_512_renders() {
    let mut bus = Bus::new(cartridge(), 48_000);
    hide_oam_except_first(&mut bus);
    bus.write16(0x0400_0000, 3 | (1 << 12));
    bus.write16(0x0400_000C, 3);
    bus.write16(0x0600_0000, 0x03E0);
    bus.write16(0x0500_0202, 0x001F);
    poke_vram8(&mut bus, 0x0601_4000, 0x11);
    bus.write16(0x0700_0000, 0);
    bus.write16(0x0700_0002, 0);
    bus.write16(0x0700_0004, 512);

    bus.render_frame();

    assert_eq!(&bus.ppu.framebuffer()[0..4], &[0xFF, 0x00, 0x00, 0xFF]);
}

#[test]
fn mode3_obj_priority_sits_behind_higher_priority_bitmap_bg() {
    let mut bus = Bus::new(cartridge(), 48_000);
    hide_oam_except_first(&mut bus);
    bus.write16(0x0400_0000, 3 | (1 << 12));
    bus.write16(0x0400_000C, 0);
    bus.write16(0x0600_0000, 0x03E0);
    bus.write16(0x0500_0202, 0x001F);
    poke_vram8(&mut bus, 0x0601_4000, 0x11);
    bus.write16(0x0700_0000, 0);
    bus.write16(0x0700_0002, 0);
    bus.write16(0x0700_0004, 512 | (3 << 10));

    bus.render_frame();

    assert_eq!(&bus.ppu.framebuffer()[0..4], &[0x00, 0xFF, 0x00, 0xFF]);
}
