use super::super::*;

pub(super) fn render_frame_rgba(
    vdp: &Vdp,
    framebuffer: &mut [u8],
    area: Mode4RenderArea,
    color_mode: Tms9918ColorMode,
) {
    let expected_len = area.expected_rgba_len();
    if framebuffer.len() < expected_len {
        return;
    }

    let backdrop = vdp.tms_backdrop_color(color_mode);
    if !vdp.display_enabled() {
        fill_rgba(framebuffer, area, backdrop);
        return;
    }

    match vdp.tms9918_mode() {
        Tms9918Mode::GraphicsI => render_graphics_i_rgba(vdp, framebuffer, area, color_mode),
        Tms9918Mode::GraphicsII => render_graphics_ii_rgba(vdp, framebuffer, area, color_mode),
        Tms9918Mode::Multicolor => render_multicolor_rgba(vdp, framebuffer, area, color_mode),
        Tms9918Mode::Text => render_text_rgba(vdp, framebuffer, area, color_mode),
        Tms9918Mode::Invalid => fill_rgba(framebuffer, area, backdrop),
    }

    if !matches!(vdp.tms9918_mode(), Tms9918Mode::Text) {
        render_sprites_rgba(vdp, framebuffer, area, color_mode);
    }
}

fn render_graphics_i_rgba(
    vdp: &Vdp,
    framebuffer: &mut [u8],
    area: Mode4RenderArea,
    color_mode: Tms9918ColorMode,
) {
    let name_base = vdp.tms_name_table_base();
    let pattern_base = vdp.tms_pattern_table_base();
    let color_base = vdp.tms_color_table_base();
    let backdrop = vdp.tms_backdrop_color(color_mode);

    for dest_y in 0..area.height {
        let full_y = area.source_y + dest_y;
        let tile_y = full_y / SMS_TILE_SIZE;
        let row = full_y % SMS_TILE_SIZE;
        for dest_x in 0..area.width {
            let full_x = area.source_x + dest_x;
            let tile_x = full_x / SMS_TILE_SIZE;
            let col = full_x % SMS_TILE_SIZE;
            let name_offset = tile_y * TMS_TILE_COLUMNS + tile_x;
            let pattern = vdp.vram[(name_base + name_offset) % vdp.vram.len()];
            let pattern_byte = vdp.vram
                [(pattern_base + usize::from(pattern) * SMS_TILE_SIZE + row) % vdp.vram.len()];
            let color_byte = vdp.vram[(color_base + usize::from(pattern >> 3)) % vdp.vram.len()];
            let color = if pattern_byte & (0x80 >> col) != 0 {
                color_byte >> 4
            } else {
                color_byte & 0x0F
            };
            let rgba = vdp.tms_color_rgba(color, backdrop, color_mode);
            let offset = (dest_y * area.width + dest_x) * RGBA_CHANNELS;
            framebuffer[offset..offset + RGBA_CHANNELS].copy_from_slice(&rgba);
        }
    }
}

fn render_graphics_ii_rgba(
    vdp: &Vdp,
    framebuffer: &mut [u8],
    area: Mode4RenderArea,
    color_mode: Tms9918ColorMode,
) {
    let name_base = vdp.tms_name_table_base();
    let pattern_base = vdp.tms_graphics_ii_pattern_table_base();
    let color_base = vdp.tms_graphics_ii_color_table_base();
    let backdrop = vdp.tms_backdrop_color(color_mode);

    for dest_y in 0..area.height {
        let full_y = area.source_y + dest_y;
        let tile_y = full_y / SMS_TILE_SIZE;
        let section = tile_y / TMS_GRAPHICS_II_SECTION_TILE_ROWS;
        let row = full_y % SMS_TILE_SIZE;
        for dest_x in 0..area.width {
            let full_x = area.source_x + dest_x;
            let tile_x = full_x / SMS_TILE_SIZE;
            let col = full_x % SMS_TILE_SIZE;
            let name_offset = tile_y * TMS_TILE_COLUMNS + tile_x;
            let pattern = vdp.vram[(name_base + name_offset) % vdp.vram.len()];
            let row_offset =
                section * TMS_TABLE_SECTION_BYTES + usize::from(pattern) * SMS_TILE_SIZE + row;
            let pattern_byte = vdp.vram[(pattern_base + row_offset) % vdp.vram.len()];
            let color_byte = vdp.vram[(color_base + row_offset) % vdp.vram.len()];
            let color = if pattern_byte & (0x80 >> col) != 0 {
                color_byte >> 4
            } else {
                color_byte & 0x0F
            };
            let rgba = vdp.tms_color_rgba(color, backdrop, color_mode);
            let offset = (dest_y * area.width + dest_x) * RGBA_CHANNELS;
            framebuffer[offset..offset + RGBA_CHANNELS].copy_from_slice(&rgba);
        }
    }
}

fn render_multicolor_rgba(
    vdp: &Vdp,
    framebuffer: &mut [u8],
    area: Mode4RenderArea,
    color_mode: Tms9918ColorMode,
) {
    let name_base = vdp.tms_name_table_base();
    let pattern_base = vdp.tms_pattern_table_base();
    let backdrop = vdp.tms_backdrop_color(color_mode);

    for dest_y in 0..area.height {
        let full_y = area.source_y + dest_y;
        let tile_y = full_y / SMS_TILE_SIZE;
        let color_row = (full_y % SMS_TILE_SIZE) / 4;
        for dest_x in 0..area.width {
            let full_x = area.source_x + dest_x;
            let tile_x = full_x / SMS_TILE_SIZE;
            let color_col = (full_x % SMS_TILE_SIZE) / 4;
            let name_offset = tile_y * TMS_TILE_COLUMNS + tile_x;
            let pattern = vdp.vram[(name_base + name_offset) % vdp.vram.len()];
            let color_byte =
                vdp.vram[(pattern_base + usize::from(pattern) * SMS_TILE_SIZE + color_row * 2)
                    % vdp.vram.len()];
            let color = if color_col == 0 {
                color_byte >> 4
            } else {
                color_byte & 0x0F
            };
            let rgba = vdp.tms_color_rgba(color, backdrop, color_mode);
            let offset = (dest_y * area.width + dest_x) * RGBA_CHANNELS;
            framebuffer[offset..offset + RGBA_CHANNELS].copy_from_slice(&rgba);
        }
    }
}

fn render_text_rgba(
    vdp: &Vdp,
    framebuffer: &mut [u8],
    area: Mode4RenderArea,
    color_mode: Tms9918ColorMode,
) {
    let name_base = vdp.tms_name_table_base();
    let pattern_base = vdp.tms_pattern_table_base();
    let fg = vdp.registers[TMS_REGISTER_TEXT_BACKDROP] >> 4;
    let bg = vdp.registers[TMS_REGISTER_TEXT_BACKDROP] & 0x0F;
    let backdrop = vdp.tms_backdrop_color(color_mode);
    fill_rgba(framebuffer, area, backdrop);

    for dest_y in 0..area.height {
        let full_y = area.source_y + dest_y;
        let tile_y = full_y / SMS_TILE_SIZE;
        let row = full_y % SMS_TILE_SIZE;
        for text_x in 0..TMS_TEXT_COLUMNS {
            let x0 = TMS_TEXT_LEFT_MARGIN + text_x * 6;
            if x0 >= area.source_x + area.width {
                break;
            }
            if x0 + 5 < area.source_x {
                continue;
            }
            let pattern =
                vdp.vram[(name_base + tile_y * TMS_TEXT_COLUMNS + text_x) % vdp.vram.len()];
            let pattern_byte = vdp.vram
                [(pattern_base + usize::from(pattern) * SMS_TILE_SIZE + row) % vdp.vram.len()];
            for col in 0..6usize {
                let full_x = x0 + col;
                if !(area.source_x..area.source_x + area.width).contains(&full_x) {
                    continue;
                }
                let color = if pattern_byte & (0x80 >> col) != 0 {
                    fg
                } else {
                    bg
                };
                let rgba = vdp.tms_color_rgba(color, backdrop, color_mode);
                let dest_x = full_x - area.source_x;
                let offset = (dest_y * area.width + dest_x) * RGBA_CHANNELS;
                framebuffer[offset..offset + RGBA_CHANNELS].copy_from_slice(&rgba);
            }
        }
    }
}

fn render_sprites_rgba(
    vdp: &Vdp,
    framebuffer: &mut [u8],
    area: Mode4RenderArea,
    color_mode: Tms9918ColorMode,
) {
    let attr_base = vdp.tms_sprite_attribute_table_base();
    let pattern_base = vdp.tms_sprite_pattern_table_base();
    let sprite_size = vdp.tms_sprite_base_size();
    let magnified = vdp.registers[VDP_REGISTER_MODE_CONTROL_2] & TMS_REG1_SPRITE_MAGNIFY != 0;
    let display_size = if magnified {
        sprite_size * 2
    } else {
        sprite_size
    };
    let backdrop = vdp.tms_backdrop_color(color_mode);
    let context = TmsSpriteRenderContext {
        area,
        pattern_base,
        sprite_size,
        magnified,
        backdrop,
        color_mode,
    };

    for dest_y in 0..area.height {
        let full_y = area.source_y + dest_y;
        let mut sprites = [None; TMS_MAX_SPRITES_PER_LINE];
        let mut count = 0usize;

        for index in 0..TMS_SPRITE_COUNT {
            let Some(sprite) = vdp.tms_sprite(attr_base, index) else {
                break;
            };
            if !tms_sprite_intersects_line(sprite, display_size, full_y as isize) {
                continue;
            }
            if count >= TMS_MAX_SPRITES_PER_LINE {
                break;
            }
            sprites[count] = Some(sprite);
            count += 1;
        }

        for sprite in sprites[..count].iter().rev().flatten() {
            render_sprite_row_rgba(vdp, framebuffer, dest_y, *sprite, context);
        }
    }
}

fn render_sprite_row_rgba(
    vdp: &Vdp,
    framebuffer: &mut [u8],
    dest_y: usize,
    sprite: TmsSprite,
    context: TmsSpriteRenderContext,
) {
    if sprite.color == TMS_COLOR_TRANSPARENT {
        return;
    }

    let scale = if context.magnified { 2usize } else { 1usize };
    let area = context.area;
    let full_y = area.source_y + dest_y;
    let local_y = full_y as isize - sprite.y;
    if local_y < 0 {
        return;
    }
    let pattern_y = (local_y as usize / scale).min(context.sprite_size - 1);
    let display_size = context.sprite_size * scale;

    for dest_col in 0..display_size {
        let screen_x = sprite.x + dest_col as isize;
        let dest_x = screen_x - area.source_x as isize;
        if !(0..area.width as isize).contains(&dest_x) {
            continue;
        }
        let pattern_x = (dest_col / scale).min(context.sprite_size - 1);
        if !vdp.tms_sprite_pattern_pixel(
            context.pattern_base,
            sprite.pattern,
            context.sprite_size,
            pattern_x,
            pattern_y,
        ) {
            continue;
        }
        let rgba = vdp.tms_color_rgba(sprite.color, context.backdrop, context.color_mode);
        let offset = (dest_y * area.width + dest_x as usize) * RGBA_CHANNELS;
        framebuffer[offset..offset + RGBA_CHANNELS].copy_from_slice(&rgba);
    }
}

fn fill_rgba(framebuffer: &mut [u8], area: Mode4RenderArea, color: [u8; RGBA_CHANNELS]) {
    let expected_len = area.expected_rgba_len();
    if framebuffer.len() < expected_len {
        return;
    }
    for pixel in framebuffer[..expected_len].chunks_exact_mut(RGBA_CHANNELS) {
        pixel.copy_from_slice(&color);
    }
}
