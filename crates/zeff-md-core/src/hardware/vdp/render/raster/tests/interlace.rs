use super::*;
use crate::hardware::vdp::{VdpSpriteCache, VdpWrite};

fn row(memory: &mut VdpMemory, address: u32, words: [u16; 2]) {
    word(memory, VRAM_WRITE_CODE, address, words[0]);
    word(memory, VRAM_WRITE_CODE, address + 2, words[1]);
}

#[test]
fn mode2_pattern_addresses_keep_name_low_bit_alias_high_bit_and_flip_sixteen_rows() {
    let mut memory = VdpMemory::default();
    for (name, scanline, address) in [
        (0, 0, 0),
        (0, 15, 60),
        (1, 0, 64),
        (1, 1, 68),
        (1023, 14, 65528),
        (1023, 15, 65532),
        (1024, 0, 0),
    ] {
        row(&mut memory, address, [0x1234, 0x5678]);
        assert_eq!(
            memory
                .tile_row_in_geometry(name, scanline, true)
                .unwrap()
                .colors,
            [1, 2, 3, 4, 5, 6, 7, 8]
        );
        assert_eq!(
            memory
                .tile_row_in_geometry(name | TILE_VFLIP, 15 - scanline, true)
                .unwrap()
                .colors,
            [1, 2, 3, 4, 5, 6, 7, 8]
        );
        assert_eq!(
            memory
                .tile_row_in_geometry(name | TILE_HFLIP, scanline, true)
                .unwrap()
                .colors,
            [8, 7, 6, 5, 4, 3, 2, 1]
        );
    }
    row(&mut memory, 32, [0x9abc, 0xdef1]);
    assert_eq!(
        memory.tile_row(1, 0).unwrap().colors,
        [9, 10, 11, 12, 13, 14, 15, 1]
    );
    assert_eq!(memory.tile_row(1, 8), Err(VdpRenderError::InvalidRow(8)));
    assert_eq!(
        memory.tile_row_in_geometry(1, 16, true),
        Err(VdpRenderError::InvalidRow(16))
    );
}

#[test]
fn mode2_scroll_preserves_odd_y_eleven_bits_and_physical_hscroll_line() {
    let mut memory = VdpMemory::default();
    let mut values = values(6);
    values[usize::from(PLANE_SIZE_REGISTER)] = Some(0x30);
    values[usize::from(SCROLL_MODE_REGISTER)] = Some(HSCROLL_LINE);
    values[usize::from(WINDOW_X_REGISTER)] = Some(0);
    let registers = bank(values);
    word(&mut memory, VRAM_WRITE_CODE, 0xfe00, 8);
    word(&mut memory, VSRAM_WRITE_CODE, 0, 1025);
    word(&mut memory, VRAM_WRITE_CODE, 0xd400, 1);
    row(&mut memory, 72, [0x3456, 0x789a]);
    let geometry = RasterGeometry::new(128, true, true);
    assert_eq!(
        memory
            .scroll_offsets_with_geometry(&registers, VdpPlane::A, geometry, 0)
            .unwrap(),
        (8, 1025)
    );
    assert_eq!(
        memory
            .scrolled_plane(&registers, VdpPlane::A, 8, geometry, true)
            .unwrap()
            .color,
        3
    );
    let mut cell_values = *registers.registers();
    cell_values[usize::from(SCROLL_MODE_REGISTER)] = Some(HSCROLL_CELL);
    let cell_registers = bank(cell_values);
    word(&mut memory, VRAM_WRITE_CODE, 0xfc20, 24);
    assert_eq!(
        memory
            .scroll_offsets_with_geometry(
                &cell_registers,
                VdpPlane::A,
                RasterGeometry::new(15, true, true),
                0
            )
            .unwrap()
            .0,
        24
    );
}

#[test]
fn mode2_window_split_uses_sixteen_row_cells() {
    for display in [6, H40_CLOCK_MASK | 6] {
        let mut memory = VdpMemory::default();
        let mut values = values(display);
        values[usize::from(WINDOW_X_REGISTER)] = Some(0);
        values[usize::from(WINDOW_Y_REGISTER)] = Some(0x81);
        let registers = bank(values);
        let (base, address) = if display == 6 {
            (0xb800, 0xb840)
        } else {
            (0xb000, 0xb080)
        };
        assert_eq!(registers.window_base(), Some(base));
        word(&mut memory, VRAM_WRITE_CODE, address, 2);
        row(&mut memory, 128, [0x5678, 0x9abc]);
        assert_eq!(
            memory
                .sample_window_with_geometry(&registers, 0, RasterGeometry::new(7, true, true))
                .unwrap(),
            None
        );
        assert_eq!(
            memory
                .sample_window_with_geometry(&registers, 0, RasterGeometry::new(8, true, false))
                .unwrap()
                .unwrap()
                .color,
            5
        );
    }
}

#[test]
fn mode2_raster_and_scalar_have_literal_alternating_field_pixels() {
    for display in [0, 6, H40_CLOCK_MASK, H40_CLOCK_MASK | 6] {
        let mut memory = VdpMemory::default();
        let mut values = values(display);
        values[usize::from(WINDOW_X_REGISTER)] = Some(0);
        let registers = bank(values);
        let mode = horizontal_mode(&registers).unwrap();
        for x in 0..mode.width() / 8 {
            word(&mut memory, VRAM_WRITE_CODE, 0xc000 + x as u32 * 2, 1);
        }
        row(&mut memory, 64, [0x1111, 0x1111]);
        row(&mut memory, 68, [0x2222, 0x2222]);
        word(&mut memory, CRAM_WRITE_CODE, 2, 0x00e);
        word(&mut memory, CRAM_WRITE_CODE, 4, 0x0e0);
        let sprites = memory
            .compose_sprite_line_with_registers(&registers, &[])
            .unwrap();
        for odd in [false, true] {
            // Raw R12 deliberately differs from the active geometry on half the cases.
            let geometry = RasterGeometry::new(0, true, odd);
            let expected = if odd { 0x0e0 } else { 0x00e };
            let mut actual = vec![0; mode.width()];
            memory
                .render_raster_sprite_line_with_geometry(
                    &registers,
                    &sprites,
                    &mut actual,
                    geometry,
                )
                .unwrap();
            assert_eq!(actual, vec![expected; mode.width()]);
            for x in 0..mode.width() {
                assert_eq!(
                    memory
                        .raster_sprite_line_color_with_geometry(
                            &registers, &sprites, x as u16, geometry
                        )
                        .unwrap(),
                    expected
                );
            }
        }
        row(&mut memory, 32, [0x3333, 0x3333]);
        word(&mut memory, CRAM_WRITE_CODE, 6, 0xe00);
        let mut progressive = vec![0; mode.width()];
        memory
            .render_raster_sprite_line_with_geometry(
                &registers,
                &sprites,
                &mut progressive,
                RasterGeometry::progressive(0),
            )
            .unwrap();
        assert_eq!(progressive, vec![0xe00; mode.width()]);
    }
}

#[test]
fn mode2_raster_matches_scalar_for_scroll_window_and_source_rows_above_255() {
    let memory = memory();
    for display in [
        6,
        H40_CLOCK_MASK | 6,
        H40_CLOCK_MASK | 6 | SHADOW_HIGHLIGHT_ENABLE,
    ] {
        for size in [0, 1, 0x10, 0x11, 0x30] {
            for scroll in [0, 2, 3, 4, 6, 7] {
                for (wx, wy) in [(0, 0), (8, 0), (0x88, 0x88), (0x9f, 0x9f)] {
                    let mut values = values(display);
                    values[usize::from(PLANE_SIZE_REGISTER)] = Some(size);
                    values[usize::from(SCROLL_MODE_REGISTER)] = Some(scroll);
                    values[usize::from(WINDOW_X_REGISTER)] = Some(wx);
                    values[usize::from(WINDOW_Y_REGISTER)] = Some(wy);
                    let registers = bank(values);
                    let mode = horizontal_mode(&registers).unwrap();
                    let sprites = memory
                        .compose_sprite_line_with_registers(&registers, &[])
                        .unwrap();
                    for physical_y in [0, 7, 8, 127, 128, 223, 239, 255] {
                        for odd in [false, true] {
                            let geometry = RasterGeometry::new(physical_y, true, odd);
                            let mut actual = vec![0; mode.width()];
                            assert!(
                                RasterContext::prepare_with_geometry(
                                    &memory, &registers, &sprites, &actual, geometry
                                )
                                .is_some()
                            );
                            memory
                                .render_raster_sprite_line_with_geometry(
                                    &registers,
                                    &sprites,
                                    &mut actual,
                                    geometry,
                                )
                                .unwrap();
                            for (x, actual) in actual.into_iter().enumerate() {
                                assert_eq!(
                                    actual,
                                    memory
                                        .raster_sprite_line_color_with_geometry(
                                            &registers, &sprites, x as u16, geometry
                                        )
                                        .unwrap()
                                );
                            }
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn mode2_sprite_odd_y_crosses_255_256_and_cache_keeps_tenth_bit_before_enable() {
    let mut memory = VdpMemory::default();
    let mut values = values(0);
    values[usize::from(SPRITE_TABLE_REGISTER)] = Some(0x70);
    let registers = bank(values);
    let mut cache = VdpSpriteCache::default();
    for raw_y in [511, 512, 513] {
        for (address, data) in [(0xe000, raw_y), (0xe002, 0), (0xe004, 1), (0xe006, 128)] {
            cache
                .complete_write(
                    &mut memory,
                    &registers,
                    VdpWrite {
                        command: ControlCommand {
                            address,
                            code: VRAM_WRITE_CODE,
                        },
                        data,
                    },
                )
                .unwrap();
        }
        assert_eq!(cache.entry_words(0).unwrap()[0], Some(raw_y));
        let top = raw_y - 256;
        let geometry = RasterGeometry {
            physical_y: top / 2,
            source_y: top,
            interlaced: true,
        };
        let direct = memory
            .sprite_entry_with_layout_and_geometry(&registers, 0, None, geometry)
            .unwrap();
        let cached = cache
            .raster_sprite_entry_with_geometry(&memory, &registers, 0, geometry)
            .unwrap();
        assert_eq!(direct, cached);
        assert_eq!((direct.x, direct.y, direct.height), (0, top as i16, 16));
        let selected = cache
            .select_complete_row_with_geometry(geometry, HorizontalMode::H32)
            .unwrap();
        assert_eq!(selected.indices(), [0]);
        let pattern = selected
            .prepare_sprite(&cache, &memory, &registers, 0)
            .unwrap()
            .unwrap();
        assert_eq!(pattern.row_address(0), Some(64));
        assert_eq!(
            cache.sprite_entry(&memory, &registers, 0).unwrap().y,
            (raw_y & 511) as i16 - 128
        );
    }
}

#[test]
fn mode2_sprite_whole_flip_and_column_stride_select_literal_rows_once() {
    let mut memory = VdpMemory::default();
    row(&mut memory, 64, [0x1234, 0x5678]);
    row(&mut memory, 192, [0x2345, 0x6789]);
    row(&mut memory, 188, [0x3456, 0x789a]);
    row(&mut memory, 316, [0x4567, 0x89ab]);
    let sprite = VdpSprite {
        x: 0,
        y: 255,
        width: 16,
        height: 32,
        link: 0,
        name: 1,
    };
    let geometry = RasterGeometry {
        physical_y: 127,
        source_y: 255,
        interlaced: true,
    };
    let pattern = sprite.pattern_row_with_geometry(geometry).unwrap();
    assert_eq!(
        (pattern.row_address(0), pattern.row_address(1)),
        (Some(64), Some(192))
    );
    assert_eq!(
        pattern.tile_row(&memory, 0).unwrap().unwrap().colors,
        [1, 2, 3, 4, 5, 6, 7, 8]
    );
    let flipped = VdpSprite {
        name: 1 | TILE_VFLIP | TILE_HFLIP,
        ..sprite
    }
    .pattern_row_with_geometry(geometry)
    .unwrap();
    assert_eq!(
        (flipped.row_address(0), flipped.row_address(1)),
        (Some(316), Some(188))
    );
    assert_eq!(
        flipped.tile_row(&memory, 0).unwrap().unwrap().colors,
        [11, 10, 9, 8, 7, 6, 5, 4]
    );
}

#[test]
fn mode2_sprite_pixels_render_on_odd_y_in_both_horizontal_modes() {
    for display in [6, H40_CLOCK_MASK | 6] {
        let mut memory = VdpMemory::default();
        let mut values = values(display);
        values[usize::from(SPRITE_TABLE_REGISTER)] = Some(0x70);
        values[usize::from(WINDOW_X_REGISTER)] = Some(0);
        let registers = bank(values);
        for (address, data) in [(0xe000, 513), (0xe002, 0), (0xe004, 1), (0xe006, 128)] {
            word(&mut memory, VRAM_WRITE_CODE, address, data);
        }
        row(&mut memory, 64, [0x1234, 0x5678]);
        word(&mut memory, CRAM_WRITE_CODE, 2, 0x00e);
        word(&mut memory, CRAM_WRITE_CODE, 4, 0x0e0);
        let geometry = RasterGeometry::new(128, true, true);
        let cache = VdpSpriteCache::default();
        let sprite = cache
            .raster_sprite_entry_with_geometry(&memory, &registers, 0, geometry)
            .unwrap();
        assert!(
            sprite
                .pattern_row_with_geometry(RasterGeometry::new(128, true, false))
                .is_none()
        );
        let pattern = sprite.pattern_row_with_geometry(geometry).unwrap();
        let mode = horizontal_mode(&registers).unwrap();
        let sprites = memory.compose_raster_sprite_line(&[pattern], mode).unwrap();
        assert_eq!(sprites.pixels()[0].color, 1);
        assert_eq!(sprites.pixels()[1].color, 2);
        let mut actual = vec![0; mode.width()];
        memory
            .render_raster_sprite_line_with_geometry(&registers, &sprites, &mut actual, geometry)
            .unwrap();
        assert_eq!(&actual[..2], [0x00e, 0x0e0]);
        assert_eq!(actual[8], 0);
        for (x, actual) in actual.into_iter().enumerate() {
            assert_eq!(
                actual,
                memory
                    .raster_sprite_line_color_with_geometry(
                        &registers, &sprites, x as u16, geometry
                    )
                    .unwrap()
            );
        }
    }
}
