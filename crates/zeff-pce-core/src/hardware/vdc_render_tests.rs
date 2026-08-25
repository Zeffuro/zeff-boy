use super::cpu::VdcPort;
use super::{
    BackgroundColorMode, BackgroundRenderState, BackgroundScanlineStatus, HuC6270, VdcRegister,
};

fn write_register(vdc: &mut HuC6270, register: VdcRegister, value: u16) {
    vdc.write_port(VdcPort::SelectOrStatus, register as u8);
    vdc.write_port(VdcPort::DataLow, value as u8);
    vdc.write_port(VdcPort::DataHigh, (value >> 8) as u8);
}

fn encode_row(colors: [u8; 8]) -> (u16, u16) {
    let mut planes_zero_one = 0;
    let mut planes_two_three = 0;
    for (column, color) in colors.into_iter().enumerate() {
        let bit = 7 - column;
        planes_zero_one |= u16::from(color & 1) << bit;
        planes_zero_one |= u16::from((color >> 1) & 1) << (bit + 8);
        planes_two_three |= u16::from((color >> 2) & 1) << bit;
        planes_two_three |= u16::from((color >> 3) & 1) << (bit + 8);
    }
    (planes_zero_one, planes_two_three)
}

fn set_pattern_row(vdc: &mut HuC6270, code: usize, row: usize, colors: [u8; 8]) {
    let (planes_zero_one, planes_two_three) = encode_row(colors);
    vdc.vram_mut()[code * 16 + row] = planes_zero_one;
    vdc.vram_mut()[code * 16 + 8 + row] = planes_two_three;
}

fn reference_background_pixel(
    vdc: &HuC6270,
    state: &BackgroundRenderState,
    display_line: usize,
) -> u8 {
    let width_pixels = state.width_tiles() * 8;
    let height_pixels = state.height_tiles() * 8;
    let virtual_y = (state.scroll_y() + display_line % height_pixels) % height_pixels;
    let virtual_x = state.scroll_x() % width_pixels;
    let entry = vdc.vram()[(virtual_y / 8) * state.width_tiles() + virtual_x / 8];
    let base = usize::from(entry & 0x0FFF) << 4;
    let row = virtual_y % 8;
    let bit = 7 - virtual_x % 8;
    let (planes_zero_one, planes_two_three) = match state.color_mode() {
        BackgroundColorMode::Full => (
            vdc.read_logical_vram_word((base + row) as u16),
            vdc.read_logical_vram_word((base + 8 + row) as u16),
        ),
        BackgroundColorMode::PlanesZeroAndOne => {
            (vdc.read_logical_vram_word((base + row) as u16), 0)
        }
        BackgroundColorMode::PlanesTwoAndThree => {
            (0, vdc.read_logical_vram_word((base + 8 + row) as u16))
        }
    };
    let pattern = ((planes_zero_one >> bit) & 1) as u8
        | (((planes_zero_one >> (bit + 8)) & 1) as u8) << 1
        | (((planes_two_three >> bit) & 1) as u8) << 2
        | (((planes_two_three >> (bit + 8)) & 1) as u8) << 3;
    if pattern == 0 {
        0
    } else {
        ((entry >> 8) as u8 & 0xF0) | pattern
    }
}

#[test]
fn background_decodes_all_planes_bit_order_palette_and_color_zero() {
    let mut vdc = HuC6270::new();
    write_register(&mut vdc, VdcRegister::Control, 0x80);
    let code = 0x20;
    vdc.vram_mut()[0] = 0xA000 | code as u16;
    set_pattern_row(&mut vdc, code, 0, [0, 1, 2, 4, 8, 15, 3, 12]);
    let state = vdc.background_render_state();
    let mut output = [0xFF; 8];

    assert_eq!(
        vdc.render_background_scanline(&state, 0, &mut output),
        Ok(BackgroundScanlineStatus::Rendered)
    );
    assert_eq!(output, [0, 0xA1, 0xA2, 0xA4, 0xA8, 0xAF, 0xA3, 0xAC]);
}

#[test]
fn render_state_decodes_all_mwr_geometries_and_color_modes() {
    let mut vdc = HuC6270::new();
    for (memory_width, width, height, mode) in [
        (0x00, 32, 32, BackgroundColorMode::Full),
        (0x10, 64, 32, BackgroundColorMode::Full),
        (0x20, 128, 32, BackgroundColorMode::Full),
        (0x30, 128, 32, BackgroundColorMode::Full),
        (0x40, 32, 64, BackgroundColorMode::Full),
        (0x50, 64, 64, BackgroundColorMode::Full),
        (0x60, 128, 64, BackgroundColorMode::Full),
        (0x70, 128, 64, BackgroundColorMode::Full),
        (0x03, 32, 32, BackgroundColorMode::PlanesZeroAndOne),
        (0x83, 32, 32, BackgroundColorMode::PlanesTwoAndThree),
    ] {
        write_register(&mut vdc, VdcRegister::MemoryWidth, memory_width);
        let state = vdc.background_render_state();
        assert_eq!(state.width_tiles(), width);
        assert_eq!(state.height_tiles(), height);
        assert_eq!(state.color_mode(), mode);
    }
}

#[test]
fn four_color_modes_place_the_selected_planes_in_the_documented_bits() {
    let mut vdc = HuC6270::new();
    write_register(&mut vdc, VdcRegister::Control, 0x80);
    let code = 0x20;
    vdc.vram_mut()[0] = 0x5000 | code as u16;
    set_pattern_row(&mut vdc, code, 0, [0, 15, 15, 15, 15, 15, 15, 15]);

    for (memory_width, expected) in [
        (0x00, 0x5F),
        (0x01, 0x5F),
        (0x02, 0x5F),
        (0x03, 0x53),
        (0x83, 0x5C),
    ] {
        write_register(&mut vdc, VdcRegister::MemoryWidth, memory_width);
        let state = vdc.background_render_state();
        let mut output = [0xFF; 2];
        vdc.render_background_scanline(&state, 0, &mut output)
            .unwrap();
        assert_eq!(output, [0, expected]);
    }
}

#[test]
fn scroll_wraps_across_the_virtual_screen_edges() {
    let mut vdc = HuC6270::new();
    write_register(&mut vdc, VdcRegister::Control, 0x80);
    write_register(&mut vdc, VdcRegister::MemoryWidth, 0x50);
    write_register(&mut vdc, VdcRegister::BackgroundScrollX, 511);
    write_register(&mut vdc, VdcRegister::BackgroundScrollY, 511);
    let left_code = 0x20;
    let right_code = 0x21;
    vdc.vram_mut()[0] = 0x2000 | left_code;
    vdc.vram_mut()[63] = 0x3000 | right_code;
    set_pattern_row(&mut vdc, left_code as usize, 0, [2, 0, 0, 0, 0, 0, 0, 0]);
    set_pattern_row(&mut vdc, right_code as usize, 0, [0, 0, 0, 0, 0, 0, 0, 1]);
    let state = vdc.background_render_state();
    let mut output = [0; 2];

    vdc.render_background_scanline(&state, 1, &mut output)
        .unwrap();

    assert_eq!(output, [0x31, 0x22]);
}

#[test]
fn vertical_scroll_advances_from_the_last_tile_row_without_wrapping() {
    let mut vdc = HuC6270::new();
    write_register(&mut vdc, VdcRegister::Control, 0x80);
    write_register(&mut vdc, VdcRegister::BackgroundScrollY, 7);
    let first_code = 0x20;
    let second_code = 0x21;
    vdc.vram_mut()[0] = 0x1000 | first_code;
    vdc.vram_mut()[32] = 0x2000 | second_code;
    set_pattern_row(&mut vdc, first_code as usize, 7, [1; 8]);
    set_pattern_row(&mut vdc, second_code as usize, 0, [2; 8]);
    let state = vdc.background_render_state();
    let mut first_line = [0; 1];
    let mut second_line = [0; 1];

    vdc.render_background_scanline(&state, 0, &mut first_line)
        .unwrap();
    vdc.render_background_scanline(&state, 1, &mut second_line)
        .unwrap();

    assert_eq!(first_line, [0x11]);
    assert_eq!(second_line, [0x22]);
}

#[test]
fn snapshots_preserve_caller_selected_scroll_latching() {
    let mut vdc = HuC6270::new();
    write_register(&mut vdc, VdcRegister::Control, 0x80);
    let first_code = 0x20;
    let second_code = 0x21;
    vdc.vram_mut()[0] = 0x1000 | first_code;
    vdc.vram_mut()[1] = 0x2000 | second_code;
    set_pattern_row(&mut vdc, first_code as usize, 0, [1; 8]);
    set_pattern_row(&mut vdc, second_code as usize, 0, [2; 8]);
    let old_state = vdc.background_render_state();
    write_register(&mut vdc, VdcRegister::BackgroundScrollX, 8);
    let new_state = vdc.background_render_state();
    let mut old_output = [0; 1];
    let mut new_output = [0; 1];

    vdc.render_background_scanline(&old_state, 0, &mut old_output)
        .unwrap();
    vdc.render_background_scanline(&new_state, 0, &mut new_output)
        .unwrap();

    assert_eq!(old_output, [0x11]);
    assert_eq!(new_output, [0x22]);
}

#[test]
fn disabled_background_is_distinct_from_an_enabled_zero_pixel() {
    let mut vdc = HuC6270::new();
    let disabled = vdc.background_render_state();
    let mut output = [0xAA; 1];
    assert_eq!(
        vdc.render_background_scanline(&disabled, 0, &mut output),
        Ok(BackgroundScanlineStatus::Disabled)
    );
    assert_eq!(output, [0xAA]);

    write_register(&mut vdc, VdcRegister::Control, 0x80);
    let enabled = vdc.background_render_state();
    assert_eq!(
        vdc.render_background_scanline(&enabled, 0, &mut output),
        Ok(BackgroundScanlineStatus::Rendered)
    );
    assert_eq!(output, [0]);
}

#[test]
fn upper_pattern_addresses_mirror_address_bit_fifteen() {
    let mut vdc = HuC6270::new();
    write_register(&mut vdc, VdcRegister::Control, 0x80);
    vdc.vram_mut()[0] = 0xF801;
    set_pattern_row(&mut vdc, 1, 0, [1; 8]);
    let (_, high_planes) = encode_row([12; 8]);
    vdc.vram_mut()[0x18] = high_planes;
    let mut output = [0; 1];

    let full = vdc.background_render_state();
    assert_eq!(
        vdc.render_background_scanline(&full, 0, &mut output),
        Ok(BackgroundScanlineStatus::Rendered)
    );
    assert_eq!(output, [0xFD]);

    write_register(&mut vdc, VdcRegister::MemoryWidth, 0x83);
    let upper_planes = vdc.background_render_state();
    assert_eq!(
        vdc.render_background_scanline(&upper_planes, 0, &mut output),
        Ok(BackgroundScanlineStatus::Rendered)
    );
    assert_eq!(output, [0xFC]);
}

#[test]
fn after_burner_upper_pattern_word_ccc0_reads_lower_mirror_4cc0() {
    let mut vdc = HuC6270::new();
    write_register(&mut vdc, VdcRegister::Control, 0x80);
    vdc.vram_mut()[0] = 0x3CCC;
    set_pattern_row(&mut vdc, 0x4CC, 0, [5; 8]);
    let state = vdc.background_render_state();
    let mut output = [0; 8];

    assert_eq!(
        vdc.render_background_scanline(&state, 0, &mut output),
        Ok(BackgroundScanlineStatus::Rendered)
    );
    assert_eq!(output, [0x35; 8]);
}

#[test]
fn tile_cached_background_rendering_matches_the_pixel_reference_for_all_bat_coordinates() {
    let mut vdc = HuC6270::new();
    write_register(&mut vdc, VdcRegister::Control, 0x80);
    for (index, word) in vdc.vram_mut().iter_mut().enumerate() {
        *word = (index as u16).wrapping_mul(0x9E37).rotate_left(5);
    }

    for memory_width in [0x00, 0x10, 0x20, 0x40, 0x50, 0x60, 0x03, 0x83] {
        write_register(&mut vdc, VdcRegister::MemoryWidth, memory_width);
        write_register(&mut vdc, VdcRegister::BackgroundScrollY, 0);
        let dimensions = vdc.background_render_state();
        let width = dimensions.width_tiles() * 8;
        let height = dimensions.height_tiles() * 8;
        for scroll_x in 0..width {
            write_register(&mut vdc, VdcRegister::BackgroundScrollX, scroll_x as u16);
            let state = vdc.background_render_state();
            for display_line in 0..height {
                let expected = reference_background_pixel(&vdc, &state, display_line);
                let mut actual = [0xFF];
                vdc.render_background_scanline(&state, display_line, &mut actual)
                    .unwrap();
                assert_eq!(
                    actual,
                    [expected],
                    "MWR {memory_width:#04X}, x {scroll_x}, y {display_line}"
                );
            }
        }
    }
}
