use super::*;

const SHAPES: [(u8, usize, usize, [usize; 10]); 16] = [
    (0x00, 32, 32, [0, 1, 31, 0, 31, 0, 31, 0, 31, 0]),
    (0x01, 64, 32, [0, 1, 31, 0, 31, 0, 31, 0, 31, 0]),
    (0x02, 32, 1, [0, 0, 0, 0, 0, 0, 0, 0, 0, 0]),
    (0x03, 128, 32, [0, 1, 31, 0, 31, 0, 31, 0, 31, 0]),
    (0x10, 32, 64, [0, 1, 31, 32, 63, 0, 31, 32, 63, 0]),
    (0x11, 64, 64, [0, 1, 31, 32, 63, 0, 31, 32, 63, 0]),
    (0x12, 32, 1, [0, 0, 0, 0, 0, 0, 0, 0, 0, 0]),
    (0x13, 128, 32, [0, 1, 31, 0, 31, 0, 31, 0, 31, 0]),
    (0x20, 32, 128, [0, 1, 31, 0, 31, 64, 95, 64, 95, 0]),
    (0x21, 64, 32, [0, 1, 31, 0, 31, 0, 31, 0, 31, 0]),
    (0x22, 32, 1, [0, 0, 0, 0, 0, 0, 0, 0, 0, 0]),
    (0x23, 128, 32, [0, 1, 31, 0, 31, 0, 31, 0, 31, 0]),
    (0x30, 32, 128, [0, 1, 31, 32, 63, 64, 95, 96, 127, 0]),
    (0x31, 64, 64, [0, 1, 31, 32, 63, 0, 31, 32, 63, 0]),
    (0x32, 32, 1, [0, 0, 0, 0, 0, 0, 0, 0, 0, 0]),
    (0x33, 128, 32, [0, 1, 31, 0, 31, 0, 31, 0, 31, 0]),
];
const SOURCE_ROWS: [usize; 10] = [0, 1, 31, 32, 63, 64, 95, 96, 127, 128];

fn name(cell: usize) -> u16 {
    (1 + cell % 31) as u16
        | (((cell / 31) % 4) as u16 * TILE_HFLIP)
        | (((cell / 124) % 4) as u16 * 0x2000)
        | if cell & 512 != 0 { TILE_PRIORITY } else { 0 }
}

fn expected(cell: usize, row: usize, column: usize, interlaced: bool) -> VdpPixel {
    let name = name(cell);
    let height = if interlaced { 16 } else { 8 };
    let row = if name & TILE_VFLIP != 0 {
        height - 1 - row
    } else {
        row
    };
    let column = if name & TILE_HFLIP != 0 {
        7 - column
    } else {
        column
    };
    VdpPixel {
        color: ((usize::from(name & 0x7ff) + row * 3 + column * 5) % 16) as u8,
        palette: ((name >> 13) & 3) as u8,
        priority: name & TILE_PRIORITY != 0,
    }
}

fn addressing_memory(base: u16, interlaced: bool) -> VdpMemory {
    let mut memory = VdpMemory::default();
    let height = if interlaced { 16 } else { 8 };
    for tile in 1..=31 {
        for row in 0..height {
            for column in (0..8).step_by(2) {
                let a = (tile + row * 3 + column * 5) % 16;
                let b = (tile + row * 3 + (column + 1) * 5) % 16;
                memory.write_dma_byte(
                    (tile * height * 4 + row * 4 + column / 2) as u16,
                    (a * 16 + b) as u8,
                );
            }
        }
    }
    for cell in 0..4096 {
        word(
            &mut memory,
            VRAM_WRITE_CODE,
            u32::from(base) + cell as u32 * 2,
            name(cell),
        );
    }
    word(
        &mut memory,
        VRAM_WRITE_CODE,
        u32::from(base.wrapping_add(0x2000)),
        31,
    );
    memory
}

fn addressing_registers(size: u8, base: u16, display: u8) -> VdpRegisterPipeline {
    let mut raw = values(display);
    raw[usize::from(PLANE_SIZE_REGISTER)] = Some(size);
    raw[usize::from(PLANE_A_TABLE_REGISTER)] = Some((base >> 10) as u8);
    raw[usize::from(PLANE_B_TABLE_REGISTER)] = Some((base >> 13) as u8);
    raw[usize::from(HSCROLL_TABLE_REGISTER)] = Some(0x1e);
    raw[usize::from(WINDOW_X_REGISTER)] = Some(0);
    bank(raw)
}

#[test]
fn all_raw_plane_sizes_have_literal_address_witnesses_and_inspection_bounds() {
    for interlaced in [false, true] {
        let height = if interlaced { 16 } else { 8 };
        for base in [0x8000, 0xe000] {
            let memory = addressing_memory(base, interlaced);
            let saved_memory = memory.clone();
            for display in [0, H40_CLOCK_MASK] {
                for (size, columns, rows, addressed_rows) in SHAPES {
                    for reserved in [
                        0, 4, 8, 12, 0x40, 0x44, 0x48, 0x4c, 0x80, 0x84, 0x88, 0x8c, 0xc0, 0xc4,
                        0xc8, 0xcc,
                    ] {
                        let registers = addressing_registers(size | reserved, base, display);
                        let saved = registers;
                        for plane in [VdpPlane::A, VdpPlane::B] {
                            for (row, mapped) in SOURCE_ROWS.into_iter().zip(addressed_rows) {
                                if row >= rows {
                                    continue;
                                }
                                for within in [0, 1, height - 2, height - 1] {
                                    for column in [0, 1, columns - 1] {
                                        for pixel in [0, 7] {
                                            let x = (column * 8 + pixel) as u16;
                                            let y = (row * height + within) as u16;
                                            assert_eq!(
                                                memory.sample_plane_in_geometry(
                                                    &registers, plane, x, y, interlaced
                                                ),
                                                Ok(expected(
                                                    mapped * columns + column,
                                                    within,
                                                    pixel,
                                                    interlaced
                                                )),
                                                "size {:02x}, base {base:04x}, ({x},{y})",
                                                size | reserved
                                            );
                                        }
                                    }
                                }
                            }
                            for (x, y) in [(columns * 8, 0), (0, rows * height), (65535, 65535)] {
                                assert_eq!(
                                    memory.sample_plane_in_geometry(
                                        &registers, plane, x as u16, y as u16, interlaced
                                    ),
                                    Err(VdpRenderError::CoordinateOutOfBounds {
                                        x: x as u16,
                                        y: y as u16
                                    })
                                );
                            }
                        }
                        assert_eq!(registers, saved);
                    }
                }
            }
            assert_eq!(memory, saved_memory);
        }
    }
}

#[test]
fn sparse_and_single_row_scrolls_follow_literal_hardware_bands_in_both_fields() {
    for interlaced in [false, true] {
        let height = if interlaced { 16 } else { 8 };
        for base in [0x8000, 0xe000] {
            let mut memory = addressing_memory(base, interlaced);
            for display in [0, H40_CLOCK_MASK] {
                for (size, columns, _, addressed_rows) in SHAPES {
                    let registers = addressing_registers(size, base, display);
                    for (row, mapped) in SOURCE_ROWS.into_iter().zip(addressed_rows) {
                        for odd in [false, true] {
                            let within = usize::from(odd);
                            let physical = if row == 128 { 8 } else { 0 };
                            let geometry = RasterGeometry::new(physical, interlaced, odd);
                            let target = row * height + if interlaced { within } else { 0 };
                            let vertical = target - usize::from(geometry.source_y);
                            word(&mut memory, VSRAM_WRITE_CODE, 0, vertical as u16);
                            word(&mut memory, VSRAM_WRITE_CODE, 2, vertical as u16);
                            for plane in [VdpPlane::A, VdpPlane::B] {
                                for column in [0, 1, 31] {
                                    assert_eq!(
                                        memory.scrolled_plane(
                                            &registers,
                                            plane,
                                            (column * 8) as u16,
                                            geometry,
                                            true
                                        ),
                                        Ok(expected(
                                            mapped * columns + column,
                                            if interlaced { within } else { 0 },
                                            0,
                                            interlaced
                                        )),
                                        "size {size:02x}, row {row}, field {odd}"
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn window_rows_are_independent_of_plane_size_and_missing_r16() {
    let mut memory = VdpMemory::default();
    word(&mut memory, VRAM_WRITE_CODE, 0xb840, 1);
    word(&mut memory, VRAM_WRITE_CODE, 32, 0x1234);
    for size in [Some(2), Some(0x20), Some(0x22), Some(0x33), None] {
        let mut raw = values(0);
        raw[usize::from(PLANE_SIZE_REGISTER)] = size;
        raw[usize::from(WINDOW_X_REGISTER)] = Some(0x80);
        assert_eq!(
            memory
                .sample_window(&bank(raw), 0, 8)
                .unwrap()
                .unwrap()
                .color,
            1
        );
    }
}

#[test]
fn every_plane_base_keeps_last_name_inside_its_eight_kib_aperture() {
    let last_rows = [31, 31, 0, 31, 63, 63, 0, 31, 95, 31, 0, 31, 127, 63, 0, 31];
    for interlaced in [false, true] {
        let height = if interlaced { 16 } else { 8 };
        for base in (0..=0xe000u16).step_by(0x2000) {
            for ((size, columns, rows, _), mapped) in SHAPES.into_iter().zip(last_rows) {
                let mut memory = VdpMemory::default();
                let tile = if base < 0x4000 {
                    0x4000 / (height * 4) + 1
                } else {
                    1
                };
                let pattern = tile * height * 4 + (height - 1) * 4;
                word(&mut memory, VRAM_WRITE_CODE, pattern as u32 + 2, 0x1234);
                let address = usize::from(base) + (mapped * columns + columns - 1) * 2;
                word(
                    &mut memory,
                    VRAM_WRITE_CODE,
                    address as u32,
                    tile as u16 | 0xe000,
                );
                word(
                    &mut memory,
                    VRAM_WRITE_CODE,
                    u32::from(base.wrapping_add(0x2000)),
                    0,
                );
                let registers = addressing_registers(size, base, 0);
                for plane in [VdpPlane::A, VdpPlane::B] {
                    assert_eq!(
                        memory.sample_plane_in_geometry(
                            &registers,
                            plane,
                            (columns * 8 - 1) as u16,
                            (rows * height - 1) as u16,
                            interlaced
                        ),
                        Ok(VdpPixel {
                            color: 4,
                            palette: 3,
                            priority: true
                        }),
                        "size {size:02x}, base {base:04x}, address {address:04x}"
                    );
                }
            }
        }
    }
}
