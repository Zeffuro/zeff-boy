use super::*;

fn fixture() -> VdpMemory {
    let mut result = memory();
    for index in 0..CRAM_WORDS {
        let color = ((index & 7) << 1) | ((index >> 3) << 5);
        word(&mut result, CRAM_WRITE_CODE, index as u32 * 2, color as u16);
    }
    result
}

fn reference(
    memory: &VdpMemory,
    registers: &VdpRegisterPipeline,
    sprites: &VdpSpriteLine,
    colors: &mut [u16],
    geometry: RasterGeometry,
) -> Result<(), VdpRenderError> {
    if let Some(context) =
        RasterContext::prepare_with_geometry(memory, registers, sprites, colors, geometry)
    {
        for (x, (color, pixel)) in colors.iter_mut().zip(sprites.pixels()).enumerate() {
            *color = memory.composite_color(
                registers,
                context.background(memory, x, geometry.source_y)?,
                (!pixel.is_transparent()).then_some(*pixel),
            )?;
        }
        Ok(())
    } else {
        for (x, color) in colors.iter_mut().enumerate() {
            *color = memory
                .raster_sprite_line_color_with_geometry(registers, sprites, x as u16, geometry)?;
        }
        Ok(())
    }
}

fn check(
    memory: &VdpMemory,
    registers: &VdpRegisterPipeline,
    sprites: &VdpSpriteLine,
    geometry: RasterGeometry,
    width: usize,
) -> Result<(), VdpRenderError> {
    let mut expected = vec![0xdead; width];
    let mut actual = expected.clone();
    let wanted = reference(memory, registers, sprites, &mut expected, geometry);
    let result =
        memory.render_raster_sprite_line_with_geometry(registers, sprites, &mut actual, geometry);
    assert_eq!(result, wanted, "geometry {geometry:?}");
    assert_eq!(actual, expected, "geometry {geometry:?}");
    result
}

fn empty_sprites(memory: &VdpMemory, registers: &VdpRegisterPipeline) -> VdpSpriteLine {
    memory
        .compose_sprite_line_with_registers(registers, &[])
        .unwrap()
}

fn names(memory: &mut VdpMemory, base: u16, count: usize, name: u16) {
    for index in 0..count {
        let address = base.wrapping_add((index * NAME_ENTRY_BYTES) as u16);
        word(memory, VRAM_WRITE_CODE, u32::from(address), name);
    }
}

fn pattern(memory: &mut VdpMemory, index: u16, height: usize, color: u8) {
    for row in 0..height {
        for byte in 0..TILE_ROW_BYTES {
            memory.write_dma_byte(
                (usize::from(index) * height * TILE_ROW_BYTES + row * TILE_ROW_BYTES + byte) as u16,
                color * 17,
            );
        }
    }
}

fn first_overlap() {
    let mut memory = fixture();
    word(&mut memory, VRAM_WRITE_CODE, 0xfc00, 17);
    word(&mut memory, VRAM_WRITE_CODE, 0xfc02, 1023);
    let mut raw = values(H40_CLOCK_MASK | SHADOW_HIGHLIGHT_ENABLE);
    raw[usize::from(SCROLL_MODE_REGISTER)] = Some(VSCROLL_COLUMN_MODE);
    raw[usize::from(WINDOW_X_REGISTER)] = Some(0x88);
    let registers = bank(raw);
    let line = sprites(&memory, HorizontalMode::H40, 127);
    check(
        &memory,
        &registers,
        &line,
        RasterGeometry::progressive(127),
        H40_SCREEN_WIDTH,
    )
    .unwrap();
    let geometry = RasterGeometry::progressive(127);
    let mut colors = vec![0; H40_SCREEN_WIDTH];
    memory
        .render_raster_with_horizontal(&registers, &line, &mut colors, geometry, Some([7, 1022]))
        .unwrap();
    for (x, color) in colors.into_iter().enumerate() {
        let background = memory
            .background_pixel_with_horizontal(&registers, x as u16, geometry, true, Some([7, 1022]))
            .unwrap();
        let pixel = line.pixels()[x];
        assert_eq!(
            color,
            memory
                .composite_color(
                    &registers,
                    background,
                    (!pixel.is_transparent()).then_some(pixel)
                )
                .unwrap()
        );
    }
}

fn fine_scroll_and_screen_columns() {
    let mut memory = fixture();
    for display in [0, H40_CLOCK_MASK] {
        for size in [0, 1, 3, 0x10, 0x11, 0x30] {
            for scroll in [0, 2, 3, 4, 6, 7] {
                let mut raw = values(display);
                raw[usize::from(PLANE_SIZE_REGISTER)] = Some(size);
                raw[usize::from(SCROLL_MODE_REGISTER)] = Some(scroll);
                raw[usize::from(WINDOW_X_REGISTER)] = Some(0);
                let registers = bank(raw);
                let line = empty_sprites(&memory, &registers);
                for a in 0..TILE_WIDTH {
                    for b in 0..TILE_WIDTH {
                        let address = 0xfc00 + 127 * SCROLL_PAIR_BYTES;
                        let address = match scroll & HSCROLL_MODE_MASK {
                            HSCROLL_FULL => 0xfc00,
                            HSCROLL_CELL => 0xfc00 + 120 * SCROLL_PAIR_BYTES,
                            _ => address,
                        };
                        word(
                            &mut memory,
                            VRAM_WRITE_CODE,
                            address as u32,
                            (1024 - a) as u16,
                        );
                        word(&mut memory, VRAM_WRITE_CODE, address as u32 + 2, b as u16);
                        check(
                            &memory,
                            &registers,
                            &line,
                            RasterGeometry::progressive(127),
                            line.pixels().len(),
                        )
                        .unwrap();
                    }
                }
            }
        }
    }
}

fn window_seams_and_active_geometry() {
    let memory = fixture();
    for display in [0, H40_CLOCK_MASK] {
        for position in 0..=WINDOW_POSITION_MASK {
            for direction in [0, WINDOW_DIRECTION] {
                for (physical_y, source_y, interlaced) in [
                    (0, 0, false),
                    (7, 15, true),
                    (8, 16, true),
                    (127, 255, true),
                    (128, 256, true),
                    (255, 511, true),
                ] {
                    let mut raw = values(display);
                    raw[usize::from(WINDOW_X_REGISTER)] = Some(position | direction);
                    raw[usize::from(WINDOW_Y_REGISTER)] = Some(0x81);
                    raw[usize::from(SCROLL_MODE_REGISTER)] =
                        Some(HSCROLL_LINE | VSCROLL_COLUMN_MODE);
                    let registers = bank(raw);
                    let line = empty_sprites(&memory, &registers);
                    check(
                        &memory,
                        &registers,
                        &line,
                        RasterGeometry {
                            physical_y,
                            source_y,
                            interlaced,
                        },
                        line.pixels().len(),
                    )
                    .unwrap();
                }
            }
        }
    }
}

fn attributes_rows_and_name_wrap() {
    let mut memory = fixture();
    for index in 0..VSRAM_WORDS {
        word(&mut memory, VSRAM_WRITE_CODE, index as u32 * 2, 0);
    }
    word(&mut memory, VRAM_WRITE_CODE, 0xfc00, 0);
    word(&mut memory, VRAM_WRITE_CODE, 0xfc02, 0);
    for interlaced in [false, true] {
        let height = tile_height(interlaced);
        for display in [0, H40_CLOCK_MASK | 6] {
            let mut raw = values(display);
            raw[usize::from(WINDOW_X_REGISTER)] = Some(0);
            let registers = bank(raw);
            let line = empty_sprites(&memory, &registers);
            for attributes in 0..32 {
                for index in [0, 1, 1023, 1024, 2047] {
                    let name = (attributes << 11) | index;
                    let mask = if interlaced {
                        INTERLACE_TILE_INDEX_MASK
                    } else {
                        TILE_INDEX_MASK
                    };
                    for row in 0..height {
                        for byte in 0..TILE_ROW_BYTES {
                            let first = ((row + byte * 2) & 15) as u8;
                            let second = (first + 1) & 15;
                            let address = usize::from(index & mask) * height * TILE_ROW_BYTES
                                + row * TILE_ROW_BYTES
                                + byte;
                            memory.write_dma_byte(address as u16, first << 4 | second);
                        }
                    }
                    names(&mut memory, registers.plane_a_base().unwrap(), 64, name);
                    names(&mut memory, registers.plane_b_base().unwrap(), 64, name);
                    for row in 0..height {
                        check(
                            &memory,
                            &registers,
                            &line,
                            RasterGeometry {
                                physical_y: if interlaced {
                                    row as u16 / 2
                                } else {
                                    row as u16
                                },
                                source_y: row as u16,
                                interlaced,
                            },
                            line.pixels().len(),
                        )
                        .unwrap();
                    }
                }
            }
        }
        let mut raw = values(0);
        raw[usize::from(PLANE_A_TABLE_REGISTER)] = Some(0x38);
        raw[usize::from(PLANE_SIZE_REGISTER)] = Some(3);
        raw[usize::from(WINDOW_X_REGISTER)] = Some(0);
        let registers = bank(raw);
        let line = empty_sprites(&memory, &registers);
        word(&mut memory, VRAM_WRITE_CODE, 0xfc00, 1);
        word(&mut memory, VRAM_WRITE_CODE, 0xfc02, 1);
        let last_y = 32 * height - 1;
        word(&mut memory, VSRAM_WRITE_CODE, 0, last_y as u16);
        word(&mut memory, VSRAM_WRITE_CODE, 2, last_y as u16);
        word(&mut memory, VRAM_WRITE_CODE, 65534, 1);
        check(
            &memory,
            &registers,
            &line,
            RasterGeometry {
                physical_y: 0,
                source_y: 0,
                interlaced,
            },
            line.pixels().len(),
        )
        .unwrap();
        word(&mut memory, VRAM_WRITE_CODE, 0xfc00, 0);
        word(&mut memory, VRAM_WRITE_CODE, 0xfc02, 0);
        word(&mut memory, VSRAM_WRITE_CODE, 0, 0);
        word(&mut memory, VSRAM_WRITE_CODE, 2, 0);
    }
}

fn composition_and_transparent_window() {
    let mut memory = fixture();
    for index in 0..VSRAM_WORDS {
        word(&mut memory, VSRAM_WRITE_CODE, index as u32 * 2, 0);
    }
    word(&mut memory, VRAM_WRITE_CODE, 0xfc00, 0);
    word(&mut memory, VRAM_WRITE_CODE, 0xfc02, 0);
    for shadow in [0, SHADOW_HIGHLIGHT_ENABLE] {
        let mut raw = values(shadow);
        raw[usize::from(WINDOW_X_REGISTER)] = Some(0);
        let registers = bank(raw);
        for palette in 0..4 {
            for priority in 0..4 {
                for a in [0, 1, 14, 15] {
                    for b in [0, 2, 14, 15] {
                        pattern(&mut memory, 1, 8, a);
                        pattern(&mut memory, 2, 8, b);
                        names(
                            &mut memory,
                            0xc000,
                            32,
                            1 | (palette << 13) | ((priority & 1) << 15),
                        );
                        names(
                            &mut memory,
                            0xe000,
                            32,
                            2 | (((palette + 1) & 3) << 13) | ((priority >> 1) << 15),
                        );
                        for (sprite_color, sprite_palette, sprite_priority) in [
                            (0, 0, false),
                            (1, 0, false),
                            (1, 0, true),
                            (14, 0, false),
                            (14, 3, false),
                            (14, 3, true),
                            (15, 3, false),
                            (15, 3, true),
                        ] {
                            pattern(&mut memory, 3, 8, sprite_color);
                            let sprite = VdpSprite {
                                x: 0,
                                y: 0,
                                width: 8,
                                height: 8,
                                link: 0,
                                name: 3
                                    | (sprite_palette << 13)
                                    | if sprite_priority { TILE_PRIORITY } else { 0 },
                            }
                            .pattern_row(0)
                            .unwrap();
                            let line = memory
                                .compose_raster_sprite_line(&[sprite], HorizontalMode::H32)
                                .unwrap();
                            check(
                                &memory,
                                &registers,
                                &line,
                                RasterGeometry::progressive(0),
                                H32_SCREEN_WIDTH,
                            )
                            .unwrap();
                        }
                    }
                }
            }
        }
        pattern(&mut memory, 0, 8, 0);
        pattern(&mut memory, 1, 8, 1);
        pattern(&mut memory, 2, 8, 2);
        names(&mut memory, 0xc000, 32, 1);
        names(&mut memory, 0xe000, 32, 2);
        let mut raw = values(shadow);
        raw[usize::from(WINDOW_X_REGISTER)] = Some(WINDOW_DIRECTION);
        let registers = bank(raw);
        names(&mut memory, registers.window_base().unwrap(), 32, 0);
        let line = empty_sprites(&memory, &registers);
        check(
            &memory,
            &registers,
            &line,
            RasterGeometry::progressive(0),
            H32_SCREEN_WIDTH,
        )
        .unwrap();
        let context =
            RasterContext::prepare(&memory, &registers, &line, &[0; H32_SCREEN_WIDTH], 0).unwrap();
        assert_eq!(context.background(&memory, 0, 0).unwrap().selected.color, 2);
    }
}

fn interval_reference(
    memory: &VdpMemory,
    registers: &VdpRegisterPipeline,
    sprites: &VdpSpriteLine,
    geometry: RasterGeometry,
    start: usize,
    colors: &mut [u16],
) {
    let full = vec![0; sprites.pixels().len()];
    let context =
        RasterContext::prepare_with_geometry(memory, registers, sprites, &full, geometry).unwrap();
    for (offset, color) in colors.iter_mut().enumerate() {
        let x = start + offset;
        let pixel = sprites.pixels()[x];
        *color = memory
            .composite_color(
                registers,
                context.background(memory, x, geometry.source_y).unwrap(),
                (!pixel.is_transparent()).then_some(pixel),
            )
            .unwrap();
    }
}

fn invocation_lifetimes() {
    for split in 1..8 {
        for mutation in 0..15 {
            let mut memory = fixture();
            for index in 0..VSRAM_WORDS {
                word(&mut memory, VSRAM_WRITE_CODE, index as u32 * 2, 0);
            }
            word(&mut memory, VRAM_WRITE_CODE, 0xfc00, 0);
            word(&mut memory, VRAM_WRITE_CODE, 0xfc02, 0);
            pattern(&mut memory, 0, 8, 0);
            for row in 0..8 {
                for byte in 0..TILE_ROW_BYTES {
                    let first = ((row + byte * 2) % 15 + 1) as u8;
                    let second = first % 15 + 1;
                    memory.write_dma_byte((32 + row * 4 + byte) as u16, first << 4 | second);
                }
            }
            names(&mut memory, 0xc000, 1024, 1);
            names(&mut memory, 0xe000, 1024, 0);
            names(&mut memory, 0xb000, 2048, 2);
            let mut raw = values(H40_CLOCK_MASK | SHADOW_HIGHLIGHT_ENABLE);
            let registers = bank(raw);
            let mut geometry = RasterGeometry::progressive(0);
            let line = empty_sprites(&memory, &registers);
            let mut actual = vec![0xdead; H40_SCREEN_WIDTH];
            let mut expected = actual.clone();
            let context =
                RasterContext::prepare_with_geometry(&memory, &registers, &line, &actual, geometry)
                    .unwrap();
            context
                .render_spans::<true>(&memory, &registers, &line, &mut actual[..split], 0)
                .unwrap();
            interval_reference(
                &memory,
                &registers,
                &line,
                geometry,
                0,
                &mut expected[..split],
            );
            match mutation {
                0 => word(&mut memory, VRAM_WRITE_CODE, 0xc000, 0xffff),
                1..=4 => memory.write_dma_byte(32 + mutation - 1, 0xef),
                5 => word(&mut memory, CRAM_WRITE_CODE, 2, 0xeee),
                6 => word(&mut memory, VRAM_WRITE_CODE, 0xfc00, 7),
                7 => word(&mut memory, VSRAM_WRITE_CODE, 0, 17),
                8 => raw[usize::from(WINDOW_X_REGISTER)] = Some(0x80),
                9 => raw[usize::from(WINDOW_TABLE_REGISTER)] = Some(0x20),
                10 => raw[usize::from(PLANE_A_TABLE_REGISTER)] = Some(0x28),
                11 => geometry = RasterGeometry::new(128, true, true),
                12 => {
                    let snapshot = VdpMemory::default().snapshot();
                    memory.restore(snapshot);
                }
                13 => {
                    let mut writer = StateWriter::new();
                    VdpMemory::default().write_state(&mut writer);
                    let bytes = writer.into_bytes();
                    memory.clear();
                    memory = VdpMemory::read_state(&mut StateReader::new(&bytes)).unwrap();
                }
                _ => memory.clear(),
            }
            let registers = bank(raw);
            let context =
                RasterContext::prepare_with_geometry(&memory, &registers, &line, &actual, geometry)
                    .unwrap();
            context
                .render_spans::<true>(&memory, &registers, &line, &mut actual[split..], split)
                .unwrap();
            interval_reference(
                &memory,
                &registers,
                &line,
                geometry,
                split,
                &mut expected[split..],
            );
            assert_eq!(actual, expected, "split {split}, mutation {mutation}");
        }
    }
}

fn publication_and_pending_registers() {
    let mut memory = fixture();
    let mut registers = bank(values(0));
    let mut fifo = VdpWriteFifo::default();
    for (code, address, data) in [
        (VRAM_WRITE_CODE, 0xc000, 0xe001),
        (VRAM_WRITE_CODE, 32, 0x1234),
        (CRAM_WRITE_CODE, 46, 0xeee),
        (VSRAM_WRITE_CODE, 0, 17),
    ] {
        let mut command = ControlCommand { address, code };
        fifo.push_data(&mut command, Some(2), data).unwrap();
        for _ in 0..3 {
            let before_memory = memory.clone();
            let before_fifo = fifo.clone();
            let line = sprites(&memory, HorizontalMode::H32, 0);
            check(
                &memory,
                &registers,
                &line,
                RasterGeometry::progressive(0),
                H32_SCREEN_WIDTH,
            )
            .unwrap();
            assert_eq!(memory, before_memory);
            assert_eq!(fifo, before_fifo);
            fifo.service_write_slot(&mut memory).unwrap();
        }
    }
    registers.advance_to(MasterTicks::new(1)).unwrap();
    let publication = registers.queue_register(WINDOW_X_REGISTER, 0x80).unwrap();
    let line = sprites(&memory, HorizontalMode::H32, 0);
    for at in [
        publication.publish_at.get() - 1,
        publication.publish_at.get(),
        publication.publish_at.get() + 1,
    ] {
        registers.advance_to(MasterTicks::new(at)).unwrap();
        let before = registers;
        check(
            &memory,
            &registers,
            &line,
            RasterGeometry::progressive(0),
            H32_SCREEN_WIDTH,
        )
        .unwrap();
        assert_eq!(registers, before);
    }
}

fn fallback_errors_and_written_prefix() {
    let memory = fixture();
    for display in [0, H40_CLOCK_MASK] {
        let complete = bank(values(display));
        let line = empty_sprites(&memory, &complete);
        for index in 0..REGISTER_COUNT {
            let mut raw = values(display);
            raw[usize::from(index)] = None;
            let _ = check(
                &memory,
                &bank(raw),
                &line,
                RasterGeometry::progressive(0),
                line.pixels().len(),
            );
        }
        for index in [
            DISPLAY_SIZE_REGISTER,
            SCROLL_MODE_REGISTER,
            PLANE_SIZE_REGISTER,
        ] {
            for value in 0..=u8::MAX {
                let mut raw = values(display);
                raw[usize::from(index)] = Some(value);
                let _ = check(
                    &memory,
                    &bank(raw),
                    &line,
                    RasterGeometry::progressive(255),
                    line.pixels().len(),
                );
            }
        }
        for width in [0, 1, line.pixels().len() - 1, line.pixels().len() + 1] {
            let _ = check(
                &memory,
                &complete,
                &line,
                RasterGeometry::progressive(0),
                width,
            );
        }
        for geometry in [
            RasterGeometry::progressive(256),
            RasterGeometry {
                physical_y: 255,
                source_y: 512,
                interlaced: true,
            },
        ] {
            assert!(check(&memory, &complete, &line, geometry, line.pixels().len()).is_err());
        }
        let other = bank(values(display ^ H40_CLOCK_MASK));
        let _ = check(
            &memory,
            &other,
            &line,
            RasterGeometry::progressive(0),
            horizontal_mode(&other).unwrap().width(),
        );
    }
}

pub(super) fn conformance() {
    first_overlap();
    fine_scroll_and_screen_columns();
    window_seams_and_active_geometry();
    attributes_rows_and_name_wrap();
    composition_and_transparent_window();
    invocation_lifetimes();
    publication_and_pending_registers();
    fallback_errors_and_written_prefix();
}

#[test]
fn two_stream_spans_match_original_scalar_conformance() {
    conformance();
}
mod layout;
