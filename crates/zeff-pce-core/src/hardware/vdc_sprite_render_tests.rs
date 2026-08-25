use super::cpu::VdcPort;
use super::{
    HuC6270, SpriteBackgroundPriority, SpriteColorMode, SpritePixel, SpriteScanlineStatus,
    VDC_SATB_WORDS, VdcDmaChannel, VdcDmaProgress, VdcRegister, VdcStatus,
};

const SAT_SOURCE: usize = 0x7000;

fn write_register(vdc: &mut HuC6270, register: VdcRegister, value: u16) {
    vdc.write_port(VdcPort::SelectOrStatus, register as u8);
    vdc.write_port(VdcPort::DataLow, value as u8);
    vdc.write_port(VdcPort::DataHigh, (value >> 8) as u8);
}

fn sat_entry(x: i16, y: i16, pattern_code: u16, attributes: u16) -> [u16; 4] {
    [(y + 64) as u16, (x + 32) as u16, pattern_code, attributes]
}

fn load_satb(vdc: &mut HuC6270, entries: &[(usize, [u16; 4])]) {
    let mut words = [0; VDC_SATB_WORDS];
    for (index, entry) in entries {
        words[index * 4..index * 4 + 4].copy_from_slice(entry);
    }
    vdc.vram_mut()[SAT_SOURCE..SAT_SOURCE + VDC_SATB_WORDS].copy_from_slice(&words);
    write_register(vdc, VdcRegister::SatbSource, SAT_SOURCE as u16);
    assert!(vdc.start_satb_dma_for_vertical_blank());
    for _ in 0..VDC_SATB_WORDS - 1 {
        assert!(matches!(
            vdc.service_dma_slot(VdcDmaChannel::Satb),
            Ok(VdcDmaProgress::Transferred { .. })
        ));
    }
    assert_eq!(
        vdc.service_dma_slot(VdcDmaChannel::Satb),
        Ok(VdcDmaProgress::Complete)
    );
}

fn set_sprite_row(vdc: &mut HuC6270, base: usize, row: usize, colors: [u8; 16]) {
    for plane in 0..4 {
        let mut word = 0;
        for (column, color) in colors.into_iter().enumerate() {
            word |= u16::from((color >> plane) & 1) << (15 - column);
        }
        vdc.vram_mut()[base + plane * 16 + row] = word;
    }
}

fn set_solid_cell_row(vdc: &mut HuC6270, base: usize, cell: usize, row: usize, color: u8) {
    set_sprite_row(vdc, base + cell * 64, row, [color; 16]);
}

fn rendered_events(status: SpriteScanlineStatus) -> super::SpriteScanlineEvents {
    match status {
        SpriteScanlineStatus::Rendered(events) => events,
        SpriteScanlineStatus::Disabled => panic!("sprite renderer was disabled"),
    }
}

fn reference_sprite_scanline(
    vdc: &HuC6270,
    state: &super::SpriteRenderState,
    display_line: usize,
    output: &mut [Option<SpritePixel>],
) -> (bool, bool) {
    if !state.enabled() {
        return (false, false);
    }

    output.fill(None);
    let mut count = 0;
    let mut overflow = false;
    let mut collision = false;
    for sat_index in 0..64 {
        let base = sat_index * 4;
        let attributes = vdc.satb()[base + 3];
        let height = match (attributes >> 12) & 3 {
            0 => 16,
            1 => 32,
            _ => 64,
        };
        let top = i64::from(vdc.satb()[base] & 0x03FF) - 64;
        let row = i64::try_from(display_line).unwrap() - top;
        if row < 0 || row >= i64::from(height) {
            continue;
        }
        if count == 16 {
            overflow = true;
            break;
        }
        count += 1;

        let source_y = if attributes & 0x8000 == 0 {
            row as usize
        } else {
            height as usize - 1 - row as usize
        };
        let width = if attributes & 0x0100 == 0 { 16 } else { 32 };
        let left = i32::from(vdc.satb()[base + 1] & 0x03FF) - 32;
        let start = left.max(0) as usize;
        let end = (left + width).min(output.len() as i32).max(0) as usize;
        let pattern_code = vdc.satb()[base + 2] & 0x07FF;
        let palette = 0x0100 | ((attributes & 0x000F) << 4);
        let priority = if attributes & 0x0080 == 0 {
            SpriteBackgroundPriority::Background
        } else {
            SpriteBackgroundPriority::Sprite
        };
        for (display_x, destination) in output.iter_mut().enumerate().take(end).skip(start) {
            let local_x = (display_x as i32 - left) as usize;
            let source_x = if attributes & 0x0800 == 0 {
                local_x
            } else {
                width as usize - 1 - local_x
            };
            let cell_x = source_x / 16;
            let cell_y = source_y / 16;
            let mut code = pattern_code & 0x07FE;
            if width == 32 {
                code &= !0x0002;
            }
            if height == 32 {
                code &= !0x0004;
            } else if height == 64 {
                code &= !0x000C;
            }
            let pattern_base = (usize::from(code) << 5) + (cell_y * 2 + cell_x) * 64;
            let bit = 15 - source_x % 16;
            let color = match state.color_mode() {
                SpriteColorMode::Full => (0..4).fold(0, |color, plane| {
                    color
                        | (((vdc.vram()[(pattern_base + plane * 16 + source_y % 16) & 0x7FFF]
                            >> bit)
                            & 1) as u8)
                            << plane
                }),
                SpriteColorMode::PlanePair => {
                    let first_plane = usize::from(pattern_code & 1) * 2;
                    ((vdc.vram()[(pattern_base + first_plane * 16 + source_y % 16) & 0x7FFF]
                        >> bit)
                        & 1) as u8
                        | (((vdc.vram()
                            [(pattern_base + (first_plane + 1) * 16 + source_y % 16) & 0x7FFF]
                            >> bit)
                            & 1) as u8)
                            << 1
                }
            };
            if color == 0 {
                continue;
            }
            if let Some(existing) = destination {
                if existing.sat_index() == 0 && sat_index != 0 {
                    collision = true;
                }
            } else {
                *destination = Some(SpritePixel::new(
                    palette | u16::from(color),
                    priority,
                    sat_index as u8,
                ));
            }
        }
    }
    (collision, overflow)
}

#[test]
fn sprite_state_snapshots_enable_and_the_single_plane_pair_mode() {
    let mut vdc = HuC6270::new();
    let disabled = vdc.sprite_render_state();
    let sentinel = SpritePixel::new(0x1FF, SpriteBackgroundPriority::Sprite, 63);
    let mut output = [Some(sentinel)];

    write_register(&mut vdc, VdcRegister::Control, 0x40);
    for (memory_width, expected) in [
        (0x00, SpriteColorMode::Full),
        (0x04, SpriteColorMode::PlanePair),
        (0x08, SpriteColorMode::Full),
        (0x0C, SpriteColorMode::Full),
    ] {
        write_register(&mut vdc, VdcRegister::MemoryWidth, memory_width);
        assert_eq!(vdc.sprite_render_state().color_mode(), expected);
    }

    assert!(!disabled.enabled());
    assert_eq!(
        vdc.render_sprite_scanline(&disabled, 0, &mut output),
        Ok(SpriteScanlineStatus::Disabled)
    );
    assert_eq!(output, [Some(sentinel)]);
}

#[test]
fn sprite_pixels_decode_planes_palette_priority_and_sat_index() {
    let mut vdc = HuC6270::new();
    write_register(&mut vdc, VdcRegister::Control, 0x40);
    load_satb(&mut vdc, &[(0, sat_entry(0, 0, 2, 0x008A))]);
    set_sprite_row(
        &mut vdc,
        0x0040,
        0,
        [0, 1, 2, 4, 8, 15, 3, 12, 0, 1, 2, 4, 8, 15, 3, 12],
    );
    let mut output = [None; 16];

    let status = vdc
        .render_sprite_scanline(&vdc.sprite_render_state(), 0, &mut output)
        .unwrap();

    assert_eq!(rendered_events(status), Default::default());
    assert_eq!(output[0], None);
    let pixel = output[5].unwrap();
    assert_eq!(pixel.palette_index(), 0x01AF);
    assert_eq!(
        pixel.background_priority(),
        SpriteBackgroundPriority::Sprite
    );
    assert_eq!(pixel.sat_index(), 0);
    assert_eq!(output[7].unwrap().palette_index(), 0x01AC);
}

#[test]
fn coordinate_origins_clip_without_wrapping() {
    let mut vdc = HuC6270::new();
    write_register(&mut vdc, VdcRegister::Control, 0x40);
    load_satb(&mut vdc, &[(0, sat_entry(-1, -1, 2, 0))]);
    set_sprite_row(
        &mut vdc,
        0x0040,
        1,
        [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 1],
    );
    let mut output = [None; 3];

    vdc.render_sprite_scanline(&vdc.sprite_render_state(), 0, &mut output)
        .unwrap();

    assert_eq!(output[0].unwrap().palette_index(), 0x0102);
    assert_eq!(output[1].unwrap().palette_index(), 0x0103);
    assert_eq!(output[2].unwrap().palette_index(), 0x0104);
}

#[test]
fn pc_zero_is_not_an_address_bit_and_selects_planes_only_in_sm01() {
    let mut vdc = HuC6270::new();
    write_register(&mut vdc, VdcRegister::Control, 0x40);
    load_satb(
        &mut vdc,
        &[(0, sat_entry(0, 0, 2, 0)), (1, sat_entry(16, 0, 3, 0))],
    );
    set_sprite_row(&mut vdc, 0x0040, 0, [9; 16]);
    let mut output = [None; 32];

    let full = vdc.sprite_render_state();
    vdc.render_sprite_scanline(&full, 0, &mut output).unwrap();
    assert_eq!(output[0].unwrap().palette_index(), 0x0109);
    assert_eq!(output[16].unwrap().palette_index(), 0x0109);

    write_register(&mut vdc, VdcRegister::MemoryWidth, 0x04);
    let plane_pair = vdc.sprite_render_state();
    vdc.render_sprite_scanline(&plane_pair, 0, &mut output)
        .unwrap();
    assert_eq!(output[0].unwrap().palette_index(), 0x0101);
    assert_eq!(output[16].unwrap().palette_index(), 0x0102);
}

#[test]
fn combined_sizes_normalize_pattern_groups_and_apply_both_flips() {
    let mut vdc = HuC6270::new();
    write_register(&mut vdc, VdcRegister::Control, 0x40);
    load_satb(&mut vdc, &[(0, sat_entry(0, 0, 0x000E, 0xB900))]);
    set_solid_cell_row(&mut vdc, 0, 6, 15, 7);
    set_solid_cell_row(&mut vdc, 0, 7, 15, 8);
    let mut output = [None; 32];

    vdc.render_sprite_scanline(&vdc.sprite_render_state(), 0, &mut output)
        .unwrap();

    assert!(
        output[..16]
            .iter()
            .all(|pixel| pixel.unwrap().palette_index() == 0x0108)
    );
    assert!(
        output[16..]
            .iter()
            .all(|pixel| pixel.unwrap().palette_index() == 0x0107)
    );
}

#[test]
fn two_cell_height_clears_pc2_and_uses_even_vertical_block_offsets() {
    let mut vdc = HuC6270::new();
    write_register(&mut vdc, VdcRegister::Control, 0x40);
    load_satb(&mut vdc, &[(0, sat_entry(0, 0, 0x000C, 0x1000))]);
    set_solid_cell_row(&mut vdc, 0x0100, 0, 0, 3);
    set_solid_cell_row(&mut vdc, 0x0100, 2, 0, 6);
    let mut top = [None; 1];
    let mut bottom = [None; 1];

    vdc.render_sprite_scanline(&vdc.sprite_render_state(), 0, &mut top)
        .unwrap();
    vdc.render_sprite_scanline(&vdc.sprite_render_state(), 16, &mut bottom)
        .unwrap();

    assert_eq!(top[0].unwrap().palette_index(), 0x0103);
    assert_eq!(bottom[0].unwrap().palette_index(), 0x0106);
}

#[test]
fn lower_sat_index_wins_and_sprite_zero_overlap_reports_collision() {
    let mut vdc = HuC6270::new();
    write_register(&mut vdc, VdcRegister::Control, 0x40);
    load_satb(
        &mut vdc,
        &[(0, sat_entry(0, 0, 2, 0)), (1, sat_entry(0, 0, 4, 0x0080))],
    );
    set_sprite_row(&mut vdc, 0x0040, 0, [1; 16]);
    set_sprite_row(&mut vdc, 0x0080, 0, [2; 16]);
    let mut output = [None; 16];

    let status = vdc
        .render_sprite_scanline(&vdc.sprite_render_state(), 0, &mut output)
        .unwrap();
    let events = rendered_events(status);

    assert!(events.collision_within_output());
    assert!(!events.overflow());
    assert!(output.iter().all(|pixel| pixel.unwrap().sat_index() == 0));
    assert_eq!(vdc.status(), VdcStatus::empty());
}

#[test]
fn transparent_sprite_zero_does_not_collide_or_hide_a_later_sprite() {
    let mut vdc = HuC6270::new();
    write_register(&mut vdc, VdcRegister::Control, 0x40);
    load_satb(
        &mut vdc,
        &[(0, sat_entry(0, 0, 2, 0)), (1, sat_entry(0, 0, 4, 0))],
    );
    set_sprite_row(&mut vdc, 0x0080, 0, [2; 16]);
    let mut output = [None; 1];

    let status = vdc
        .render_sprite_scanline(&vdc.sprite_render_state(), 0, &mut output)
        .unwrap();

    assert!(!rendered_events(status).collision_within_output());
    assert_eq!(output[0].unwrap().sat_index(), 1);
}

#[test]
fn seventeenth_vertically_qualified_sprite_sets_overflow_and_is_not_rendered() {
    let mut vdc = HuC6270::new();
    write_register(&mut vdc, VdcRegister::Control, 0x40);
    let entries = (0..17)
        .map(|index| (index, sat_entry((index * 16) as i16, 0, 2, 0)))
        .collect::<Vec<_>>();
    load_satb(&mut vdc, &entries);
    set_sprite_row(&mut vdc, 0x0040, 0, [1; 16]);
    let mut output = [None; 17 * 16];

    let status = vdc
        .render_sprite_scanline(&vdc.sprite_render_state(), 0, &mut output)
        .unwrap();
    let events = rendered_events(status);

    assert!(events.overflow());
    assert!(!events.collision_within_output());
    assert!(output[..16 * 16].iter().all(Option::is_some));
    assert!(output[16 * 16..].iter().all(Option::is_none));
    assert_eq!(vdc.status(), VdcStatus::empty());
}

#[test]
fn both_large_height_encodings_render_sixty_four_lines_and_share_alignment() {
    for attributes in [0x2000, 0x3000] {
        let mut vdc = HuC6270::new();
        write_register(&mut vdc, VdcRegister::Control, 0x40);
        load_satb(&mut vdc, &[(0, sat_entry(0, 0, 0x000E, attributes))]);
        set_solid_cell_row(&mut vdc, 0x0040, 0, 0, 1);
        set_solid_cell_row(&mut vdc, 0x0040, 6, 15, 7);
        let mut output = [None; 1];

        vdc.render_sprite_scanline(&vdc.sprite_render_state(), 0, &mut output)
            .unwrap();
        assert_eq!(output[0].unwrap().palette_index(), 0x0101);

        vdc.render_sprite_scanline(&vdc.sprite_render_state(), 63, &mut output)
            .unwrap();
        assert_eq!(output[0].unwrap().palette_index(), 0x0107);

        vdc.render_sprite_scanline(&vdc.sprite_render_state(), 64, &mut output)
            .unwrap();
        assert_eq!(output, [None]);
    }
}

#[test]
fn horizontally_clipped_sprites_count_toward_overflow_without_pattern_faults() {
    let mut vdc = HuC6270::new();
    write_register(&mut vdc, VdcRegister::Control, 0x40);
    let entries = (0..17)
        .map(|index| (index, sat_entry(-16, 0, 0x0400, 0)))
        .collect::<Vec<_>>();
    load_satb(&mut vdc, &entries);
    let sentinel = SpritePixel::new(0x1FF, SpriteBackgroundPriority::Sprite, 63);
    let mut output = [Some(sentinel); 1];

    let status = vdc
        .render_sprite_scanline(&vdc.sprite_render_state(), 0, &mut output)
        .unwrap();

    assert!(rendered_events(status).overflow());
    assert_eq!(output, [None]);
}

#[test]
fn collision_reporting_is_local_to_the_caller_output_span() {
    let mut vdc = HuC6270::new();
    write_register(&mut vdc, VdcRegister::Control, 0x40);
    load_satb(
        &mut vdc,
        &[(0, sat_entry(16, 0, 2, 0)), (1, sat_entry(16, 0, 4, 0))],
    );
    set_sprite_row(&mut vdc, 0x0040, 0, [1; 16]);
    set_sprite_row(&mut vdc, 0x0080, 0, [2; 16]);
    let state = vdc.sprite_render_state();
    let mut cropped = [None; 16];
    let mut includes_overlap = [None; 17];

    let cropped_status = vdc.render_sprite_scanline(&state, 0, &mut cropped).unwrap();
    let full_status = vdc
        .render_sprite_scanline(&state, 0, &mut includes_overlap)
        .unwrap();

    assert!(!rendered_events(cropped_status).collision_within_output());
    assert!(rendered_events(full_status).collision_within_output());
}

#[test]
fn upper_sprite_patterns_mirror_address_bit_fifteen() {
    let mut vdc = HuC6270::new();
    write_register(&mut vdc, VdcRegister::Control, 0x40);
    load_satb(&mut vdc, &[(0, sat_entry(0, 0, 0x0400, 0x0003))]);
    set_sprite_row(&mut vdc, 0, 0, [9; 16]);
    let mut output = [None; 2];
    assert_eq!(
        vdc.render_sprite_scanline(&vdc.sprite_render_state(), 0, &mut output),
        Ok(SpriteScanlineStatus::Rendered(Default::default()))
    );
    assert_eq!(output[0].unwrap().palette_index(), 0x139);
    assert_eq!(output[1].unwrap().palette_index(), 0x139);
}

#[test]
fn cell_cached_renderer_matches_pixel_reference_for_all_sizes_flips_rows_and_clipping() {
    for color_mode in [0x0000, 0x0004] {
        for height_attributes in [0x0000, 0x1000, 0x2000, 0x3000] {
            for width_attributes in [0x0000, 0x0100] {
                for horizontal_flip in [0x0000, 0x0800] {
                    let mut vdc = HuC6270::new();
                    write_register(&mut vdc, VdcRegister::Control, 0x40);
                    write_register(&mut vdc, VdcRegister::MemoryWidth, color_mode);
                    let mut seed = 0xC0DE_1234_u32;
                    for word in vdc.vram_mut().iter_mut() {
                        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                        *word = (seed >> 16) as u16;
                    }
                    let attributes =
                        height_attributes | width_attributes | horizontal_flip | 0x0085;
                    let entries = (0..17)
                        .map(|index| (index, sat_entry(-8, 0, 2, attributes)))
                        .collect::<Vec<_>>();
                    load_satb(&mut vdc, &entries);
                    for base in [0, 0x0040, 0x0080] {
                        set_sprite_row(&mut vdc, base, 0, [0; 16]);
                        set_sprite_row(&mut vdc, base, 1, [1; 16]);
                    }
                    let state = vdc.sprite_render_state();
                    let height = match height_attributes {
                        0x0000 => 16,
                        0x1000 => 32,
                        _ => 64,
                    };

                    for display_line in 0..height {
                        let mut actual = [None; 48];
                        let mut expected = [None; 48];
                        let actual_status = vdc
                            .render_sprite_scanline(&state, display_line, &mut actual)
                            .unwrap();
                        let (expected_collision, expected_overflow) =
                            reference_sprite_scanline(&vdc, &state, display_line, &mut expected);
                        let actual_events = rendered_events(actual_status);
                        assert_eq!(actual_events.collision_within_output(), expected_collision);
                        assert_eq!(actual_events.overflow(), expected_overflow);
                        assert_eq!(actual, expected);
                    }

                    let mut transparent = [None; 48];
                    let transparent_status = vdc
                        .render_sprite_scanline(&state, 0, &mut transparent)
                        .unwrap();
                    let transparent_events = rendered_events(transparent_status);
                    assert!(transparent_events.overflow());
                    assert!(!transparent_events.collision_within_output());
                    let mut opaque = [None; 48];
                    let opaque_events = rendered_events(
                        vdc.render_sprite_scanline(&state, 1, &mut opaque).unwrap(),
                    );
                    assert!(opaque_events.overflow());
                    assert!(opaque_events.collision_within_output());
                }
            }
        }
    }
}
