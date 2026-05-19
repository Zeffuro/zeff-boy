use super::Ppu;
use crate::hardware::constants::{SCREEN_HEIGHT, SCREEN_WIDTH};

impl Ppu {
    pub(super) fn render(&mut self, io: &[u8], palette_ram: &[u8], vram: &[u8], oam: &[u8]) {
        let dispcnt = read_le16(io, 0);
        let mode = dispcnt & 0x7;
        match mode {
            0 => self.render_text_mode(dispcnt, io, palette_ram, vram, 4),
            1 => {
                self.render_text_mode(dispcnt, io, palette_ram, vram, 2);
                if dispcnt & (1 << 10) != 0 {
                    self.render_affine_bg(2, read_le16(io, 0x0C), io, palette_ram, vram);
                }
            }
            2 => self.render_affine_mode(dispcnt, io, palette_ram, vram),
            3 => self.render_mode3(vram),
            4 => self.render_mode4(dispcnt, palette_ram, vram),
            5 => self.render_mode5(dispcnt, vram),
            _ => self.fill_backdrop(palette_ram),
        }
        if dispcnt & (1 << 12) != 0 {
            self.render_objs(dispcnt, palette_ram, vram, oam);
        }
    }

    fn fill_backdrop(&mut self, palette_ram: &[u8]) {
        let [r, g, b] = bgr555_to_rgb(read_le16(palette_ram, 0));
        for px in self.framebuffer.chunks_exact_mut(4) {
            px.copy_from_slice(&[r, g, b, 0xFF]);
        }
    }

    fn render_mode3(&mut self, vram: &[u8]) {
        for y in 0..SCREEN_HEIGHT {
            for x in 0..SCREEN_WIDTH {
                let src = (y * SCREEN_WIDTH + x) * 2;
                let color = bgr555_to_rgb(read_le16(vram, src));
                write_rgba(&mut self.framebuffer, x, y, color);
            }
        }
    }

    fn render_mode4(&mut self, dispcnt: u16, palette_ram: &[u8], vram: &[u8]) {
        let page = if dispcnt & (1 << 4) != 0 { 0xA000 } else { 0 };
        for y in 0..SCREEN_HEIGHT {
            for x in 0..SCREEN_WIDTH {
                let index = vram.get(page + y * SCREEN_WIDTH + x).copied().unwrap_or(0) as usize;
                let color = bgr555_to_rgb(read_le16(palette_ram, index * 2));
                write_rgba(&mut self.framebuffer, x, y, color);
            }
        }
    }

    fn render_mode5(&mut self, dispcnt: u16, vram: &[u8]) {
        let page = if dispcnt & (1 << 4) != 0 { 0xA000 } else { 0 };
        self.fill_backdrop(&[]);
        for y in 0..128usize {
            for x in 0..160usize {
                let src = page + (y * 160 + x) * 2;
                let color = bgr555_to_rgb(read_le16(vram, src));
                write_rgba(&mut self.framebuffer, x, y, color);
            }
        }
    }

    fn render_text_mode(
        &mut self,
        dispcnt: u16,
        io: &[u8],
        palette_ram: &[u8],
        vram: &[u8],
        bg_count: usize,
    ) {
        self.fill_backdrop(palette_ram);
        for bg in 0..bg_count {
            if dispcnt & (1 << (8 + bg)) == 0 {
                continue;
            }
            let control = read_le16(io, 0x08 + bg * 2);
            self.render_text_bg(bg, control, io, palette_ram, vram);
        }
    }

    fn render_text_bg(
        &mut self,
        bg: usize,
        control: u16,
        io: &[u8],
        palette_ram: &[u8],
        vram: &[u8],
    ) {
        let char_base = (((control >> 2) & 0x3) as usize) * 0x4000;
        let color_256 = control & (1 << 7) != 0;
        let screen_base = (((control >> 8) & 0x1F) as usize) * 0x800;
        let size = (control >> 14) & 0x3;
        let (bg_width, bg_height) = match size {
            0 => (256usize, 256usize),
            1 => (512, 256),
            2 => (256, 512),
            _ => (512, 512),
        };
        let hofs = usize::from(read_le16(io, 0x10 + bg * 4)) & 0x1FF;
        let vofs = usize::from(read_le16(io, 0x12 + bg * 4)) & 0x1FF;

        for y in 0..SCREEN_HEIGHT {
            let sy = (y + vofs) % bg_height;
            for x in 0..SCREEN_WIDTH {
                let sx = (x + hofs) % bg_width;
                let Some(color_index) = text_bg_color_index(TextBgParams {
                    vram,
                    char_base,
                    screen_base,
                    x: sx,
                    y: sy,
                    bg_width,
                    color_256,
                }) else {
                    continue;
                };
                if color_index == 0 {
                    continue;
                }
                let color = bgr555_to_rgb(read_le16(palette_ram, usize::from(color_index) * 2));
                write_rgba(&mut self.framebuffer, x, y, color);
            }
        }
    }

    fn render_affine_mode(&mut self, dispcnt: u16, io: &[u8], palette_ram: &[u8], vram: &[u8]) {
        self.fill_backdrop(palette_ram);
        for bg in 2..=3 {
            if dispcnt & (1 << (8 + bg)) == 0 {
                continue;
            }
            let control = read_le16(io, 0x08 + bg * 2);
            self.render_affine_bg(bg, control, io, palette_ram, vram);
        }
    }

    fn render_affine_bg(
        &mut self,
        bg: usize,
        control: u16,
        io: &[u8],
        palette_ram: &[u8],
        vram: &[u8],
    ) {
        let char_base = (((control >> 2) & 0x3) as usize) * 0x4000;
        let wrap = control & (1 << 13) != 0;
        let screen_base = (((control >> 8) & 0x1F) as usize) * 0x800;
        let size = 128usize << ((control >> 14) & 0x3);
        let param_base = if bg == 2 { 0x20 } else { 0x30 };
        let pa = i32::from(read_i16(io, param_base));
        let pb = i32::from(read_i16(io, param_base + 2));
        let pc = i32::from(read_i16(io, param_base + 4));
        let pd = i32::from(read_i16(io, param_base + 6));
        let ref_x = read_i28_8(io, param_base + 8);
        let ref_y = read_i28_8(io, param_base + 12);

        for y in 0..SCREEN_HEIGHT {
            let affine_y = ref_y + pc * y as i32;
            let affine_x = ref_x + pb * y as i32;
            for x in 0..SCREEN_WIDTH {
                let sx = (affine_x + pa * x as i32) >> 8;
                let sy = (affine_y + pd * x as i32) >> 8;
                let (sx, sy) = if wrap {
                    (
                        sx.rem_euclid(size as i32) as usize,
                        sy.rem_euclid(size as i32) as usize,
                    )
                } else if sx < 0 || sy < 0 || sx >= size as i32 || sy >= size as i32 {
                    continue;
                } else {
                    (sx as usize, sy as usize)
                };
                let tile_x = sx / 8;
                let tile_y = sy / 8;
                let tiles_per_row = size / 8;
                let map_offset = screen_base + tile_y * tiles_per_row + tile_x;
                let tile = usize::from(vram.get(map_offset).copied().unwrap_or(0));
                let tile_offset = char_base + tile * 64 + (sy & 7) * 8 + (sx & 7);
                let color_index = vram.get(tile_offset).copied().unwrap_or(0);
                if color_index == 0 {
                    continue;
                }
                let color = bgr555_to_rgb(read_le16(palette_ram, usize::from(color_index) * 2));
                write_rgba(&mut self.framebuffer, x, y, color);
            }
        }
    }

    fn render_objs(&mut self, dispcnt: u16, palette_ram: &[u8], vram: &[u8], oam: &[u8]) {
        let one_dimensional = dispcnt & (1 << 6) != 0;
        for obj in (0..128usize).rev() {
            let base = obj * 8;
            let attr0 = read_le16(oam, base);
            let attr1 = read_le16(oam, base + 2);
            let attr2 = read_le16(oam, base + 4);
            if attr0 & (1 << 8) != 0 || attr0 & 0x0300 == 0x0200 {
                continue;
            }
            if attr0 & 0x0C00 == 0x0800 {
                continue;
            }
            let color_256 = attr0 & (1 << 13) != 0;
            let shape = (attr0 >> 14) & 0x3;
            let size = (attr1 >> 14) & 0x3;
            let Some((width, height)) = obj_dimensions(shape, size) else {
                continue;
            };
            let y = sign_obj_coord(attr0 & 0x00FF, 256);
            let x = sign_obj_coord(attr1 & 0x01FF, 512);
            let hflip = attr1 & (1 << 12) != 0;
            let vflip = attr1 & (1 << 13) != 0;
            let tile_base = usize::from(attr2 & 0x03FF);
            let palette_bank = usize::from((attr2 >> 12) & 0xF);
            for py in 0..height {
                let screen_y = y + py as i32;
                if !(0..SCREEN_HEIGHT as i32).contains(&screen_y) {
                    continue;
                }
                let src_y = if vflip { height - 1 - py } else { py };
                for px in 0..width {
                    let screen_x = x + px as i32;
                    if !(0..SCREEN_WIDTH as i32).contains(&screen_x) {
                        continue;
                    }
                    let src_x = if hflip { width - 1 - px } else { px };
                    let color_index = obj_color_index(ObjColorParams {
                        vram,
                        tile_base,
                        x: src_x,
                        y: src_y,
                        width,
                        color_256,
                        one_dimensional,
                    });
                    if color_index == 0 {
                        continue;
                    }
                    let palette_index = if color_256 {
                        usize::from(color_index)
                    } else {
                        0x100 + palette_bank * 16 + usize::from(color_index)
                    };
                    let color = bgr555_to_rgb(read_le16(palette_ram, palette_index * 2));
                    write_rgba(
                        &mut self.framebuffer,
                        screen_x as usize,
                        screen_y as usize,
                        color,
                    );
                }
            }
        }
    }
}

fn obj_dimensions(shape: u16, size: u16) -> Option<(usize, usize)> {
    match (shape, size) {
        (0, 0) => Some((8, 8)),
        (0, 1) => Some((16, 16)),
        (0, 2) => Some((32, 32)),
        (0, 3) => Some((64, 64)),
        (1, 0) => Some((16, 8)),
        (1, 1) => Some((32, 8)),
        (1, 2) => Some((32, 16)),
        (1, 3) => Some((64, 32)),
        (2, 0) => Some((8, 16)),
        (2, 1) => Some((8, 32)),
        (2, 2) => Some((16, 32)),
        (2, 3) => Some((32, 64)),
        _ => None,
    }
}

fn sign_obj_coord(value: u16, range: i32) -> i32 {
    let value = i32::from(value);
    if value >= range / 2 {
        value - range
    } else {
        value
    }
}

struct ObjColorParams<'a> {
    vram: &'a [u8],
    tile_base: usize,
    x: usize,
    y: usize,
    width: usize,
    color_256: bool,
    one_dimensional: bool,
}

fn obj_color_index(params: ObjColorParams<'_>) -> u16 {
    let ObjColorParams {
        vram,
        tile_base,
        x,
        y,
        width,
        color_256,
        one_dimensional,
    } = params;
    let tiles_per_row = if one_dimensional { width / 8 } else { 32 };
    let tile_x = x / 8;
    let tile_y = y / 8;
    let tile_number = tile_base
        + tile_y * tiles_per_row * if color_256 { 2 } else { 1 }
        + tile_x * if color_256 { 2 } else { 1 };
    let base = 0x10000 + tile_number * 32;
    let px = x & 7;
    let py = y & 7;
    if color_256 {
        u16::from(vram.get(base + py * 8 + px).copied().unwrap_or(0))
    } else {
        let byte = vram.get(base + py * 4 + px / 2).copied().unwrap_or(0);
        u16::from(if px & 1 == 0 { byte & 0x0F } else { byte >> 4 })
    }
}

struct TextBgParams<'a> {
    vram: &'a [u8],
    char_base: usize,
    screen_base: usize,
    x: usize,
    y: usize,
    bg_width: usize,
    color_256: bool,
}

fn text_bg_color_index(params: TextBgParams<'_>) -> Option<u16> {
    let TextBgParams {
        vram,
        char_base,
        screen_base,
        x,
        y,
        bg_width,
        color_256,
    } = params;
    let screen_x = x / 256;
    let screen_y = y / 256;
    let block = match (bg_width > 256, screen_y > 0, screen_x > 0) {
        (false, true, _) => 1,
        (true, false, true) => 1,
        (true, true, false) => 2,
        (true, true, true) => 3,
        _ => 0,
    };
    let tile_x = (x % 256) / 8;
    let tile_y = (y % 256) / 8;
    let entry_offset = screen_base + block * 0x800 + (tile_y * 32 + tile_x) * 2;
    let entry = read_le16(vram, entry_offset);
    let tile = usize::from(entry & 0x03FF);
    let hflip = entry & (1 << 10) != 0;
    let vflip = entry & (1 << 11) != 0;
    let palette_bank = usize::from((entry >> 12) & 0xF);
    let px = if hflip { 7 - (x & 7) } else { x & 7 };
    let py = if vflip { 7 - (y & 7) } else { y & 7 };
    if color_256 {
        let tile_offset = char_base + tile * 64 + py * 8 + px;
        Some(u16::from(vram.get(tile_offset).copied().unwrap_or(0)))
    } else {
        let tile_offset = char_base + tile * 32 + py * 4 + px / 2;
        let byte = vram.get(tile_offset).copied().unwrap_or(0);
        let nibble = if px & 1 == 0 { byte & 0x0F } else { byte >> 4 };
        Some((palette_bank * 16 + usize::from(nibble)) as u16)
    }
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
