use super::*;
use crate::hardware::constants::{
    SMS_SCREEN_H, SMS_SCREEN_W, VDP_STATUS_SPRITE_COLLISION, VDP_STATUS_VBLANK,
};

const SMS_RED: u8 = 0x03;
const SMS_GREEN: u8 = 0x0C;
const SMS_RED_RGBA: [u8; RGBA_CHANNELS] = [0xFF, 0x00, 0x00, 0xFF];
const SMS_GREEN_RGBA: [u8; RGBA_CHANNELS] = [0x00, 0xFF, 0x00, 0xFF];
const MODE4_TEST_SPRITE_TABLE_REGISTER: u8 = 0x7E;
const MODE4_TOP_SCANLINE_SPRITE_Y: u8 = 0xFF;

fn set_tile_row(vdp: &mut Vdp, tile_index: usize, row: usize, planes: [u8; 4]) {
    let base = tile_index * SMS_MODE4_TILE_BYTES + row * 4;
    vdp.vram[base..base + 4].copy_from_slice(&planes);
}

fn set_tile_row_at(
    vdp: &mut Vdp,
    pattern_base: usize,
    tile_index: usize,
    row: usize,
    planes: [u8; 4],
) {
    let base = pattern_base + tile_index * SMS_MODE4_TILE_BYTES + row * 4;
    vdp.vram[base..base + 4].copy_from_slice(&planes);
}

fn render_area(width: usize, height: usize, source_x: usize, source_y: usize) -> Mode4RenderArea {
    Mode4RenderArea::new(width, height, source_x, source_y)
}

fn reference_mode4_background_rgba(
    vdp: &Vdp,
    area: Mode4RenderArea,
    color_mode: Mode4ColorMode,
) -> Vec<u8> {
    let mut framebuffer = vec![0; area.expected_rgba_len()];
    let name_table_base = vdp.mode4_name_table_base();
    for y in 0..area.height {
        for x in 0..area.width {
            let pixel =
                vdp.mode4_background_pixel(name_table_base, area.source_x + x, area.source_y + y);
            let rgba = vdp.mode4_color_rgba(pixel.color_index, color_mode);
            let offset = (y * area.width + x) * RGBA_CHANNELS;
            framebuffer[offset..offset + RGBA_CHANNELS].copy_from_slice(&rgba);
        }
    }
    framebuffer
}

fn set_name_entry(vdp: &mut Vdp, tile_x: usize, tile_y: usize, entry: u16) {
    vdp.registers[MODE4_NAME_TABLE_REGISTER] = MODE4_NAME_TABLE_MASK;
    let base = vdp.mode4_name_table_base();
    let offset = base + ((tile_y * SMS_NAME_TABLE_COLUMNS + tile_x) * SMS_NAME_TABLE_ENTRY_BYTES);
    let [lo, hi] = entry.to_le_bytes();
    vdp.vram[offset] = lo;
    vdp.vram[offset + 1] = hi;
}

fn use_mode4_test_sprite_table(vdp: &mut Vdp) -> usize {
    vdp.registers[MODE4_SPRITE_TABLE_REGISTER] = MODE4_TEST_SPRITE_TABLE_REGISTER;
    vdp.mode4_sprite_table_base()
}

fn set_mode4_sprite(
    vdp: &mut Vdp,
    sprite_table: usize,
    sprite_index: usize,
    y_raw: u8,
    x: u8,
    tile_index: u8,
) {
    vdp.vram[sprite_table + sprite_index] = y_raw;
    let x_tile_offset = sprite_table + MODE4_SPRITE_X_TILE_TABLE_OFFSET + sprite_index * 2;
    vdp.vram[x_tile_offset] = x;
    vdp.vram[x_tile_offset + 1] = tile_index;
}

fn terminate_mode4_sprites(vdp: &mut Vdp, sprite_table: usize, sprite_index: usize) {
    vdp.vram[sprite_table + sprite_index] = MODE4_SPRITE_TERMINATOR_Y;
}

fn set_tms_name(vdp: &mut Vdp, tile_x: usize, tile_y: usize, pattern: u8) {
    let base = vdp.tms_name_table_base();
    vdp.vram[base + tile_y * TMS_TILE_COLUMNS + tile_x] = pattern;
}

fn set_tms_pattern_row(vdp: &mut Vdp, pattern_base: usize, pattern: u8, row: usize, byte: u8) {
    vdp.vram[pattern_base + usize::from(pattern) * SMS_TILE_SIZE + row] = byte;
}

fn set_tms_color_row(vdp: &mut Vdp, color_base: usize, pattern: u8, row: usize, byte: u8) {
    vdp.vram[color_base + usize::from(pattern) * SMS_TILE_SIZE + row] = byte;
}

fn set_tms_sprite(vdp: &mut Vdp, index: usize, y: isize, x: u8, pattern: u8, color: u8) {
    let base = vdp.tms_sprite_attribute_table_base() + index * TMS_SPRITE_ATTRIBUTE_BYTES;
    vdp.vram[base] = (y as u8).wrapping_sub(1);
    vdp.vram[base + 1] = x;
    vdp.vram[base + 2] = pattern;
    vdp.vram[base + 3] = color;
}

#[test]
fn control_port_sets_vram_write_address_and_data_port_writes_vram() {
    let mut vdp = Vdp::new();

    vdp.write_control(0x34);
    vdp.write_control(0x41);
    vdp.write_data(0xAA);

    assert_eq!(vdp.vram()[0x0134], 0xAA);
    assert_eq!(vdp.address(), 0x0135);
}

#[test]
fn control_port_register_write_updates_registers() {
    let mut vdp = Vdp::new();

    vdp.write_control(0xE4);
    vdp.write_control(0x82);

    assert_eq!(vdp.registers()[2], 0xE4);
    assert_eq!(vdp.code(), VDP_CODE_REGISTER_WRITE);
}

#[test]
fn mode4_background_spans_match_per_pixel_reference() {
    let mut vdp = Vdp::new_with_video_standard_and_color_mode(
        Sega8VideoStandard::Ntsc,
        Mode4ColorMode::GameGear,
    );
    vdp.registers[VDP_REGISTER_HORIZONTAL_SCROLL] = 13;
    vdp.registers[VDP_REGISTER_VERTICAL_SCROLL] = 241;
    for (index, byte) in vdp.vram.iter_mut().enumerate() {
        *byte = index.wrapping_mul(37).wrapping_add(19) as u8;
    }
    for (index, byte) in vdp.cram.iter_mut().enumerate() {
        *byte = index.wrapping_mul(11).wrapping_add(7) as u8;
    }

    for (mode_control_1, mode_control_2, name_table_register, areas) in [
        (
            VDP_REG0_MODE4,
            VDP_REG1_DISPLAY_ENABLE,
            MODE4_NAME_TABLE_MASK,
            [
                render_area(SMS_SCREEN_W, SMS_SCREEN_H, 0, 0),
                render_area(160, 144, 48, 24),
                render_area(23, 19, 181, 7),
            ],
        ),
        (
            VDP_REG0_MODE4 | VDP_REG0_MODE4_EXTENDED_HEIGHT,
            VDP_REG1_DISPLAY_ENABLE | VDP_REG1_MODE4_224_LINE,
            0x0C,
            [
                render_area(SMS_SCREEN_W, 224, 0, 0),
                render_area(160, 144, 48, 24),
                render_area(23, 19, 181, 207),
            ],
        ),
        (
            VDP_REG0_MODE4
                | VDP_REG0_MODE4_EXTENDED_HEIGHT
                | VDP_REG0_HORIZONTAL_SCROLL_LOCK
                | VDP_REG0_VERTICAL_SCROLL_LOCK,
            VDP_REG1_DISPLAY_ENABLE | VDP_REG1_MODE4_240_LINE,
            0x0C,
            [
                render_area(SMS_SCREEN_W, 240, 0, 0),
                render_area(160, 144, 48, 24),
                render_area(23, 19, 181, 223),
            ],
        ),
    ] {
        vdp.registers[VDP_REGISTER_MODE_CONTROL_1] = mode_control_1;
        vdp.registers[VDP_REGISTER_MODE_CONTROL_2] = mode_control_2;
        vdp.registers[MODE4_NAME_TABLE_REGISTER] = name_table_register;
        for area in areas {
            for color_mode in [Mode4ColorMode::Sms, Mode4ColorMode::GameGear] {
                let mut actual = vec![0; area.expected_rgba_len()];
                render::render_mode4_background_rgba_with_color(
                    &vdp,
                    &mut actual,
                    area,
                    color_mode,
                );
                assert_eq!(
                    actual,
                    reference_mode4_background_rgba(&vdp, area, color_mode),
                    "area {area:?}, color mode {color_mode:?}",
                );
            }
        }
    }
}

#[test]
fn tms_debug_snapshot_exposes_mode_and_table_bases() {
    let mut vdp = Vdp::new();
    vdp.registers[VDP_REGISTER_MODE_CONTROL_1] = TMS_REG0_MODE_GRAPHICS_II;
    vdp.registers[TMS_REGISTER_NAME_TABLE] = 0x0F;
    vdp.registers[TMS_REGISTER_COLOR_TABLE] = 0x80;
    vdp.registers[TMS_REGISTER_PATTERN_TABLE] = 0x04;
    vdp.registers[TMS_REGISTER_SPRITE_ATTRIBUTE_TABLE] = 0x7F;
    vdp.registers[TMS_REGISTER_SPRITE_PATTERN_TABLE] = 0x07;

    let debug = vdp.tms9918_debug_snapshot();

    assert_eq!(debug.mode, Tms9918Mode::GraphicsII);
    assert_eq!(debug.name_table_base, 0x3C00);
    assert_eq!(debug.pattern_table_base, 0x2000);
    assert_eq!(debug.color_table_base, 0x2000);
    assert_eq!(debug.sprite_attribute_table_base, 0x3F80);
    assert_eq!(debug.sprite_pattern_table_base, 0x3800);
}

#[test]
fn control_port_sets_cram_write_address_and_data_port_writes_cram() {
    let mut vdp = Vdp::new();

    vdp.write_control(0x03);
    vdp.write_control(0xC0);
    vdp.write_data(0x2A);

    assert_eq!(vdp.cram()[3], 0x2A);
    assert_eq!(vdp.address(), 4);
}

#[test]
fn game_gear_cram_writes_commit_on_high_byte() {
    let mut vdp = Vdp::new_with_video_standard_and_color_mode(
        Sega8VideoStandard::Ntsc,
        Mode4ColorMode::GameGear,
    );

    vdp.write_control(0x02);
    vdp.write_control(0xC0);
    vdp.write_data(0x0F);
    assert_eq!(vdp.cram()[2], 0x00);

    vdp.write_data(0x07);

    assert_eq!(vdp.cram()[2], 0x0F);
    assert_eq!(vdp.cram()[3], 0x07);
}

#[test]
fn game_gear_cram_latch_is_preserved_separately_from_visible_cram() {
    let mut vdp = Vdp::new_with_video_standard_and_color_mode(
        Sega8VideoStandard::Ntsc,
        Mode4ColorMode::GameGear,
    );

    vdp.write_control(0x00);
    vdp.write_control(0xC0);
    vdp.write_data(0x0B);
    assert_eq!(vdp.cram()[0], 0x00);

    assert_eq!(vdp.gg_cram_latch_state(), 0x0B);
    vdp.set_gg_cram_latch_state(0x03);
    vdp.write_data(0x02);

    assert_eq!(vdp.cram()[0], 0x03);
    assert_eq!(vdp.cram()[1], 0x02);
}

#[test]
fn data_reads_use_vdp_read_buffer_and_increment_address() {
    let mut vdp = Vdp::new();
    vdp.write_control(0x00);
    vdp.write_control(0x40);
    vdp.write_data(0x11);
    vdp.write_data(0x22);
    vdp.write_control(0x00);
    vdp.write_control(0x00);

    assert_eq!(vdp.read_data(), 0x11);
    assert_eq!(vdp.read_data(), 0x22);
}

#[test]
fn data_write_cancels_pending_control_latch() {
    let mut vdp = Vdp::new();

    vdp.write_control(0x12);
    vdp.write_data(0xAB);
    vdp.write_control(0x34);
    vdp.write_control(VDP_CONTROL_REGISTER_WRITE_VALUE | VDP_REGISTER_LINE_COUNTER as u8);

    assert_eq!(vdp.registers[VDP_REGISTER_LINE_COUNTER], 0x34);
}

#[test]
fn data_read_cancels_pending_control_latch() {
    let mut vdp = Vdp::new();

    vdp.write_control(0x12);
    let _ = vdp.read_data();
    vdp.write_control(0x56);
    vdp.write_control(VDP_CONTROL_REGISTER_WRITE_VALUE | VDP_REGISTER_LINE_COUNTER as u8);

    assert_eq!(vdp.registers[VDP_REGISTER_LINE_COUNTER], 0x56);
}

#[test]
fn status_read_cancels_pending_control_latch() {
    let mut vdp = Vdp::new();

    vdp.write_control(0x12);
    let _ = vdp.read_status();
    vdp.write_control(0x78);
    vdp.write_control(VDP_CONTROL_REGISTER_WRITE_VALUE | VDP_REGISTER_LINE_COUNTER as u8);

    assert_eq!(vdp.registers[VDP_REGISTER_LINE_COUNTER], 0x78);
}

#[test]
fn status_read_clears_latched_status_bits() {
    let mut vdp = Vdp::new();

    vdp.set_status_bits(VDP_STATUS_VBLANK | VDP_STATUS_SPRITE_COLLISION | 0x03);

    assert_eq!(
        vdp.read_status(),
        VDP_STATUS_VBLANK | VDP_STATUS_SPRITE_COLLISION | 0x03
    );
    assert_eq!(vdp.status(), 0x03);
}

#[test]
fn frame_interrupt_pending_requires_enable_and_vblank_status() {
    let mut vdp = Vdp::new();

    vdp.set_status_bits(VDP_STATUS_VBLANK);
    assert!(!vdp.interrupt_pending());

    vdp.write_control(VDP_REG1_FRAME_IRQ_ENABLE);
    vdp.write_control(VDP_CONTROL_REGISTER_WRITE_VALUE | VDP_REGISTER_MODE_CONTROL_2 as u8);
    assert!(vdp.frame_interrupt_enabled());
    assert!(vdp.interrupt_pending());

    assert_eq!(vdp.read_status() & VDP_STATUS_VBLANK, VDP_STATUS_VBLANK);
    assert!(!vdp.interrupt_pending());
}

#[test]
fn line_interrupt_counter_asserts_irq_and_status_read_clears_it() {
    let mut vdp = Vdp::new();

    vdp.write_control(VDP_REG0_LINE_IRQ_ENABLE);
    vdp.write_control(VDP_CONTROL_REGISTER_WRITE_VALUE | VDP_REGISTER_MODE_CONTROL_1 as u8);
    vdp.write_control(0);
    vdp.write_control(VDP_CONTROL_REGISTER_WRITE_VALUE | VDP_REGISTER_LINE_COUNTER as u8);

    vdp.step_cycles(SMS_SCANLINE_Z80_CYCLES);

    assert!(vdp.line_interrupt_enabled());
    assert!(vdp.line_interrupt_pending());
    assert!(vdp.interrupt_pending());
    assert_eq!(vdp.read_status() & VDP_STATUS_VBLANK, 0);
    assert!(!vdp.line_interrupt_pending());
    assert!(!vdp.interrupt_pending());
}

#[test]
fn line_counter_register_write_waits_until_counter_reload() {
    let mut vdp = Vdp::new();
    vdp.line_counter = 3;
    vdp.registers[VDP_REGISTER_LINE_COUNTER] = 7;

    vdp.step_cycles(SMS_SCANLINE_Z80_CYCLES);
    assert_eq!(vdp.line_counter(), 2);
    vdp.registers[VDP_REGISTER_LINE_COUNTER] = 1;

    vdp.step_cycles(SMS_SCANLINE_Z80_CYCLES);
    assert_eq!(vdp.line_counter(), 1);
    vdp.step_cycles(SMS_SCANLINE_Z80_CYCLES);
    assert_eq!(vdp.line_counter(), 0);
    assert!(!vdp.line_interrupt_pending());

    vdp.step_cycles(SMS_SCANLINE_Z80_CYCLES);
    assert_eq!(vdp.line_counter(), 1);
    assert!(vdp.line_interrupt_pending());
}

#[test]
fn line_interrupt_pending_survives_enable_toggles_until_status_read() {
    let mut vdp = Vdp::new();
    vdp.registers[VDP_REGISTER_LINE_COUNTER] = 0;

    vdp.step_cycles(SMS_SCANLINE_Z80_CYCLES);

    assert!(vdp.line_interrupt_pending());
    assert!(!vdp.interrupt_pending());

    vdp.registers[VDP_REGISTER_MODE_CONTROL_1] = VDP_REG0_LINE_IRQ_ENABLE;
    assert!(vdp.interrupt_pending());
    vdp.registers[VDP_REGISTER_MODE_CONTROL_1] = 0;
    assert!(!vdp.interrupt_pending());
    vdp.registers[VDP_REGISTER_MODE_CONTROL_1] = VDP_REG0_LINE_IRQ_ENABLE;
    assert!(vdp.interrupt_pending());

    let _ = vdp.read_status();
    assert!(!vdp.line_interrupt_pending());
    assert!(!vdp.interrupt_pending());
}

#[test]
fn line_counter_ticks_extra_visible_edge_then_reloads_during_blanking() {
    let mut vdp = Vdp::new();
    vdp.scanline = Sega8DisplayHeight::Lines192.lines();
    vdp.registers[VDP_REGISTER_LINE_COUNTER] = 7;

    vdp.step_cycles(SMS_SCANLINE_Z80_CYCLES);

    assert_eq!(
        vdp.scanline(),
        Sega8DisplayHeight::Lines192.frame_interrupt_scanline()
    );
    assert_eq!(vdp.line_counter(), 7);
    assert!(vdp.line_interrupt_pending());

    let _ = vdp.read_status();
    vdp.line_counter = 3;
    vdp.step_cycles(SMS_SCANLINE_Z80_CYCLES);

    assert_eq!(vdp.line_counter(), 7);
    assert!(!vdp.line_interrupt_pending());
}

#[test]
fn line_counter_uses_extended_mode_visible_edge() {
    let mut vdp = Vdp::new();
    vdp.registers[VDP_REGISTER_MODE_CONTROL_1] = VDP_REG0_MODE4 | VDP_REG0_MODE4_EXTENDED_HEIGHT;
    vdp.registers[VDP_REGISTER_MODE_CONTROL_2] = VDP_REG1_MODE4_224_LINE;
    vdp.scanline = Sega8DisplayHeight::Lines224.lines();
    vdp.registers[VDP_REGISTER_LINE_COUNTER] = 5;

    vdp.step_cycles(SMS_SCANLINE_Z80_CYCLES);

    assert_eq!(
        vdp.scanline(),
        Sega8DisplayHeight::Lines224.frame_interrupt_scanline()
    );
    assert_eq!(vdp.line_counter(), 5);
    assert!(vdp.line_interrupt_pending());

    let _ = vdp.read_status();
    vdp.line_counter = 2;
    vdp.step_cycles(SMS_SCANLINE_Z80_CYCLES);

    assert_eq!(vdp.line_counter(), 5);
    assert!(!vdp.line_interrupt_pending());
}

#[test]
fn mode4_debug_snapshot_decodes_layout_and_register_flags() {
    let mut vdp = Vdp::new();

    vdp.registers[VDP_REGISTER_MODE_CONTROL_1] = VDP_REG0_MODE4
        | VDP_REG0_HORIZONTAL_SCROLL_LOCK
        | VDP_REG0_VERTICAL_SCROLL_LOCK
        | VDP_REG0_HIDE_LEFT_COLUMN
        | VDP_REG0_SPRITE_SHIFT_LEFT;
    vdp.registers[VDP_REGISTER_MODE_CONTROL_2] = VDP_REG1_SPRITE_8X16;
    vdp.registers[MODE4_NAME_TABLE_REGISTER] = MODE4_NAME_TABLE_MASK;
    vdp.registers[MODE4_SPRITE_TABLE_REGISTER] = MODE4_TEST_SPRITE_TABLE_REGISTER;
    vdp.registers[MODE4_SPRITE_PATTERN_TABLE_REGISTER] = MODE4_SPRITE_PATTERN_BASE_SELECT;
    vdp.registers[VDP_REGISTER_HORIZONTAL_SCROLL] = 13;
    vdp.registers[VDP_REGISTER_VERTICAL_SCROLL] = 21;
    vdp.registers[VDP_REGISTER_BACKDROP_COLOR] = 5;

    let snapshot = vdp.mode4_debug_snapshot();

    assert!(snapshot.enabled);
    assert_eq!(
        snapshot.name_table_base,
        usize::from(MODE4_NAME_TABLE_MASK) << MODE4_NAME_TABLE_SHIFT
    );
    assert_eq!(
        snapshot.sprite_table_base,
        usize::from(MODE4_TEST_SPRITE_TABLE_REGISTER & MODE4_SPRITE_TABLE_MASK)
            << MODE4_SPRITE_TABLE_SHIFT
    );
    assert_eq!(snapshot.sprite_pattern_base, MODE4_SPRITE_PATTERN_BASE_HIGH);
    assert_eq!(snapshot.horizontal_scroll, 13);
    assert_eq!(snapshot.vertical_scroll, 21);
    assert_eq!(
        snapshot.backdrop_color_index,
        MODE4_PALETTE_COLOR_OFFSET + 5
    );
    assert_eq!(snapshot.sprite_height, SMS_TILE_SIZE * 2);
    assert_eq!(snapshot.sprite_width, SMS_TILE_SIZE);
    assert_eq!(snapshot.max_sprites_per_line, MODE4_MAX_SPRITES_PER_LINE);
    assert!(snapshot.horizontal_scroll_lock);
    assert!(snapshot.vertical_scroll_lock);
    assert!(snapshot.hide_left_column);
    assert!(snapshot.sprite_shift_left);
    assert!(!snapshot.sprite_magnified);
}

#[test]
fn mode4_extended_height_uses_extended_name_table_base_and_rows() {
    let mut vdp = Vdp::new();
    let mut framebuffer = vec![0; SMS_TILE_SIZE * SMS_TILE_SIZE * RGBA_CHANNELS];

    vdp.registers[VDP_REGISTER_MODE_CONTROL_1] = VDP_REG0_MODE4 | VDP_REG0_MODE4_EXTENDED_HEIGHT;
    vdp.registers[VDP_REGISTER_MODE_CONTROL_2] = VDP_REG1_DISPLAY_ENABLE | VDP_REG1_MODE4_224_LINE;
    set_tile_row(&mut vdp, 1, 0, [MODE4_PATTERN_LEFT_PIXEL_MASK, 0, 0, 0]);
    set_name_entry(&mut vdp, 0, 31, 1);
    vdp.cram[1] = SMS_RED;

    assert_eq!(vdp.mode4_display_height(), Sega8DisplayHeight::Lines224);
    assert_eq!(vdp.mode4_name_table_base(), 0x3700);

    vdp.render_mode4_background_rgba(
        &mut framebuffer,
        render_area(SMS_TILE_SIZE, SMS_TILE_SIZE, 0, 31 * SMS_TILE_SIZE),
    );

    assert_eq!(&framebuffer[0..RGBA_CHANNELS], &SMS_RED_RGBA);
}

#[test]
fn mode4_extended_height_bits_require_m2_and_do_not_stack() {
    let mut vdp = Vdp::new();
    vdp.registers[VDP_REGISTER_MODE_CONTROL_1] = VDP_REG0_MODE4;
    vdp.registers[VDP_REGISTER_MODE_CONTROL_2] = VDP_REG1_DISPLAY_ENABLE | VDP_REG1_MODE4_224_LINE;
    assert_eq!(vdp.mode4_display_height(), Sega8DisplayHeight::Lines192);

    vdp.registers[VDP_REGISTER_MODE_CONTROL_1] = VDP_REG0_MODE4 | VDP_REG0_MODE4_EXTENDED_HEIGHT;
    assert_eq!(vdp.mode4_display_height(), Sega8DisplayHeight::Lines224);

    vdp.registers[VDP_REGISTER_MODE_CONTROL_2] = VDP_REG1_DISPLAY_ENABLE | VDP_REG1_MODE4_240_LINE;
    assert_eq!(vdp.mode4_display_height(), Sega8DisplayHeight::Lines240);

    vdp.registers[VDP_REGISTER_MODE_CONTROL_2] =
        VDP_REG1_DISPLAY_ENABLE | VDP_REG1_MODE4_224_LINE | VDP_REG1_MODE4_240_LINE;
    assert_eq!(vdp.mode4_display_height(), Sega8DisplayHeight::Lines192);
}

#[test]
fn mode4_background_renderer_decodes_tile_pixels_and_sms_cram() {
    let mut vdp = Vdp::new();
    let mut framebuffer = vec![0; SMS_TILE_SIZE * SMS_TILE_SIZE * RGBA_CHANNELS];

    set_tile_row(&mut vdp, 1, 0, [0x80, 0x80, 0x00, 0x00]);
    set_name_entry(&mut vdp, 0, 0, 1);
    vdp.cram[0] = 0x00;
    vdp.cram[3] = 0x03;

    vdp.render_mode4_background_rgba(
        &mut framebuffer,
        render_area(SMS_TILE_SIZE, SMS_TILE_SIZE, 0, 0),
    );

    assert_eq!(&framebuffer[0..RGBA_CHANNELS], &[0xFF, 0x00, 0x00, 0xFF]);
    assert_eq!(
        &framebuffer[RGBA_CHANNELS..RGBA_CHANNELS * 2],
        &[0x00, 0x00, 0x00, 0xFF]
    );
}

#[test]
fn mode4_background_renderer_honors_flips_and_palette_bit() {
    let mut vdp = Vdp::new();
    let mut framebuffer = vec![0; SMS_TILE_SIZE * SMS_TILE_SIZE * RGBA_CHANNELS];

    set_tile_row(&mut vdp, 2, 7, [0x01, 0x00, 0x00, 0x00]);
    set_name_entry(
        &mut vdp,
        0,
        0,
        2 | MODE4_TILE_HFLIP | MODE4_TILE_VFLIP | MODE4_TILE_PALETTE,
    );
    vdp.cram[17] = 0x0C;

    vdp.render_mode4_background_rgba(
        &mut framebuffer,
        render_area(SMS_TILE_SIZE, SMS_TILE_SIZE, 0, 0),
    );

    assert_eq!(&framebuffer[0..RGBA_CHANNELS], &[0x00, 0xFF, 0x00, 0xFF]);
}

#[test]
fn mode4_background_renderer_applies_global_scroll_registers() {
    let mut vdp = Vdp::new();
    let mut framebuffer = vec![0; SMS_TILE_SIZE * SMS_TILE_SIZE * RGBA_CHANNELS];

    set_tile_row(&mut vdp, 3, 0, [0x80, 0x80, 0x00, 0x00]);
    set_name_entry(&mut vdp, 31, 0, 3);
    vdp.registers[VDP_REGISTER_HORIZONTAL_SCROLL] = 8;
    vdp.cram[3] = 0x03;

    vdp.render_mode4_background_rgba(
        &mut framebuffer,
        render_area(SMS_TILE_SIZE, SMS_TILE_SIZE, 0, 0),
    );

    assert_eq!(&framebuffer[0..RGBA_CHANNELS], &[0xFF, 0x00, 0x00, 0xFF]);
}

#[test]
fn mode4_background_renderer_honors_top_row_horizontal_scroll_lock() {
    let mut vdp = Vdp::new();
    let mut framebuffer = vec![0; SMS_TILE_SIZE * SMS_TILE_SIZE * RGBA_CHANNELS];

    set_tile_row(&mut vdp, 1, 0, [0x80, 0x00, 0x00, 0x00]);
    set_tile_row(&mut vdp, 2, 0, [0x80, 0x80, 0x00, 0x00]);
    set_name_entry(&mut vdp, 0, 0, 1);
    set_name_entry(&mut vdp, 31, 0, 2);
    vdp.registers[VDP_REGISTER_MODE_CONTROL_1] = VDP_REG0_HORIZONTAL_SCROLL_LOCK;
    vdp.registers[VDP_REGISTER_HORIZONTAL_SCROLL] = 8;
    vdp.cram[1] = 0x03;
    vdp.cram[3] = 0x0C;

    vdp.render_mode4_background_rgba(
        &mut framebuffer,
        render_area(SMS_TILE_SIZE, SMS_TILE_SIZE, 0, 0),
    );

    assert_eq!(&framebuffer[0..RGBA_CHANNELS], &[0xFF, 0x00, 0x00, 0xFF]);
}

#[test]
fn mode4_background_renderer_honors_right_column_vertical_scroll_lock() {
    let mut vdp = Vdp::new();
    let mut framebuffer = vec![0; SMS_TILE_SIZE * SMS_TILE_SIZE * RGBA_CHANNELS];

    set_tile_row(&mut vdp, 1, 0, [0x80, 0x00, 0x00, 0x00]);
    set_tile_row(&mut vdp, 2, 0, [0x80, 0x80, 0x00, 0x00]);
    set_name_entry(&mut vdp, 24, 0, 1);
    set_name_entry(&mut vdp, 24, 1, 2);
    vdp.registers[VDP_REGISTER_MODE_CONTROL_1] = VDP_REG0_VERTICAL_SCROLL_LOCK;
    vdp.registers[VDP_REGISTER_VERTICAL_SCROLL] = 8;
    vdp.cram[1] = 0x03;
    vdp.cram[3] = 0x0C;

    vdp.render_mode4_background_rgba(
        &mut framebuffer,
        render_area(SMS_TILE_SIZE, SMS_TILE_SIZE, 192, 0),
    );

    assert_eq!(&framebuffer[0..RGBA_CHANNELS], &[0xFF, 0x00, 0x00, 0xFF]);
}

#[test]
fn mode4_frame_renderer_decodes_game_gear_cram() {
    let mut vdp = Vdp::new();
    let mut framebuffer = vec![0; SMS_TILE_SIZE * SMS_TILE_SIZE * RGBA_CHANNELS];

    vdp.registers[VDP_REGISTER_MODE_CONTROL_2] = VDP_REG1_DISPLAY_ENABLE;
    set_tile_row(&mut vdp, 1, 0, [0x80, 0x80, 0x00, 0x00]);
    set_name_entry(&mut vdp, 0, 0, 1);
    vdp.vram[0] = MODE4_SPRITE_TERMINATOR_Y;
    vdp.cram[6] = 0x0F;
    vdp.cram[7] = 0x00;

    vdp.render_mode4_frame_rgba(
        &mut framebuffer,
        render_area(SMS_TILE_SIZE, SMS_TILE_SIZE, 0, 0),
        Mode4ColorMode::GameGear,
    );

    assert_eq!(&framebuffer[0..RGBA_CHANNELS], &[0xFF, 0x00, 0x00, 0xFF]);
}

#[test]
fn mode4_frame_renderer_draws_nonzero_sprites_over_background() {
    let mut vdp = Vdp::new();
    let mut framebuffer = vec![0; SMS_TILE_SIZE * SMS_TILE_SIZE * RGBA_CHANNELS];

    vdp.registers[VDP_REGISTER_MODE_CONTROL_2] = VDP_REG1_DISPLAY_ENABLE;
    set_tile_row(&mut vdp, 4, 0, [0x80, 0x00, 0x00, 0x00]);
    vdp.registers[MODE4_SPRITE_TABLE_REGISTER] = 0x7E;
    let sprite_table = vdp.mode4_sprite_table_base();
    vdp.vram[sprite_table] = 0xFF;
    vdp.vram[sprite_table + 1] = MODE4_SPRITE_TERMINATOR_Y;
    vdp.vram[sprite_table + MODE4_SPRITE_X_TILE_TABLE_OFFSET] = 0;
    vdp.vram[sprite_table + MODE4_SPRITE_X_TILE_TABLE_OFFSET + 1] = 4;
    vdp.cram[17] = 0x03;

    vdp.render_mode4_frame_rgba(
        &mut framebuffer,
        render_area(SMS_TILE_SIZE, SMS_TILE_SIZE, 0, 0),
        Mode4ColorMode::Sms,
    );

    assert_eq!(&framebuffer[0..RGBA_CHANNELS], &[0xFF, 0x00, 0x00, 0xFF]);
}

#[test]
fn mode4_frame_renderer_uses_sprite_pattern_table_base_register() {
    let mut vdp = Vdp::new();
    let mut framebuffer = vec![0; SMS_TILE_SIZE * SMS_TILE_SIZE * RGBA_CHANNELS];

    vdp.registers[VDP_REGISTER_MODE_CONTROL_2] = VDP_REG1_DISPLAY_ENABLE;
    vdp.registers[MODE4_SPRITE_PATTERN_TABLE_REGISTER] = MODE4_SPRITE_PATTERN_BASE_SELECT;
    set_tile_row_at(
        &mut vdp,
        MODE4_SPRITE_PATTERN_BASE_HIGH,
        4,
        0,
        [MODE4_PATTERN_LEFT_PIXEL_MASK, 0, 0, 0],
    );
    let sprite_table = use_mode4_test_sprite_table(&mut vdp);
    set_mode4_sprite(&mut vdp, sprite_table, 0, MODE4_TOP_SCANLINE_SPRITE_Y, 0, 4);
    terminate_mode4_sprites(&mut vdp, sprite_table, 1);
    vdp.cram[MODE4_PALETTE_COLOR_OFFSET + 1] = SMS_RED;

    vdp.render_mode4_frame_rgba(
        &mut framebuffer,
        render_area(SMS_TILE_SIZE, SMS_TILE_SIZE, 0, 0),
        Mode4ColorMode::Sms,
    );

    assert_eq!(&framebuffer[0..RGBA_CHANNELS], &SMS_RED_RGBA);
}

#[test]
fn mode4_frame_renderer_prioritizes_lower_sprite_indices() {
    let mut vdp = Vdp::new();
    let mut framebuffer = vec![0; SMS_TILE_SIZE * SMS_TILE_SIZE * RGBA_CHANNELS];

    vdp.registers[VDP_REGISTER_MODE_CONTROL_2] = VDP_REG1_DISPLAY_ENABLE;
    set_tile_row(&mut vdp, 4, 0, [MODE4_PATTERN_LEFT_PIXEL_MASK, 0, 0, 0]);
    set_tile_row(&mut vdp, 5, 0, [0, MODE4_PATTERN_LEFT_PIXEL_MASK, 0, 0]);
    let sprite_table = use_mode4_test_sprite_table(&mut vdp);
    set_mode4_sprite(&mut vdp, sprite_table, 0, MODE4_TOP_SCANLINE_SPRITE_Y, 0, 4);
    set_mode4_sprite(&mut vdp, sprite_table, 1, MODE4_TOP_SCANLINE_SPRITE_Y, 0, 5);
    terminate_mode4_sprites(&mut vdp, sprite_table, 2);
    vdp.cram[MODE4_PALETTE_COLOR_OFFSET + 1] = SMS_RED;
    vdp.cram[MODE4_PALETTE_COLOR_OFFSET + 2] = SMS_GREEN;

    vdp.render_mode4_frame_rgba(
        &mut framebuffer,
        render_area(SMS_TILE_SIZE, SMS_TILE_SIZE, 0, 0),
        Mode4ColorMode::Sms,
    );

    assert_eq!(&framebuffer[0..RGBA_CHANNELS], &SMS_RED_RGBA);
}

#[test]
fn mode4_frame_renderer_honors_sprite_magnification() {
    let mut vdp = Vdp::new();
    let mut framebuffer = vec![0; SMS_TILE_SIZE * SMS_TILE_SIZE * RGBA_CHANNELS];

    vdp.registers[VDP_REGISTER_MODE_CONTROL_2] = VDP_REG1_DISPLAY_ENABLE | VDP_REG1_SPRITE_MAGNIFY;
    set_tile_row(&mut vdp, 4, 0, [MODE4_PATTERN_LEFT_PIXEL_MASK, 0, 0, 0]);
    let sprite_table = use_mode4_test_sprite_table(&mut vdp);
    set_mode4_sprite(&mut vdp, sprite_table, 0, MODE4_TOP_SCANLINE_SPRITE_Y, 0, 4);
    terminate_mode4_sprites(&mut vdp, sprite_table, 1);
    vdp.cram[MODE4_PALETTE_COLOR_OFFSET + 1] = SMS_RED;

    vdp.render_mode4_frame_rgba(
        &mut framebuffer,
        render_area(SMS_TILE_SIZE, SMS_TILE_SIZE, 0, 0),
        Mode4ColorMode::Sms,
    );

    let pixel = |x: usize, y: usize| {
        let offset = (y * SMS_TILE_SIZE + x) * RGBA_CHANNELS;
        &framebuffer[offset..offset + RGBA_CHANNELS]
    };
    assert_eq!(pixel(0, 0), &SMS_RED_RGBA);
    assert_eq!(pixel(1, 0), &SMS_RED_RGBA);
    assert_eq!(pixel(0, 1), &SMS_RED_RGBA);
    assert_eq!(pixel(1, 1), &SMS_RED_RGBA);
    assert_eq!(pixel(2, 0), &[0x00, 0x00, 0x00, 0xFF]);
}

#[test]
fn mode4_8x16_sprite_index_mask_is_independent_of_magnification() {
    let mut vdp = Vdp::new();
    let mut framebuffer = vec![0; SMS_TILE_SIZE * SMS_TILE_SIZE * RGBA_CHANNELS];

    vdp.registers[VDP_REGISTER_MODE_CONTROL_2] =
        VDP_REG1_DISPLAY_ENABLE | VDP_REG1_SPRITE_8X16 | VDP_REG1_SPRITE_MAGNIFY;
    set_tile_row(&mut vdp, 6, 0, [MODE4_PATTERN_LEFT_PIXEL_MASK, 0, 0, 0]);
    let sprite_table = use_mode4_test_sprite_table(&mut vdp);
    set_mode4_sprite(&mut vdp, sprite_table, 0, MODE4_TOP_SCANLINE_SPRITE_Y, 0, 7);
    terminate_mode4_sprites(&mut vdp, sprite_table, 1);
    vdp.cram[MODE4_PALETTE_COLOR_OFFSET + 1] = SMS_RED;

    vdp.render_mode4_frame_rgba(
        &mut framebuffer,
        render_area(SMS_TILE_SIZE, SMS_TILE_SIZE, 0, 0),
        Mode4ColorMode::Sms,
    );

    assert_eq!(&framebuffer[0..RGBA_CHANNELS], &SMS_RED_RGBA);
    assert_eq!(vdp.mode4_debug_snapshot().sprite_height, SMS_TILE_SIZE * 4);
    assert_eq!(vdp.mode4_debug_snapshot().sprite_width, SMS_TILE_SIZE * 2);
    assert!(vdp.mode4_debug_snapshot().sprite_magnified);
}

#[test]
fn mode4_frame_renderer_honors_priority_background_pixels_over_sprites() {
    let mut vdp = Vdp::new();
    let mut framebuffer = vec![0; SMS_TILE_SIZE * SMS_TILE_SIZE * RGBA_CHANNELS];

    vdp.registers[VDP_REGISTER_MODE_CONTROL_2] = VDP_REG1_DISPLAY_ENABLE;
    set_tile_row(&mut vdp, 1, 0, [MODE4_PATTERN_LEFT_PIXEL_MASK, 0, 0, 0]);
    set_name_entry(&mut vdp, 0, 0, 1 | MODE4_TILE_PRIORITY);
    set_tile_row(&mut vdp, 4, 0, [MODE4_PATTERN_LEFT_PIXEL_MASK, 0, 0, 0]);
    let sprite_table = use_mode4_test_sprite_table(&mut vdp);
    set_mode4_sprite(&mut vdp, sprite_table, 0, MODE4_TOP_SCANLINE_SPRITE_Y, 0, 4);
    terminate_mode4_sprites(&mut vdp, sprite_table, 1);
    vdp.cram[1] = SMS_GREEN;
    vdp.cram[MODE4_PALETTE_COLOR_OFFSET + 1] = SMS_RED;

    vdp.render_mode4_frame_rgba(
        &mut framebuffer,
        render_area(SMS_TILE_SIZE, SMS_TILE_SIZE, 0, 0),
        Mode4ColorMode::Sms,
    );

    assert_eq!(&framebuffer[0..RGBA_CHANNELS], &SMS_GREEN_RGBA);
}

#[test]
fn mode4_frame_renderer_draws_sprites_over_transparent_priority_background_pixels() {
    let mut vdp = Vdp::new();
    let mut framebuffer = vec![0; SMS_TILE_SIZE * SMS_TILE_SIZE * RGBA_CHANNELS];

    vdp.registers[VDP_REGISTER_MODE_CONTROL_2] = VDP_REG1_DISPLAY_ENABLE;
    set_name_entry(&mut vdp, 0, 0, 1 | MODE4_TILE_PALETTE | MODE4_TILE_PRIORITY);
    set_tile_row(&mut vdp, 4, 0, [MODE4_PATTERN_LEFT_PIXEL_MASK, 0, 0, 0]);
    let sprite_table = use_mode4_test_sprite_table(&mut vdp);
    set_mode4_sprite(&mut vdp, sprite_table, 0, MODE4_TOP_SCANLINE_SPRITE_Y, 0, 4);
    terminate_mode4_sprites(&mut vdp, sprite_table, 1);
    vdp.cram[MODE4_PALETTE_COLOR_OFFSET] = SMS_GREEN;
    vdp.cram[MODE4_PALETTE_COLOR_OFFSET + 1] = SMS_RED;

    vdp.render_mode4_frame_rgba(
        &mut framebuffer,
        render_area(SMS_TILE_SIZE, SMS_TILE_SIZE, 0, 0),
        Mode4ColorMode::Sms,
    );

    assert_eq!(&framebuffer[0..RGBA_CHANNELS], &SMS_RED_RGBA);
}

#[test]
fn mode4_frame_renderer_blanks_to_backdrop_when_display_disabled() {
    let mut vdp = Vdp::new();
    let mut framebuffer = vec![0; SMS_TILE_SIZE * SMS_TILE_SIZE * RGBA_CHANNELS];

    set_tile_row(&mut vdp, 1, 0, [0x80, 0x80, 0x00, 0x00]);
    set_name_entry(&mut vdp, 0, 0, 1);
    vdp.registers[7] = 1;
    vdp.cram[17] = 0x0C;

    vdp.render_mode4_frame_rgba(
        &mut framebuffer,
        render_area(SMS_TILE_SIZE, SMS_TILE_SIZE, 0, 0),
        Mode4ColorMode::Sms,
    );

    assert_eq!(&framebuffer[0..RGBA_CHANNELS], &[0x00, 0xFF, 0x00, 0xFF]);
}

#[test]
fn mode4_presented_renderer_uses_scanline_display_history() {
    let mut vdp = Vdp::new();
    let mut framebuffer = vec![0; SMS_TILE_SIZE * 2 * RGBA_CHANNELS];

    set_tile_row(&mut vdp, 1, 0, [0xFF, 0x00, 0x00, 0x00]);
    set_tile_row(&mut vdp, 1, 1, [0xFF, 0x00, 0x00, 0x00]);
    set_name_entry(&mut vdp, 0, 0, 1);
    vdp.registers[7] = 2;
    vdp.cram[1] = SMS_RED;
    vdp.cram[MODE4_PALETTE_COLOR_OFFSET + 2] = SMS_GREEN;
    vdp.scanline_display_enabled[0] = true;
    vdp.scanline_display_enabled[1] = false;

    vdp.render_mode4_presented_frame_rgba(
        &mut framebuffer,
        render_area(SMS_TILE_SIZE, 2, 0, 0),
        Mode4ColorMode::Sms,
    );

    assert_eq!(&framebuffer[0..RGBA_CHANNELS], &SMS_RED_RGBA);
    let row_1_pixel = SMS_TILE_SIZE * RGBA_CHANNELS;
    assert_eq!(
        &framebuffer[row_1_pixel..row_1_pixel + RGBA_CHANNELS],
        &SMS_GREEN_RGBA
    );
}

#[test]
fn mode4_presented_renderer_uses_latched_scanline_pixels() {
    let mut vdp = Vdp::new();
    let mut framebuffer = vec![0; 96 * RGBA_CHANNELS];

    vdp.registers[VDP_REGISTER_MODE_CONTROL_1] = VDP_REG0_MODE4;
    vdp.registers[VDP_REGISTER_MODE_CONTROL_2] = VDP_REG1_DISPLAY_ENABLE;
    let sprite_table = use_mode4_test_sprite_table(&mut vdp);
    set_tile_row(&mut vdp, 4, 0, [MODE4_PATTERN_LEFT_PIXEL_MASK, 0, 0, 0]);
    set_mode4_sprite(&mut vdp, sprite_table, 0, MODE4_TOP_SCANLINE_SPRITE_Y, 0, 4);
    terminate_mode4_sprites(&mut vdp, sprite_table, 1);
    vdp.cram[MODE4_PALETTE_COLOR_OFFSET + 1] = SMS_RED;
    vdp.scanline_start_registers = vdp.registers;

    vdp.step_cycles(SMS_SCANLINE_Z80_CYCLES);
    set_mode4_sprite(
        &mut vdp,
        sprite_table,
        0,
        MODE4_TOP_SCANLINE_SPRITE_Y,
        80,
        4,
    );
    vdp.cram[MODE4_PALETTE_COLOR_OFFSET + 1] = SMS_GREEN;

    vdp.render_mode4_presented_frame_rgba(
        &mut framebuffer,
        render_area(96, 1, 0, 0),
        Mode4ColorMode::Sms,
    );

    assert_eq!(&framebuffer[0..RGBA_CHANNELS], &SMS_RED_RGBA);
    let moved_sprite_pixel = 80 * RGBA_CHANNELS;
    assert_eq!(
        &framebuffer[moved_sprite_pixel..moved_sprite_pixel + RGBA_CHANNELS],
        &[0x00, 0x00, 0x00, 0xFF]
    );
}

#[test]
fn mode4_presented_scanline_uses_registers_from_scanline_start() {
    let mut vdp = Vdp::new();
    let mut framebuffer = vec![0; SMS_TILE_SIZE * 2 * RGBA_CHANNELS];

    vdp.registers[VDP_REGISTER_MODE_CONTROL_1] = VDP_REG0_MODE4;
    vdp.registers[VDP_REGISTER_MODE_CONTROL_2] = VDP_REG1_DISPLAY_ENABLE;
    set_tile_row(&mut vdp, 1, 0, [0xFF, 0x00, 0x00, 0x00]);
    set_tile_row(&mut vdp, 2, 0, [0x00, 0xFF, 0x00, 0x00]);
    set_name_entry(&mut vdp, 0, 0, 1);
    set_name_entry(&mut vdp, 1, 0, 2);
    vdp.cram[1] = SMS_RED;
    vdp.cram[2] = SMS_GREEN;
    vdp.scanline_start_registers = vdp.registers;

    vdp.registers[VDP_REGISTER_HORIZONTAL_SCROLL] = SMS_TILE_SIZE as u8;
    vdp.step_cycles(SMS_SCANLINE_Z80_CYCLES);

    vdp.render_mode4_presented_frame_rgba(
        &mut framebuffer,
        render_area(SMS_TILE_SIZE * 2, 1, 0, 0),
        Mode4ColorMode::Sms,
    );

    assert_eq!(&framebuffer[0..RGBA_CHANNELS], &SMS_RED_RGBA);
    let second_tile_pixel = SMS_TILE_SIZE * RGBA_CHANNELS;
    assert_eq!(
        &framebuffer[second_tile_pixel..second_tile_pixel + RGBA_CHANNELS],
        &SMS_GREEN_RGBA
    );
}

#[test]
fn mode4_frame_renderer_masks_left_column_to_backdrop() {
    let mut vdp = Vdp::new();
    let mut framebuffer = vec![0; SMS_TILE_SIZE * 2 * SMS_TILE_SIZE * RGBA_CHANNELS];

    vdp.registers[VDP_REGISTER_MODE_CONTROL_1] = VDP_REG0_HIDE_LEFT_COLUMN;
    vdp.registers[VDP_REGISTER_MODE_CONTROL_2] = VDP_REG1_DISPLAY_ENABLE;
    vdp.registers[MODE4_SPRITE_TABLE_REGISTER] = 0x7E;
    let sprite_table = vdp.mode4_sprite_table_base();
    vdp.vram[sprite_table] = MODE4_SPRITE_TERMINATOR_Y;
    set_tile_row(&mut vdp, 1, 0, [0xFF, 0x00, 0x00, 0x00]);
    set_name_entry(&mut vdp, 0, 0, 1);
    set_name_entry(&mut vdp, 1, 0, 1);
    vdp.registers[7] = 2;
    vdp.cram[1] = 0x03;
    vdp.cram[18] = 0x0C;

    vdp.render_mode4_frame_rgba(
        &mut framebuffer,
        render_area(SMS_TILE_SIZE * 2, SMS_TILE_SIZE, 0, 0),
        Mode4ColorMode::Sms,
    );

    assert_eq!(&framebuffer[0..RGBA_CHANNELS], &[0x00, 0xFF, 0x00, 0xFF]);
    let unmasked = SMS_TILE_SIZE * RGBA_CHANNELS;
    assert_eq!(
        &framebuffer[unmasked..unmasked + RGBA_CHANNELS],
        &[0xFF, 0x00, 0x00, 0xFF]
    );
}

#[test]
fn mode4_sprite_status_latches_collision_and_overflow_on_scanline() {
    let mut vdp = Vdp::new();

    vdp.registers[VDP_REGISTER_MODE_CONTROL_1] = VDP_REG0_MODE4;
    vdp.registers[VDP_REGISTER_MODE_CONTROL_2] = VDP_REG1_DISPLAY_ENABLE;
    vdp.registers[MODE4_SPRITE_TABLE_REGISTER] = 0x7E;
    let sprite_table = vdp.mode4_sprite_table_base();
    set_tile_row(&mut vdp, 4, 0, [0x80, 0x00, 0x00, 0x00]);
    for sprite in 0..9usize {
        vdp.vram[sprite_table + sprite] = 0xFF;
        let xt = sprite_table + MODE4_SPRITE_X_TILE_TABLE_OFFSET + sprite * 2;
        vdp.vram[xt] = 0;
        vdp.vram[xt + 1] = 4;
    }
    vdp.vram[sprite_table + 9] = MODE4_SPRITE_TERMINATOR_Y;

    vdp.step_cycles(SMS_SCANLINE_Z80_CYCLES);

    assert_eq!(
        vdp.status() & VDP_STATUS_SPRITE_COLLISION,
        VDP_STATUS_SPRITE_COLLISION
    );
    assert_eq!(
        vdp.status() & VDP_STATUS_SPRITE_OVERFLOW,
        VDP_STATUS_SPRITE_OVERFLOW
    );
}

#[test]
fn mode4_sprite_status_uses_sprite_pattern_table_base_register() {
    let mut vdp = Vdp::new();

    vdp.registers[VDP_REGISTER_MODE_CONTROL_1] = VDP_REG0_MODE4;
    vdp.registers[VDP_REGISTER_MODE_CONTROL_2] = VDP_REG1_DISPLAY_ENABLE;
    vdp.registers[MODE4_SPRITE_PATTERN_TABLE_REGISTER] = MODE4_SPRITE_PATTERN_BASE_SELECT;
    let sprite_table = use_mode4_test_sprite_table(&mut vdp);
    set_tile_row_at(
        &mut vdp,
        MODE4_SPRITE_PATTERN_BASE_HIGH,
        4,
        0,
        [MODE4_PATTERN_LEFT_PIXEL_MASK, 0, 0, 0],
    );
    set_mode4_sprite(&mut vdp, sprite_table, 0, MODE4_TOP_SCANLINE_SPRITE_Y, 0, 4);
    set_mode4_sprite(&mut vdp, sprite_table, 1, MODE4_TOP_SCANLINE_SPRITE_Y, 0, 4);
    terminate_mode4_sprites(&mut vdp, sprite_table, 2);

    vdp.step_cycles(SMS_SCANLINE_Z80_CYCLES);

    assert_eq!(
        vdp.status() & VDP_STATUS_SPRITE_COLLISION,
        VDP_STATUS_SPRITE_COLLISION
    );
}

#[test]
fn mode4_frame_renderer_limits_to_eight_sprites_per_line() {
    let mut vdp = Vdp::new();
    let mut framebuffer = vec![0; SMS_TILE_SIZE * SMS_TILE_SIZE * RGBA_CHANNELS];

    vdp.registers[VDP_REGISTER_MODE_CONTROL_2] = VDP_REG1_DISPLAY_ENABLE;
    vdp.registers[MODE4_SPRITE_TABLE_REGISTER] = 0x7E;
    let sprite_table = vdp.mode4_sprite_table_base();
    set_tile_row(&mut vdp, 1, 0, [0x80, 0x00, 0x00, 0x00]);
    vdp.cram[17] = 0x03;
    for sprite in 0..8usize {
        vdp.vram[sprite_table + sprite] = 0xFF;
        let xt = sprite_table + MODE4_SPRITE_X_TILE_TABLE_OFFSET + sprite * 2;
        vdp.vram[xt] = 240;
        vdp.vram[xt + 1] = 0;
    }
    vdp.vram[sprite_table + 8] = 0xFF;
    let ninth_xt = sprite_table + MODE4_SPRITE_X_TILE_TABLE_OFFSET + 8 * 2;
    vdp.vram[ninth_xt] = 0;
    vdp.vram[ninth_xt + 1] = 1;
    vdp.vram[sprite_table + 9] = MODE4_SPRITE_TERMINATOR_Y;

    vdp.render_mode4_frame_rgba(
        &mut framebuffer,
        render_area(SMS_TILE_SIZE, SMS_TILE_SIZE, 0, 0),
        Mode4ColorMode::Sms,
    );

    assert_eq!(&framebuffer[0..RGBA_CHANNELS], &[0x00, 0x00, 0x00, 0xFF]);
}

#[test]
fn tms9918_graphics_i_renderer_uses_pattern_name_and_color_tables() {
    let mut vdp = Vdp::new();
    let mut framebuffer = vec![0; SMS_TILE_SIZE * SMS_TILE_SIZE * RGBA_CHANNELS];

    vdp.registers[VDP_REGISTER_MODE_CONTROL_2] = VDP_REG1_DISPLAY_ENABLE;
    vdp.registers[TMS_REGISTER_NAME_TABLE] = 0x0E;
    vdp.registers[TMS_REGISTER_COLOR_TABLE] = 0x20;
    vdp.registers[TMS_REGISTER_PATTERN_TABLE] = 0x00;
    vdp.registers[TMS_REGISTER_TEXT_BACKDROP] = 0x01;
    set_tms_name(&mut vdp, 0, 0, 1);
    set_tms_pattern_row(&mut vdp, 0, 1, 0, 0x80);
    vdp.vram[vdp.tms_color_table_base()] = 0x60;

    vdp.render_tms9918_frame_rgba(&mut framebuffer, SMS_TILE_SIZE, SMS_TILE_SIZE);

    assert_eq!(&framebuffer[0..RGBA_CHANNELS], &[0xD4, 0x52, 0x4D, 0xFF]);
    assert_eq!(
        &framebuffer[RGBA_CHANNELS..RGBA_CHANNELS * 2],
        &[0x00, 0x00, 0x00, 0xFF]
    );
}

#[test]
fn tms9918_graphics_ii_renderer_uses_sectioned_pattern_and_color_tables() {
    let mut vdp = Vdp::new();
    let mut framebuffer = vec![0; SMS_SCREEN_W * SMS_SCREEN_H * RGBA_CHANNELS];

    vdp.registers[VDP_REGISTER_MODE_CONTROL_1] = TMS_REG0_MODE_GRAPHICS_II;
    vdp.registers[VDP_REGISTER_MODE_CONTROL_2] = VDP_REG1_DISPLAY_ENABLE;
    vdp.registers[TMS_REGISTER_NAME_TABLE] = 0x0E;
    vdp.registers[TMS_REGISTER_COLOR_TABLE] = 0x80;
    vdp.registers[TMS_REGISTER_PATTERN_TABLE] = 0x00;
    vdp.registers[TMS_REGISTER_TEXT_BACKDROP] = 0x01;
    set_tms_name(&mut vdp, 0, 8, 2);
    set_tms_pattern_row(&mut vdp, TMS_TABLE_SECTION_BYTES, 2, 0, 0x80);
    set_tms_color_row(&mut vdp, 0x2000 + TMS_TABLE_SECTION_BYTES, 2, 0, 0x50);

    vdp.render_tms9918_frame_rgba(&mut framebuffer, SMS_SCREEN_W, SMS_SCREEN_H);

    assert_eq!(&framebuffer[0..RGBA_CHANNELS], &[0x00, 0x00, 0x00, 0xFF]);
    let section_1_pixel = (SMS_TILE_SIZE * 8 * SMS_SCREEN_W) * RGBA_CHANNELS;
    assert_eq!(
        &framebuffer[section_1_pixel..section_1_pixel + RGBA_CHANNELS],
        &[0x7D, 0x76, 0xFC, 0xFF]
    );
}

#[test]
fn tms9918_text_renderer_draws_six_pixel_wide_characters() {
    let mut vdp = Vdp::new();
    let mut framebuffer = vec![0; SMS_SCREEN_W * SMS_TILE_SIZE * RGBA_CHANNELS];

    vdp.registers[VDP_REGISTER_MODE_CONTROL_2] = VDP_REG1_DISPLAY_ENABLE | TMS_REG1_MODE_TEXT;
    vdp.registers[TMS_REGISTER_NAME_TABLE] = 0x0E;
    vdp.registers[TMS_REGISTER_PATTERN_TABLE] = 0x00;
    vdp.registers[TMS_REGISTER_TEXT_BACKDROP] = 0xF1;
    vdp.vram[vdp.tms_name_table_base()] = 3;
    set_tms_pattern_row(&mut vdp, 0, 3, 0, 0x80);

    vdp.render_tms9918_frame_rgba(&mut framebuffer, SMS_SCREEN_W, SMS_TILE_SIZE);

    let text_pixel = TMS_TEXT_LEFT_MARGIN * RGBA_CHANNELS;
    assert_eq!(
        &framebuffer[text_pixel..text_pixel + RGBA_CHANNELS],
        &[0xFF, 0xFF, 0xFF, 0xFF]
    );
    assert_eq!(&framebuffer[0..RGBA_CHANNELS], &[0x00, 0x00, 0x00, 0xFF]);
}

#[test]
fn tms9918_renderer_draws_basic_sprites_over_background() {
    let mut vdp = Vdp::new();
    let mut framebuffer = vec![0; SMS_TILE_SIZE * SMS_TILE_SIZE * RGBA_CHANNELS];

    vdp.registers[VDP_REGISTER_MODE_CONTROL_2] = VDP_REG1_DISPLAY_ENABLE;
    vdp.registers[TMS_REGISTER_SPRITE_ATTRIBUTE_TABLE] = 0x00;
    vdp.registers[TMS_REGISTER_SPRITE_PATTERN_TABLE] = 0x01;
    let pattern_base = vdp.tms_sprite_pattern_table_base();
    set_tms_pattern_row(&mut vdp, pattern_base, 4, 0, 0x80);
    vdp.vram[0] = 0xFF;
    vdp.vram[1] = 0;
    vdp.vram[2] = 4;
    vdp.vram[3] = 6;
    vdp.vram[4] = TMS_SPRITE_TERMINATOR_Y;

    vdp.render_tms9918_frame_rgba(&mut framebuffer, SMS_TILE_SIZE, SMS_TILE_SIZE);

    assert_eq!(&framebuffer[0..RGBA_CHANNELS], &[0xD4, 0x52, 0x4D, 0xFF]);
}

#[test]
fn tms9918_sprite_status_latches_collision_and_fifth_sprite() {
    let mut vdp = Vdp::new();

    vdp.registers[VDP_REGISTER_MODE_CONTROL_2] = VDP_REG1_DISPLAY_ENABLE;
    vdp.registers[TMS_REGISTER_SPRITE_ATTRIBUTE_TABLE] = 0x00;
    vdp.registers[TMS_REGISTER_SPRITE_PATTERN_TABLE] = 0x01;
    let pattern_base = vdp.tms_sprite_pattern_table_base();
    for pattern in 0..5 {
        set_tms_pattern_row(&mut vdp, pattern_base, pattern, 0, 0x80);
    }
    set_tms_sprite(&mut vdp, 0, 0, 10, 0, 2);
    set_tms_sprite(&mut vdp, 1, 0, 10, 1, 3);
    set_tms_sprite(&mut vdp, 2, 0, 30, 2, 4);
    set_tms_sprite(&mut vdp, 3, 0, 40, 3, 5);
    set_tms_sprite(&mut vdp, 4, 0, 50, 4, 6);
    vdp.vram[5 * TMS_SPRITE_ATTRIBUTE_BYTES] = TMS_SPRITE_TERMINATOR_Y;

    vdp.step_cycles(SMS_SCANLINE_Z80_CYCLES);

    assert_eq!(
        vdp.status() & VDP_STATUS_SPRITE_COLLISION,
        VDP_STATUS_SPRITE_COLLISION
    );
    assert_eq!(
        vdp.status() & VDP_STATUS_SPRITE_OVERFLOW,
        VDP_STATUS_SPRITE_OVERFLOW
    );
    assert_eq!(vdp.status() & 0x1F, 4);
}

#[test]
fn tms9918_transparent_sprite_pixels_still_collide() {
    let mut vdp = Vdp::new();

    vdp.registers[VDP_REGISTER_MODE_CONTROL_2] = VDP_REG1_DISPLAY_ENABLE;
    vdp.registers[TMS_REGISTER_SPRITE_ATTRIBUTE_TABLE] = 0x00;
    vdp.registers[TMS_REGISTER_SPRITE_PATTERN_TABLE] = 0x01;
    let pattern_base = vdp.tms_sprite_pattern_table_base();
    set_tms_pattern_row(&mut vdp, pattern_base, 0, 0, 0x80);
    set_tms_pattern_row(&mut vdp, pattern_base, 1, 0, 0x80);
    set_tms_sprite(&mut vdp, 0, 0, 0, 0, 0);
    set_tms_sprite(&mut vdp, 1, 0, 0, 1, 2);
    vdp.vram[2 * TMS_SPRITE_ATTRIBUTE_BYTES] = TMS_SPRITE_TERMINATOR_Y;

    vdp.step_cycles(SMS_SCANLINE_Z80_CYCLES);

    assert_eq!(
        vdp.status() & VDP_STATUS_SPRITE_COLLISION,
        VDP_STATUS_SPRITE_COLLISION
    );
}

#[test]
fn tms9918_transparent_sprites_count_for_fifth_sprite_status() {
    let mut vdp = Vdp::new();

    vdp.registers[VDP_REGISTER_MODE_CONTROL_2] = VDP_REG1_DISPLAY_ENABLE;
    vdp.registers[TMS_REGISTER_SPRITE_ATTRIBUTE_TABLE] = 0x00;
    vdp.registers[TMS_REGISTER_SPRITE_PATTERN_TABLE] = 0x01;
    let pattern_base = vdp.tms_sprite_pattern_table_base();
    for pattern in 0..5usize {
        set_tms_pattern_row(&mut vdp, pattern_base, pattern as u8, 0, 0x80);
        set_tms_sprite(&mut vdp, pattern, 0, pattern as u8 * 8, pattern as u8, 0);
    }
    vdp.vram[5 * TMS_SPRITE_ATTRIBUTE_BYTES] = TMS_SPRITE_TERMINATOR_Y;

    vdp.step_cycles(SMS_SCANLINE_Z80_CYCLES);

    assert_eq!(
        vdp.status() & VDP_STATUS_SPRITE_OVERFLOW,
        VDP_STATUS_SPRITE_OVERFLOW
    );
    assert_eq!(vdp.status() & 0x1F, 4);
}

#[test]
fn tms9918_fifth_sprite_number_latches_the_first_overflow_until_status_read() {
    let mut vdp = Vdp::new();

    vdp.registers[VDP_REGISTER_MODE_CONTROL_2] = VDP_REG1_DISPLAY_ENABLE;
    vdp.registers[TMS_REGISTER_SPRITE_ATTRIBUTE_TABLE] = 0x00;

    // The first overflowing line drops sprite 4. A later line drops sprite 9,
    // but the status register must retain the earlier fifth-sprite number until
    // the CPU acknowledges the status latch.
    for sprite in 0..5usize {
        set_tms_sprite(&mut vdp, sprite, 0, sprite as u8 * 8, sprite as u8, 1);
    }
    for sprite in 5..10usize {
        set_tms_sprite(&mut vdp, sprite, 8, sprite as u8 * 8, sprite as u8, 1);
    }
    vdp.vram[10 * TMS_SPRITE_ATTRIBUTE_BYTES] = TMS_SPRITE_TERMINATOR_Y;

    vdp.step_cycles(SMS_SCANLINE_Z80_CYCLES * 2);
    assert_eq!(
        vdp.status() & VDP_STATUS_SPRITE_OVERFLOW,
        VDP_STATUS_SPRITE_OVERFLOW
    );
    assert_eq!(vdp.status() & 0x1F, 4);

    let _ = vdp.read_status();
    vdp.scanline = 8;
    vdp.step_cycles(SMS_SCANLINE_Z80_CYCLES);

    assert_eq!(
        vdp.status() & VDP_STATUS_SPRITE_OVERFLOW,
        VDP_STATUS_SPRITE_OVERFLOW
    );
    assert_eq!(vdp.status() & 0x1F, 9);
}

#[test]
fn tms9918_sprite_terminator_stops_status_scan() {
    let mut vdp = Vdp::new();

    vdp.registers[VDP_REGISTER_MODE_CONTROL_2] = VDP_REG1_DISPLAY_ENABLE;
    vdp.registers[TMS_REGISTER_SPRITE_ATTRIBUTE_TABLE] = 0x00;
    vdp.registers[TMS_REGISTER_SPRITE_PATTERN_TABLE] = 0x01;
    let pattern_base = vdp.tms_sprite_pattern_table_base();
    for pattern in 0..6 {
        set_tms_pattern_row(&mut vdp, pattern_base, pattern, 0, 0x80);
    }
    set_tms_sprite(&mut vdp, 0, 0, 0, 0, 2);
    vdp.vram[TMS_SPRITE_ATTRIBUTE_BYTES] = TMS_SPRITE_TERMINATOR_Y;
    for sprite in 2..6 {
        set_tms_sprite(&mut vdp, sprite, 0, sprite as u8 * 8, sprite as u8, 2);
    }

    vdp.step_cycles(SMS_SCANLINE_Z80_CYCLES);

    assert_eq!(vdp.status() & VDP_STATUS_SPRITE_OVERFLOW, 0);
    assert_eq!(vdp.status() & VDP_STATUS_SPRITE_COLLISION, 0);
}

#[test]
fn tms9918_sprite_status_is_disabled_in_text_mode() {
    let mut vdp = Vdp::new();

    vdp.registers[VDP_REGISTER_MODE_CONTROL_2] = VDP_REG1_DISPLAY_ENABLE | TMS_REG1_MODE_TEXT;
    vdp.registers[TMS_REGISTER_SPRITE_ATTRIBUTE_TABLE] = 0x00;
    vdp.registers[TMS_REGISTER_SPRITE_PATTERN_TABLE] = 0x01;
    let pattern_base = vdp.tms_sprite_pattern_table_base();
    set_tms_pattern_row(&mut vdp, pattern_base, 0, 0, 0x80);
    set_tms_pattern_row(&mut vdp, pattern_base, 1, 0, 0x80);
    set_tms_sprite(&mut vdp, 0, 0, 0, 0, 2);
    set_tms_sprite(&mut vdp, 1, 0, 0, 1, 3);
    vdp.vram[2 * TMS_SPRITE_ATTRIBUTE_BYTES] = TMS_SPRITE_TERMINATOR_Y;

    vdp.step_cycles(SMS_SCANLINE_Z80_CYCLES);

    assert_eq!(
        vdp.status() & (VDP_STATUS_SPRITE_COLLISION | VDP_STATUS_SPRITE_OVERFLOW),
        0
    );
}

#[test]
fn tms9918_sprite_y_values_above_terminator_wrap_above_screen() {
    let mut vdp = Vdp::new();
    let mut framebuffer = vec![0; SMS_TILE_SIZE * SMS_TILE_SIZE * RGBA_CHANNELS];

    vdp.registers[VDP_REGISTER_MODE_CONTROL_2] = VDP_REG1_DISPLAY_ENABLE;
    vdp.registers[TMS_REGISTER_SPRITE_ATTRIBUTE_TABLE] = 0x00;
    vdp.registers[TMS_REGISTER_SPRITE_PATTERN_TABLE] = 0x01;
    let pattern_base = vdp.tms_sprite_pattern_table_base();
    set_tms_pattern_row(&mut vdp, pattern_base, 4, 0, 0x00);
    set_tms_pattern_row(&mut vdp, pattern_base, 4, 1, 0x80);
    set_tms_sprite(&mut vdp, 0, -1, 0, 4, 6);
    vdp.vram[TMS_SPRITE_ATTRIBUTE_BYTES] = TMS_SPRITE_TERMINATOR_Y;

    vdp.render_tms9918_frame_rgba(&mut framebuffer, SMS_TILE_SIZE, SMS_TILE_SIZE);

    assert_eq!(&framebuffer[0..RGBA_CHANNELS], &[0xD4, 0x52, 0x4D, 0xFF]);
}

#[test]
fn tms9918_early_clock_sprites_render_and_collide_left_of_raw_x() {
    let mut vdp = Vdp::new();
    let mut framebuffer = vec![0; SMS_TILE_SIZE * SMS_TILE_SIZE * RGBA_CHANNELS];

    vdp.registers[VDP_REGISTER_MODE_CONTROL_2] = VDP_REG1_DISPLAY_ENABLE;
    vdp.registers[TMS_REGISTER_SPRITE_ATTRIBUTE_TABLE] = 0x00;
    vdp.registers[TMS_REGISTER_SPRITE_PATTERN_TABLE] = 0x01;
    let pattern_base = vdp.tms_sprite_pattern_table_base();
    set_tms_pattern_row(&mut vdp, pattern_base, 0, 0, 0x80);
    set_tms_pattern_row(&mut vdp, pattern_base, 1, 0, 0x80);
    set_tms_sprite(&mut vdp, 0, 0, 32, 0, 6 | TMS_SPRITE_EARLY_CLOCK);
    set_tms_sprite(&mut vdp, 1, 0, 0, 1, 5);
    vdp.vram[2 * TMS_SPRITE_ATTRIBUTE_BYTES] = TMS_SPRITE_TERMINATOR_Y;

    vdp.step_cycles(SMS_SCANLINE_Z80_CYCLES);
    vdp.render_tms9918_frame_rgba(&mut framebuffer, SMS_TILE_SIZE, SMS_TILE_SIZE);

    assert_eq!(
        vdp.status() & VDP_STATUS_SPRITE_COLLISION,
        VDP_STATUS_SPRITE_COLLISION
    );
    assert_eq!(&framebuffer[0..RGBA_CHANNELS], &[0xD4, 0x52, 0x4D, 0xFF]);
}

#[test]
fn stepping_cycles_advances_counters_and_latches_vblank() {
    let mut vdp = Vdp::new();

    vdp.step_cycles(SMS_SCANLINE_Z80_CYCLES - 1);
    assert_eq!(vdp.scanline(), 0);
    assert_ne!(vdp.h_counter(), 0);

    vdp.step_cycles(1);
    assert_eq!(vdp.scanline(), 1);
    assert_eq!(vdp.v_counter(), 1);

    vdp.step_cycles(SMS_SCANLINE_Z80_CYCLES * u32::from(SMS_VISIBLE_SCANLINES - 1));
    assert_eq!(vdp.scanline(), SMS_VISIBLE_SCANLINES);
    assert_eq!(vdp.status() & VDP_STATUS_VBLANK, 0);

    vdp.step_cycles(SMS_SCANLINE_Z80_CYCLES);
    assert_eq!(
        vdp.scanline(),
        Sega8DisplayHeight::Lines192.frame_interrupt_scanline()
    );
    assert_eq!(vdp.status() & VDP_STATUS_VBLANK, VDP_STATUS_VBLANK);
    assert_eq!(vdp.read_status() & VDP_STATUS_VBLANK, VDP_STATUS_VBLANK);
    assert_eq!(vdp.status() & VDP_STATUS_VBLANK, 0);

    vdp.step_cycles(
        SMS_SCANLINE_Z80_CYCLES
            * u32::from(
                vdp.total_scanlines() - Sega8DisplayHeight::Lines192.frame_interrupt_scanline(),
            ),
    );
    assert_eq!(vdp.scanline(), 0);
}

#[test]
fn pal_video_standard_uses_313_scanline_frames() {
    let mut vdp = Vdp::new_with_video_standard(Sega8VideoStandard::Pal);

    assert_eq!(vdp.total_scanlines(), 313);
    vdp.step_cycles(
        SMS_SCANLINE_Z80_CYCLES
            * u32::from(Sega8DisplayHeight::Lines192.frame_interrupt_scanline()),
    );
    assert_eq!(
        vdp.scanline(),
        Sega8DisplayHeight::Lines192.frame_interrupt_scanline()
    );
    assert_eq!(vdp.status() & VDP_STATUS_VBLANK, VDP_STATUS_VBLANK);
    vdp.read_status();

    vdp.step_cycles(
        SMS_SCANLINE_Z80_CYCLES
            * u32::from(313 - Sega8DisplayHeight::Lines192.frame_interrupt_scanline() - 1),
    );
    assert_eq!(vdp.scanline(), 312);
    assert_eq!(vdp.status() & VDP_STATUS_VBLANK, 0);

    vdp.step_cycles(SMS_SCANLINE_Z80_CYCLES);
    assert_eq!(vdp.scanline(), 0);

    vdp.reset();
    assert_eq!(vdp.video_standard(), Sega8VideoStandard::Pal);
    assert_eq!(vdp.total_scanlines(), 313);
}

#[test]
fn v_counter_uses_192_line_sms_tv_detection_sequences() {
    let mut ntsc = Vdp::new_with_video_standard(Sega8VideoStandard::Ntsc);
    ntsc.step_cycles(SMS_SCANLINE_Z80_CYCLES * 0xDB);
    assert_eq!(ntsc.scanline(), 0xDB);
    assert_eq!(ntsc.v_counter(), 0xD5);

    let mut pal = Vdp::new_with_video_standard(Sega8VideoStandard::Pal);
    pal.step_cycles(SMS_SCANLINE_Z80_CYCLES * 0xF3);
    assert_eq!(pal.scanline(), 0xF3);
    assert_eq!(pal.v_counter(), 0xBA);
}

#[test]
fn mode4_224_line_mode_uses_extended_vblank_and_v_counter() {
    let mut vdp = Vdp::new_with_video_standard(Sega8VideoStandard::Ntsc);
    vdp.registers[VDP_REGISTER_MODE_CONTROL_1] = VDP_REG0_MODE4 | VDP_REG0_MODE4_EXTENDED_HEIGHT;
    vdp.registers[VDP_REGISTER_MODE_CONTROL_2] = VDP_REG1_MODE4_224_LINE;

    vdp.step_cycles(
        SMS_SCANLINE_Z80_CYCLES
            * u32::from(Sega8DisplayHeight::Lines224.frame_interrupt_scanline()),
    );
    assert_eq!(vdp.scanline(), 0xE1);
    assert_eq!(vdp.status() & VDP_STATUS_VBLANK, VDP_STATUS_VBLANK);

    vdp.step_cycles(SMS_SCANLINE_Z80_CYCLES * u32::from(0xEBu16 - 0xE1));
    assert_eq!(vdp.scanline(), 0xEB);
    assert_eq!(vdp.v_counter(), 0xE5);
}

#[test]
fn mode4_240_line_mode_uses_extended_vblank_and_pal_v_counter() {
    let mut vdp = Vdp::new_with_video_standard(Sega8VideoStandard::Pal);
    vdp.registers[VDP_REGISTER_MODE_CONTROL_1] = VDP_REG0_MODE4 | VDP_REG0_MODE4_EXTENDED_HEIGHT;
    vdp.registers[VDP_REGISTER_MODE_CONTROL_2] = VDP_REG1_MODE4_240_LINE;

    vdp.step_cycles(
        SMS_SCANLINE_Z80_CYCLES
            * u32::from(Sega8DisplayHeight::Lines240.frame_interrupt_scanline()),
    );
    assert_eq!(vdp.scanline(), 0xF1);
    assert_eq!(vdp.status() & VDP_STATUS_VBLANK, VDP_STATUS_VBLANK);

    vdp.step_cycles(SMS_SCANLINE_Z80_CYCLES * u32::from(0x10Bu16 - 0xF1));
    assert_eq!(vdp.scanline(), 0x10B);
    assert_eq!(vdp.v_counter(), 0xD2);
}
