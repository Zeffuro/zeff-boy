use super::Ppu;
use crate::hardware::constants::{SCREEN_HEIGHT, SCREEN_WIDTH};

mod affine;
mod bitmap;
mod effects;
mod obj;
mod text;
mod window;

use effects::{ColorEffects, Layer, Mosaic};
use window::Windows;

impl Ppu {
    pub(super) fn render(&mut self, io: &[u8], palette_ram: &[u8], vram: &[u8], oam: &[u8]) {
        let dispcnt = read_le16(io, 0);
        if dispcnt & (1 << 7) != 0 {
            self.fill_forced_blank();
            return;
        }

        let mode = dispcnt & 0x7;
        let debug_flags = self.debug_flags();
        let mosaic = Mosaic::from_io(io);
        let effects = ColorEffects::from_io(io);
        let windows = if debug_flags.window {
            Windows::from_io(dispcnt, io, vram, oam, mosaic)
        } else {
            Windows::disabled()
        };
        let mut bg_priorities = vec![4u8; SCREEN_WIDTH * SCREEN_HEIGHT];
        let mut bg_layers = vec![4u8; SCREEN_WIDTH * SCREEN_HEIGHT];
        let mut bg_second_priorities = vec![4u8; SCREEN_WIDTH * SCREEN_HEIGHT];
        let mut bg_second_layers = vec![Layer::Backdrop; SCREEN_WIDTH * SCREEN_HEIGHT];
        let mut bg_second_colors = vec![0u16; SCREEN_WIDTH * SCREEN_HEIGHT];
        let mut pixel_layers = vec![Layer::Backdrop; SCREEN_WIDTH * SCREEN_HEIGHT];
        let mut pixel_colors = vec![0u16; SCREEN_WIDTH * SCREEN_HEIGHT];
        let mut obj_priorities = vec![4u8; SCREEN_WIDTH * SCREEN_HEIGHT];
        if debug_flags.bg {
            match mode {
                0 => self.render_text_mode(
                    dispcnt,
                    io,
                    palette_ram,
                    vram,
                    &mut bg_priorities,
                    &mut bg_layers,
                    &mut bg_second_priorities,
                    &mut bg_second_layers,
                    &mut bg_second_colors,
                    &mut pixel_layers,
                    &mut pixel_colors,
                    4,
                    effects,
                    &windows,
                    mosaic,
                ),
                1 => {
                    self.render_text_mode(
                        dispcnt,
                        io,
                        palette_ram,
                        vram,
                        &mut bg_priorities,
                        &mut bg_layers,
                        &mut bg_second_priorities,
                        &mut bg_second_layers,
                        &mut bg_second_colors,
                        &mut pixel_layers,
                        &mut pixel_colors,
                        2,
                        effects,
                        &windows,
                        mosaic,
                    );
                    if debug_flags.bg_layers[2] && dispcnt & (1 << 10) != 0 {
                        self.render_affine_bg(
                            2,
                            read_le16(io, 0x0C),
                            io,
                            palette_ram,
                            vram,
                            &mut bg_priorities,
                            &mut bg_layers,
                            &mut bg_second_priorities,
                            &mut bg_second_layers,
                            &mut bg_second_colors,
                            &mut pixel_layers,
                            &mut pixel_colors,
                            effects,
                            &windows,
                            mosaic,
                        );
                    }
                }
                2 => self.render_affine_mode(
                    dispcnt,
                    io,
                    palette_ram,
                    vram,
                    &mut bg_priorities,
                    &mut bg_layers,
                    &mut bg_second_priorities,
                    &mut bg_second_layers,
                    &mut bg_second_colors,
                    &mut pixel_layers,
                    &mut pixel_colors,
                    effects,
                    &windows,
                    mosaic,
                ),
                3 if debug_flags.bg_layers[2] => self.render_mode3(
                    io,
                    palette_ram,
                    vram,
                    &mut bg_priorities,
                    &mut bg_layers,
                    &mut bg_second_priorities,
                    &mut bg_second_layers,
                    &mut bg_second_colors,
                    effects,
                    &windows,
                    &mut pixel_layers,
                    &mut pixel_colors,
                    mosaic,
                ),
                4 if debug_flags.bg_layers[2] => self.render_mode4(
                    dispcnt,
                    io,
                    palette_ram,
                    vram,
                    &mut bg_priorities,
                    &mut bg_layers,
                    &mut bg_second_priorities,
                    &mut bg_second_layers,
                    &mut bg_second_colors,
                    effects,
                    &windows,
                    &mut pixel_layers,
                    &mut pixel_colors,
                    mosaic,
                ),
                5 if debug_flags.bg_layers[2] => self.render_mode5(
                    dispcnt,
                    io,
                    palette_ram,
                    vram,
                    &mut bg_priorities,
                    &mut bg_layers,
                    &mut bg_second_priorities,
                    &mut bg_second_layers,
                    &mut bg_second_colors,
                    effects,
                    &windows,
                    &mut pixel_layers,
                    &mut pixel_colors,
                    mosaic,
                ),
                _ => self.fill_backdrop(
                    palette_ram,
                    effects,
                    &windows,
                    &mut pixel_layers,
                    &mut pixel_colors,
                ),
            }
        } else {
            self.fill_backdrop(
                palette_ram,
                effects,
                &windows,
                &mut pixel_layers,
                &mut pixel_colors,
            );
        }
        if debug_flags.sprites && dispcnt & (1 << 12) != 0 {
            self.render_objs(
                dispcnt,
                palette_ram,
                vram,
                oam,
                &bg_priorities,
                &bg_second_priorities,
                &mut obj_priorities,
                &mut pixel_layers,
                &mut pixel_colors,
                effects,
                &windows,
                mosaic,
            );
        }
    }

    pub(super) fn render_scanline(
        &mut self,
        y: usize,
        io: &[u8],
        palette_ram: &[u8],
        vram: &[u8],
        oam: &[u8],
    ) {
        if y >= SCREEN_HEIGHT {
            return;
        }

        let dispcnt = read_le16(io, 0);
        if dispcnt & (1 << 7) != 0 {
            self.fill_forced_blank_line(y);
            return;
        }

        let mode = dispcnt & 0x7;
        let debug_flags = self.debug_flags();
        let mosaic = Mosaic::from_io(io);
        let effects = ColorEffects::from_io(io);
        let windows = if debug_flags.window {
            Windows::from_io_scanline(dispcnt, io, vram, oam, mosaic, y)
        } else {
            Windows::disabled()
        };
        let mut bg_priorities = [4u8; SCREEN_WIDTH];
        let mut bg_layers = [4u8; SCREEN_WIDTH];
        let mut bg_second_priorities = [4u8; SCREEN_WIDTH];
        let mut bg_second_layers = [Layer::Backdrop; SCREEN_WIDTH];
        let mut bg_second_colors = [0u16; SCREEN_WIDTH];
        let mut pixel_layers = [Layer::Backdrop; SCREEN_WIDTH];
        let mut pixel_colors = [0u16; SCREEN_WIDTH];
        let mut obj_priorities = [4u8; SCREEN_WIDTH];

        self.fill_backdrop_line(
            y,
            palette_ram,
            effects,
            &windows,
            &mut pixel_layers,
            &mut pixel_colors,
        );

        if debug_flags.bg {
            match mode {
                0 => self.render_text_mode_line(
                    dispcnt,
                    io,
                    palette_ram,
                    vram,
                    y,
                    &mut bg_priorities,
                    &mut bg_layers,
                    &mut bg_second_priorities,
                    &mut bg_second_layers,
                    &mut bg_second_colors,
                    &mut pixel_layers,
                    &mut pixel_colors,
                    4,
                    effects,
                    &windows,
                    mosaic,
                ),
                1 => {
                    self.render_text_mode_line(
                        dispcnt,
                        io,
                        palette_ram,
                        vram,
                        y,
                        &mut bg_priorities,
                        &mut bg_layers,
                        &mut bg_second_priorities,
                        &mut bg_second_layers,
                        &mut bg_second_colors,
                        &mut pixel_layers,
                        &mut pixel_colors,
                        2,
                        effects,
                        &windows,
                        mosaic,
                    );
                    if debug_flags.bg_layers[2] && dispcnt & (1 << 10) != 0 {
                        self.render_affine_bg_line(
                            2,
                            read_le16(io, 0x0C),
                            io,
                            palette_ram,
                            vram,
                            y,
                            &mut bg_priorities,
                            &mut bg_layers,
                            &mut bg_second_priorities,
                            &mut bg_second_layers,
                            &mut bg_second_colors,
                            &mut pixel_layers,
                            &mut pixel_colors,
                            effects,
                            &windows,
                            mosaic,
                        );
                    }
                }
                2 => self.render_affine_mode_line(
                    dispcnt,
                    io,
                    palette_ram,
                    vram,
                    y,
                    &mut bg_priorities,
                    &mut bg_layers,
                    &mut bg_second_priorities,
                    &mut bg_second_layers,
                    &mut bg_second_colors,
                    &mut pixel_layers,
                    &mut pixel_colors,
                    effects,
                    &windows,
                    mosaic,
                ),
                3 if debug_flags.bg_layers[2] => self.render_mode3_line(
                    io,
                    palette_ram,
                    vram,
                    y,
                    &mut bg_priorities,
                    &mut bg_layers,
                    &mut bg_second_priorities,
                    &mut bg_second_layers,
                    &mut bg_second_colors,
                    effects,
                    &windows,
                    &mut pixel_layers,
                    &mut pixel_colors,
                    mosaic,
                ),
                4 if debug_flags.bg_layers[2] => self.render_mode4_line(
                    dispcnt,
                    io,
                    palette_ram,
                    vram,
                    y,
                    &mut bg_priorities,
                    &mut bg_layers,
                    &mut bg_second_priorities,
                    &mut bg_second_layers,
                    &mut bg_second_colors,
                    effects,
                    &windows,
                    &mut pixel_layers,
                    &mut pixel_colors,
                    mosaic,
                ),
                5 if debug_flags.bg_layers[2] => self.render_mode5_line(
                    dispcnt,
                    io,
                    palette_ram,
                    vram,
                    y,
                    &mut bg_priorities,
                    &mut bg_layers,
                    &mut bg_second_priorities,
                    &mut bg_second_layers,
                    &mut bg_second_colors,
                    effects,
                    &windows,
                    &mut pixel_layers,
                    &mut pixel_colors,
                    mosaic,
                ),
                _ => {}
            }
        }

        if debug_flags.sprites && dispcnt & (1 << 12) != 0 {
            self.render_objs_line(
                dispcnt,
                palette_ram,
                vram,
                oam,
                y,
                &bg_priorities,
                &bg_second_priorities,
                &mut obj_priorities,
                &mut pixel_layers,
                &mut pixel_colors,
                effects,
                &windows,
                mosaic,
            );
        }
    }

    fn fill_forced_blank(&mut self) {
        for px in self.framebuffer.chunks_exact_mut(4) {
            px.copy_from_slice(&[0xFF, 0xFF, 0xFF, 0xFF]);
        }
    }

    fn fill_forced_blank_line(&mut self, y: usize) {
        let start = y * SCREEN_WIDTH * 4;
        let end = start + SCREEN_WIDTH * 4;
        for px in self.framebuffer[start..end].chunks_exact_mut(4) {
            px.copy_from_slice(&[0xFF, 0xFF, 0xFF, 0xFF]);
        }
    }

    fn fill_backdrop(
        &mut self,
        palette_ram: &[u8],
        effects: ColorEffects,
        windows: &Windows,
        pixel_layers: &mut [Layer],
        pixel_colors: &mut [u16],
    ) {
        let raw_color = read_le16(palette_ram, 0);
        for (index, px) in self.framebuffer.chunks_exact_mut(4).enumerate() {
            let x = index % SCREEN_WIDTH;
            let y = index / SCREEN_WIDTH;
            let color = effects.apply_pixel(
                raw_color,
                Layer::Backdrop,
                None,
                false,
                windows.allows_effect(x, y),
            );
            let [r, g, b] = bgr555_to_rgb(color);
            px.copy_from_slice(&[r, g, b, 0xFF]);
            pixel_layers[index] = Layer::Backdrop;
            pixel_colors[index] = color;
        }
    }

    fn fill_backdrop_line(
        &mut self,
        y: usize,
        palette_ram: &[u8],
        effects: ColorEffects,
        windows: &Windows,
        pixel_layers: &mut [Layer; SCREEN_WIDTH],
        pixel_colors: &mut [u16; SCREEN_WIDTH],
    ) {
        let raw_color = read_le16(palette_ram, 0);
        for x in 0..SCREEN_WIDTH {
            let color = effects.apply_pixel(
                raw_color,
                Layer::Backdrop,
                None,
                false,
                windows.allows_effect(x, y),
            );
            write_rgba(&mut self.framebuffer, x, y, bgr555_to_rgb(color));
            pixel_layers[x] = Layer::Backdrop;
            pixel_colors[x] = color;
        }
    }
}

fn bg_pixel_is_on_top(priority: u8, bg: usize, current_priority: u8, current_bg: u8) -> bool {
    priority < current_priority || (priority == current_priority && (bg as u8) < current_bg)
}

fn draw_bg_color(
    framebuffer: &mut [u8],
    pixel_layers: &mut [Layer],
    pixel_colors: &mut [u16],
    bg_priorities: &mut [u8],
    bg_layers: &mut [u8],
    bg_second_priorities: &mut [u8],
    bg_second_layers: &mut [Layer],
    bg_second_colors: &mut [u16],
    x: usize,
    y: usize,
    color: u16,
    bg: usize,
    priority: u8,
    effects: ColorEffects,
    effects_enabled: bool,
) {
    let index = y * SCREEN_WIDTH + x;
    bg_second_priorities[index] = bg_priorities[index];
    bg_second_layers[index] = pixel_layers[index];
    bg_second_colors[index] = pixel_colors[index];
    draw_color(
        framebuffer,
        pixel_layers,
        pixel_colors,
        x,
        y,
        color,
        Layer::Bg(bg),
        effects,
        false,
        effects_enabled,
    );
    bg_priorities[index] = priority;
    bg_layers[index] = bg as u8;
}

fn draw_bg_color_line(
    framebuffer: &mut [u8],
    pixel_layers: &mut [Layer; SCREEN_WIDTH],
    pixel_colors: &mut [u16; SCREEN_WIDTH],
    bg_priorities: &mut [u8; SCREEN_WIDTH],
    bg_layers: &mut [u8; SCREEN_WIDTH],
    bg_second_priorities: &mut [u8; SCREEN_WIDTH],
    bg_second_layers: &mut [Layer; SCREEN_WIDTH],
    bg_second_colors: &mut [u16; SCREEN_WIDTH],
    x: usize,
    y: usize,
    color: u16,
    bg: usize,
    priority: u8,
    effects: ColorEffects,
    effects_enabled: bool,
) {
    bg_second_priorities[x] = bg_priorities[x];
    bg_second_layers[x] = pixel_layers[x];
    bg_second_colors[x] = pixel_colors[x];
    draw_color_line(
        framebuffer,
        pixel_layers,
        pixel_colors,
        x,
        y,
        color,
        Layer::Bg(bg),
        effects,
        false,
        effects_enabled,
    );
    bg_priorities[x] = priority;
    bg_layers[x] = bg as u8;
}

fn blend_obj_under_current_top(
    framebuffer: &mut [u8],
    pixel_layers: &[Layer],
    pixel_colors: &[u16],
    x: usize,
    y: usize,
    obj_color: u16,
    effects: ColorEffects,
    effects_enabled: bool,
) {
    let index = y * SCREEN_WIDTH + x;
    let top_layer = pixel_layers[index];
    if !matches!(top_layer, Layer::Bg(_)) {
        return;
    }
    if let Some(color) =
        effects.alpha_blend_pixel(pixel_colors[index], top_layer, obj_color, Layer::Obj, false)
    {
        if effects_enabled {
            write_rgba(framebuffer, x, y, bgr555_to_rgb(color));
        }
    }
}

fn blend_obj_under_current_top_line(
    framebuffer: &mut [u8],
    pixel_layers: &[Layer; SCREEN_WIDTH],
    pixel_colors: &[u16; SCREEN_WIDTH],
    x: usize,
    y: usize,
    obj_color: u16,
    effects: ColorEffects,
    effects_enabled: bool,
) {
    let top_layer = pixel_layers[x];
    if !matches!(top_layer, Layer::Bg(_)) {
        return;
    }
    if let Some(color) =
        effects.alpha_blend_pixel(pixel_colors[x], top_layer, obj_color, Layer::Obj, false)
    {
        if effects_enabled {
            write_rgba(framebuffer, x, y, bgr555_to_rgb(color));
        }
    }
}

fn draw_color(
    framebuffer: &mut [u8],
    pixel_layers: &mut [Layer],
    pixel_colors: &mut [u16],
    x: usize,
    y: usize,
    color: u16,
    layer: Layer,
    effects: ColorEffects,
    force_alpha: bool,
    effects_enabled: bool,
) {
    let index = y * SCREEN_WIDTH + x;
    let lower = Some((pixel_colors[index], pixel_layers[index]));
    let raw_color = color;
    let displayed_color =
        effects.apply_pixel(raw_color, layer, lower, force_alpha, effects_enabled);
    write_rgba(framebuffer, x, y, bgr555_to_rgb(displayed_color));
    pixel_layers[index] = layer;
    pixel_colors[index] = raw_color;
}

fn draw_color_line(
    framebuffer: &mut [u8],
    pixel_layers: &mut [Layer; SCREEN_WIDTH],
    pixel_colors: &mut [u16; SCREEN_WIDTH],
    x: usize,
    y: usize,
    color: u16,
    layer: Layer,
    effects: ColorEffects,
    force_alpha: bool,
    effects_enabled: bool,
) {
    let lower = Some((pixel_colors[x], pixel_layers[x]));
    let raw_color = color;
    let displayed_color =
        effects.apply_pixel(raw_color, layer, lower, force_alpha, effects_enabled);
    write_rgba(framebuffer, x, y, bgr555_to_rgb(displayed_color));
    pixel_layers[x] = layer;
    pixel_colors[x] = raw_color;
}

fn read_le16(data: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes([
        data.get(offset).copied().unwrap_or(0),
        data.get(offset + 1).copied().unwrap_or(0),
    ])
}

fn read_i16(data: &[u8], offset: usize) -> i16 {
    read_le16(data, offset) as i16
}

fn read_i28_8(data: &[u8], offset: usize) -> i32 {
    let raw = u32::from_le_bytes([
        data.get(offset).copied().unwrap_or(0),
        data.get(offset + 1).copied().unwrap_or(0),
        data.get(offset + 2).copied().unwrap_or(0),
        data.get(offset + 3).copied().unwrap_or(0),
    ]);
    ((raw << 4) as i32) >> 4
}

fn bgr555_to_rgb(color: u16) -> [u8; 3] {
    let r = (color & 0x1F) as u8;
    let g = ((color >> 5) & 0x1F) as u8;
    let b = ((color >> 10) & 0x1F) as u8;
    [expand5(r), expand5(g), expand5(b)]
}

fn expand5(v: u8) -> u8 {
    (v << 3) | (v >> 2)
}

fn write_rgba(framebuffer: &mut [u8], x: usize, y: usize, [r, g, b]: [u8; 3]) {
    let dst = (y * SCREEN_WIDTH + x) * 4;
    if let Some(px) = framebuffer.get_mut(dst..dst + 4) {
        px.copy_from_slice(&[r, g, b, 0xFF]);
    }
}
