use super::super::*;

pub(super) fn render_background_rgba(vdp: &Vdp, framebuffer: &mut [u8], area: Mode4RenderArea) {
    render_background_rgba_with_color(vdp, framebuffer, area, Mode4ColorMode::Sms);
}

pub(super) fn render_frame_rgba(
    vdp: &Vdp,
    framebuffer: &mut [u8],
    area: Mode4RenderArea,
    color_mode: Mode4ColorMode,
) {
    if !vdp.display_enabled() {
        fill_backdrop_rgba(vdp, framebuffer, area, color_mode);
        return;
    }
    render_background_rgba_with_color(vdp, framebuffer, area, color_mode);
    render_sprites_rgba(vdp, framebuffer, area, color_mode);
    mask_left_column_rgba(vdp, framebuffer, area, color_mode);
}

pub(super) fn render_presented_frame_rgba(
    vdp: &Vdp,
    framebuffer: &mut [u8],
    area: Mode4RenderArea,
    color_mode: Mode4ColorMode,
) {
    let expected_len = area.expected_rgba_len();
    if framebuffer.len() < expected_len {
        return;
    }
    if color_mode == vdp.color_mode && vdp.copy_presented_area_rgba(framebuffer, area) {
        return;
    }

    render_background_rgba_with_color(vdp, framebuffer, area, color_mode);
    render_sprites_rgba(vdp, framebuffer, area, color_mode);
    mask_left_column_rgba(vdp, framebuffer, area, color_mode);
    fill_disabled_scanlines_backdrop_rgba(vdp, framebuffer, area, color_mode);
}

pub(super) fn render_background_rgba_with_color(
    vdp: &Vdp,
    framebuffer: &mut [u8],
    area: Mode4RenderArea,
    color_mode: Mode4ColorMode,
) {
    let expected_len = area.expected_rgba_len();
    if framebuffer.len() < expected_len {
        return;
    }

    let name_table_base = vdp.mode4_name_table_base();
    let name_table_rows = vdp.mode4_name_table_rows();
    for y in 0..area.height {
        let full_y = area.source_y + y;
        let h_scroll = if vdp.horizontal_scroll_locked_for_y(full_y) {
            0
        } else {
            usize::from(vdp.registers[VDP_REGISTER_HORIZONTAL_SCROLL])
        };
        let mut x = 0;
        while x < area.width {
            let full_x = area.source_x + x;
            let v_scroll = if vdp.vertical_scroll_locked_for_x(full_x) {
                0
            } else {
                usize::from(vdp.registers[VDP_REGISTER_VERTICAL_SCROLL])
            };
            let screen_y = (full_y + v_scroll) % (name_table_rows * SMS_TILE_SIZE);
            let tile_y = (screen_y / SMS_TILE_SIZE) % name_table_rows;
            let row_in_tile = screen_y % SMS_TILE_SIZE;
            let screen_x = full_x.wrapping_sub(h_scroll) % (SMS_NAME_TABLE_COLUMNS * SMS_TILE_SIZE);
            let tile_x = (screen_x / SMS_TILE_SIZE) % SMS_NAME_TABLE_COLUMNS;
            let col_in_tile = screen_x % SMS_TILE_SIZE;
            let tile_entry = vdp.mode4_name_table_entry(name_table_base, tile_x, tile_y);
            let tile_index = usize::from(tile_entry & MODE4_TILE_INDEX_MASK);
            let pattern_row = if tile_entry & MODE4_TILE_VFLIP != 0 {
                SMS_TILE_SIZE - 1 - row_in_tile
            } else {
                row_in_tile
            };
            let pattern_base = tile_index * SMS_MODE4_TILE_BYTES + pattern_row * 4;
            let planes = [
                vdp.vram[pattern_base % vdp.vram.len()],
                vdp.vram[(pattern_base + 1) % vdp.vram.len()],
                vdp.vram[(pattern_base + 2) % vdp.vram.len()],
                vdp.vram[(pattern_base + 3) % vdp.vram.len()],
            ];
            let palette_offset = if tile_entry & MODE4_TILE_PALETTE != 0 {
                MODE4_PALETTE_COLOR_OFFSET
            } else {
                0
            };
            let mut span = (SMS_TILE_SIZE - col_in_tile).min(area.width - x);
            if full_x < 24 * SMS_TILE_SIZE {
                span = span.min(24 * SMS_TILE_SIZE - full_x);
            }

            for offset in 0..span {
                let col = (col_in_tile + offset) % SMS_TILE_SIZE;
                let pattern_col = if tile_entry & MODE4_TILE_HFLIP != 0 {
                    SMS_TILE_SIZE - 1 - col
                } else {
                    col
                };
                let bit = MODE4_PATTERN_LEFT_PIXEL_MASK >> pattern_col;
                let color = usize::from(planes[0] & bit != 0)
                    | (usize::from(planes[1] & bit != 0) << 1)
                    | (usize::from(planes[2] & bit != 0) << 2)
                    | (usize::from(planes[3] & bit != 0) << 3);
                let rgba = vdp.mode4_color_rgba(color + palette_offset, color_mode);
                let dest_x = x + offset;
                let pixel_offset = (y * area.width + dest_x) * RGBA_CHANNELS;
                framebuffer[pixel_offset..pixel_offset + RGBA_CHANNELS].copy_from_slice(&rgba);
            }
            x += span;
        }
    }
}

fn mask_left_column_rgba(
    vdp: &Vdp,
    framebuffer: &mut [u8],
    area: Mode4RenderArea,
    color_mode: Mode4ColorMode,
) {
    if vdp.registers[VDP_REGISTER_MODE_CONTROL_1] & VDP_REG0_HIDE_LEFT_COLUMN == 0 {
        return;
    }
    let expected_len = area.expected_rgba_len();
    if framebuffer.len() < expected_len || area.source_x >= SMS_TILE_SIZE {
        return;
    }

    let columns = (SMS_TILE_SIZE - area.source_x).min(area.width);
    let rgba = vdp.mode4_color_rgba(vdp.mode4_backdrop_color_index(), color_mode);
    for y in 0..area.height {
        for x in 0..columns {
            let offset = (y * area.width + x) * RGBA_CHANNELS;
            framebuffer[offset..offset + RGBA_CHANNELS].copy_from_slice(&rgba);
        }
    }
}

fn fill_backdrop_rgba(
    vdp: &Vdp,
    framebuffer: &mut [u8],
    area: Mode4RenderArea,
    color_mode: Mode4ColorMode,
) {
    let expected_len = area.expected_rgba_len();
    if framebuffer.len() < expected_len {
        return;
    }

    let rgba = vdp.mode4_color_rgba(vdp.mode4_backdrop_color_index(), color_mode);
    for pixel in framebuffer[..expected_len]
        .as_chunks_mut::<RGBA_CHANNELS>()
        .0
    {
        pixel.copy_from_slice(&rgba);
    }
}

fn fill_disabled_scanlines_backdrop_rgba(
    vdp: &Vdp,
    framebuffer: &mut [u8],
    area: Mode4RenderArea,
    color_mode: Mode4ColorMode,
) {
    let rgba = vdp.mode4_color_rgba(vdp.mode4_backdrop_color_index(), color_mode);
    for y in 0..area.height {
        if vdp.scanline_display_enabled_for_source_y(area.source_y + y) {
            continue;
        }
        let row_start = y * area.width * RGBA_CHANNELS;
        let row_end = row_start + area.width * RGBA_CHANNELS;
        for pixel in framebuffer[row_start..row_end]
            .as_chunks_mut::<RGBA_CHANNELS>()
            .0
        {
            pixel.copy_from_slice(&rgba);
        }
    }
}

fn render_sprites_rgba(
    vdp: &Vdp,
    framebuffer: &mut [u8],
    area: Mode4RenderArea,
    color_mode: Mode4ColorMode,
) {
    let expected_len = area.expected_rgba_len();
    if framebuffer.len() < expected_len {
        return;
    }

    let table_base = vdp.mode4_sprite_table_base();
    let name_table_base = vdp.mode4_name_table_base();
    let sprite_pattern_base = vdp.mode4_sprite_pattern_base();
    let sprite_base_height = vdp.mode4_sprite_base_height();
    let sprite_scale = vdp.mode4_sprite_scale();
    let sprite_height = sprite_base_height * sprite_scale;
    let x_shift = vdp.mode4_sprite_x_shift();
    let context = Mode4SpriteRenderContext {
        area,
        name_table_base,
        sprite_pattern_base,
        sprite_scale,
        color_mode,
    };

    for dest_y in 0..area.height {
        let screen_y = (area.source_y + dest_y) as isize;
        let mut sprites_on_line = 0usize;
        let mut sprites = [None; MODE4_MAX_SPRITES_PER_LINE];
        for sprite_index in 0..MODE4_SPRITE_COUNT {
            let Some(sprite) =
                vdp.mode4_sprite(table_base, sprite_base_height, x_shift, sprite_index)
            else {
                break;
            };
            let Some(row) = mode4_sprite_row_for_line(sprite, sprite_height, screen_y) else {
                continue;
            };
            if sprites_on_line >= MODE4_MAX_SPRITES_PER_LINE {
                break;
            }
            sprites[sprites_on_line] = Some((sprite, row));
            sprites_on_line += 1;
        }

        for (sprite, row) in sprites[..sprites_on_line].iter().rev().flatten().copied() {
            render_sprite_row_rgba(vdp, framebuffer, context, dest_y, sprite, row);
        }
    }
}

fn render_sprite_row_rgba(
    vdp: &Vdp,
    framebuffer: &mut [u8],
    context: Mode4SpriteRenderContext,
    dest_y: usize,
    sprite: Mode4Sprite,
    row: usize,
) {
    let area = context.area;
    let sprite_scale = context.sprite_scale;
    let pattern_y = row / sprite_scale;
    let pattern_row = pattern_y % SMS_TILE_SIZE;
    let pattern_tile = usize::from(sprite.tile_index) + pattern_y / SMS_TILE_SIZE;

    for dest_col in 0..SMS_TILE_SIZE * sprite_scale {
        let screen_x = sprite.x + dest_col as isize;
        let dest_x = screen_x - area.source_x as isize;
        if !(0..area.width as isize).contains(&dest_x) {
            continue;
        }

        let col = dest_col / sprite_scale;
        let color =
            vdp.mode4_sprite_color(context.sprite_pattern_base, pattern_tile, col, pattern_row);
        if color == 0 {
            continue;
        }
        let full_x = area.source_x + dest_x as usize;
        let full_y = area.source_y + dest_y;
        if vdp
            .mode4_background_pixel(context.name_table_base, full_x, full_y)
            .priority
        {
            continue;
        }
        let rgba = vdp.mode4_color_rgba(color + MODE4_PALETTE_COLOR_OFFSET, context.color_mode);
        let offset = (dest_y * area.width + dest_x as usize) * RGBA_CHANNELS;
        framebuffer[offset..offset + RGBA_CHANNELS].copy_from_slice(&rgba);
    }
}
