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
fn reserved_height_entries_are_ignored_off_line_and_after_sixteen_entries() {
    let mut off_line = HuC6270::new();
    write_register(&mut off_line, VdcRegister::Control, 0x40);
    load_satb(
        &mut off_line,
        &[
            (0, sat_entry(0, -40, 0x0400, 0x2000)),
            (1, sat_entry(0, 0, 2, 0)),
        ],
    );
    set_sprite_row(&mut off_line, 0x0040, 0, [1; 16]);
    let mut output = [None; 1];
    let status = off_line
        .render_sprite_scanline(&off_line.sprite_render_state(), 0, &mut output)
        .unwrap();
    assert_eq!(output[0].unwrap().sat_index(), 1);
    assert!(!rendered_events(status).overflow());

    let mut after_cap = HuC6270::new();
    write_register(&mut after_cap, VdcRegister::Control, 0x40);
    let mut entries = (0..16)
        .map(|index| (index, sat_entry(0, 0, 2, 0)))
        .collect::<Vec<_>>();
    entries.push((16, sat_entry(0, 0, 0x0400, 0x2000)));
    load_satb(&mut after_cap, &entries);
    set_sprite_row(&mut after_cap, 0x0040, 0, [1; 16]);
    let mut output = [None; 1];
    let status = after_cap
        .render_sprite_scanline(&after_cap.sprite_render_state(), 0, &mut output)
        .unwrap();
    assert!(!rendered_events(status).overflow());
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
