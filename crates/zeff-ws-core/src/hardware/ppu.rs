use super::constants::{
    CYCLES_PER_SCANLINE, FRAMEBUFFER_LEN, SCANLINES_PER_FRAME, SCREEN_HEIGHT, SCREEN_WIDTH,
};

const VISIBLE_SCANLINES: u16 = SCREEN_HEIGHT as u16;
const DEFAULT_FRAME_SCANLINES: u16 = SCANLINES_PER_FRAME;
const TILE_SIZE: usize = 8;
const WSC_PALETTE_BYTES: usize = 0xFE00;
const BG_COLOR_INDEX: u8 = 64;
const DEFAULT_MONO_LUMA: [u8; 8] = [0xF0, 0xD0, 0xA8, 0x80, 0x60, 0x40, 0x28, 0x18];

#[derive(Clone, Debug)]
pub struct Ppu {
    framebuffer: Vec<u8>,
    sprite_cache: SpriteCache,
    pub frame_ready: bool,
    vcount: u16,
    line_cycles: u32,
}

#[derive(Clone, Debug)]
struct SpriteCache {
    table: [u8; 512],
    start: u8,
    count: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PpuDebugSnapshot {
    pub vcount: u16,
    pub line_cycles: u32,
    pub in_vblank: bool,
    pub frame_ready: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PpuStepEvents {
    pub completed_scanlines: u32,
    pub line_compare: bool,
    pub vblank_started: bool,
}

impl Default for Ppu {
    fn default() -> Self {
        Self::new()
    }
}

impl Ppu {
    pub fn new() -> Self {
        let mut framebuffer = vec![0; FRAMEBUFFER_LEN];
        for px in framebuffer.as_chunks_mut::<4>().0 {
            px.copy_from_slice(&[0xC8, 0xD0, 0xB0, 0xFF]);
        }
        Self {
            framebuffer,
            sprite_cache: SpriteCache::default(),
            frame_ready: false,
            vcount: 0,
            line_cycles: 0,
        }
    }

    pub fn reset(&mut self) {
        self.frame_ready = false;
        self.vcount = 0;
        self.line_cycles = 0;
        self.sprite_cache = SpriteCache::default();
        self.render_power_on_frame();
    }

    pub fn framebuffer(&self) -> &[u8] {
        &self.framebuffer
    }

    pub fn framebuffer_mut(&mut self) -> &mut [u8] {
        &mut self.framebuffer
    }

    pub fn dimensions(&self) -> (usize, usize) {
        (SCREEN_WIDTH, SCREEN_HEIGHT)
    }

    pub fn vcount(&self) -> u16 {
        self.vcount
    }

    pub fn line_cycles(&self) -> u32 {
        self.line_cycles
    }

    pub fn set_timing_state(&mut self, vcount: u16, line_cycles: u32, frame_ready: bool) {
        self.vcount = vcount & 0x00FF;
        self.line_cycles = line_cycles % CYCLES_PER_SCANLINE;
        self.frame_ready = frame_ready;
    }

    pub(crate) fn sprite_cache_state(&self) -> (&[u8; 512], u8, u8) {
        (
            &self.sprite_cache.table,
            self.sprite_cache.start,
            self.sprite_cache.count,
        )
    }

    pub(crate) fn set_sprite_cache_state(&mut self, table: [u8; 512], start: u8, count: u8) {
        self.sprite_cache = SpriteCache {
            table,
            start,
            count,
        };
    }

    pub(crate) fn cache_sprites_for_frame(&mut self, ram: &[u8], io: &[u8]) {
        self.sprite_cache = SpriteCache::from_ram(ram, io);
    }

    pub fn in_vblank(&self) -> bool {
        self.vcount >= VISIBLE_SCANLINES
    }

    pub fn step_cycles(&mut self, cycles: u32, ram: &[u8], io: &[u8]) -> PpuStepEvents {
        let mut remaining = cycles;
        let mut events = PpuStepEvents::default();
        while remaining > 0 {
            let until_next_line = CYCLES_PER_SCANLINE - self.line_cycles;
            let step = remaining.min(until_next_line);
            self.line_cycles += step;
            remaining -= step;
            if self.line_cycles == CYCLES_PER_SCANLINE {
                if self.vcount < VISIBLE_SCANLINES {
                    let mode = VideoMode::from_io(ram, io);
                    render_scanline(
                        &mut self.framebuffer,
                        self.vcount as usize,
                        ram,
                        io,
                        mode,
                        &self.sprite_cache,
                    );
                }
                self.line_cycles = 0;
                self.vcount += 1;
                events.completed_scanlines = events.completed_scanlines.saturating_add(1);
                if self.vcount == 142 {
                    self.cache_sprites_for_frame(ram, io);
                }
                if self.vcount == VISIBLE_SCANLINES {
                    events.vblank_started = true;
                }
                if self.vcount == frame_scanlines(io) {
                    self.vcount = 0;
                    self.frame_ready = true;
                }
                if self.vcount == line_compare(io) {
                    events.line_compare = true;
                }
            }
        }
        events
    }

    pub fn render_frame(&mut self, ram: &[u8], io: &[u8]) {
        self.cache_sprites_for_frame(ram, io);
        let mode = VideoMode::from_io(ram, io);
        for y in 0..SCREEN_HEIGHT {
            render_scanline(&mut self.framebuffer, y, ram, io, mode, &self.sprite_cache);
        }
    }

    pub fn debug_snapshot(&self) -> PpuDebugSnapshot {
        PpuDebugSnapshot {
            vcount: self.vcount,
            line_cycles: self.line_cycles,
            in_vblank: self.in_vblank(),
            frame_ready: self.frame_ready,
        }
    }

    fn render_power_on_frame(&mut self) {
        for px in self.framebuffer.as_chunks_mut::<4>().0 {
            px.copy_from_slice(&[0xC8, 0xD0, 0xB0, 0xFF]);
        }
    }
}

impl Default for SpriteCache {
    fn default() -> Self {
        Self {
            table: [0; 512],
            start: 0,
            count: 0,
        }
    }
}

impl SpriteCache {
    fn from_ram(ram: &[u8], io: &[u8]) -> Self {
        let mode = VideoMode::from_io(ram, io);
        let base = usize::from(io.get(0x04).copied().unwrap_or(0) & 0x3F) << 9;
        let mut table = [0; 512];
        for (i, byte) in table.iter_mut().enumerate() {
            *byte = vram_byte_by_byte(ram, mode, base + i);
        }
        Self {
            table,
            start: io.get(0x05).copied().unwrap_or(0),
            count: io.get(0x06).copied().unwrap_or(0),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct VideoMode {
    color_vdp: bool,
    color_mode: bool,
    colors_16: bool,
    packed_tiles: bool,
}

impl VideoMode {
    fn from_io(_ram: &[u8], io: &[u8]) -> Self {
        let video_mode = io.get(0x60).copied().unwrap_or(0);
        let color_vdp = io.get(0xA0).copied().unwrap_or(0) & 0x02 != 0;
        Self {
            color_vdp,
            color_mode: color_vdp && video_mode & 0x80 != 0,
            colors_16: color_vdp && video_mode & 0x40 != 0,
            packed_tiles: color_vdp && video_mode & 0x20 != 0,
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct TilePixel {
    x: usize,
    value: u8,
}

#[derive(Clone, Copy, Debug)]
struct RenderContext<'a> {
    ram: &'a [u8],
    io: &'a [u8],
    mode: VideoMode,
    color_palette: Option<&'a [[u8; 4]; 256]>,
}

#[derive(Clone, Copy, Debug)]
struct TileLayerParams {
    map_base: usize,
    scroll_x: u8,
    scroll_y: u8,
    window: WindowMode,
}

fn render_scanline(
    framebuffer: &mut [u8],
    y: usize,
    ram: &[u8],
    io: &[u8],
    mode: VideoMode,
    sprite_cache: &SpriteCache,
) {
    let display_control = io.first().copied().unwrap_or(0);
    if io.get(0x14).copied().unwrap_or(0) & 0x01 == 0 {
        let color = if mode.color_vdp && mode.color_mode {
            [0, 0, 0, 0xFF]
        } else {
            [0xFF, 0xFF, 0xFF, 0xFF]
        };
        fill_scanline(framebuffer, y, color);
        return;
    }

    let color_palette = (mode.color_vdp && mode.color_mode)
        .then(|| std::array::from_fn(|pen| color_for_pen_uncached(ram, io, mode, pen as u8)));
    let context = RenderContext {
        ram,
        io,
        mode,
        color_palette: color_palette.as_ref(),
    };

    fill_scanline(framebuffer, y, background_color(context));

    if display_control & 0x01 != 0 {
        render_background(framebuffer, y, context);
    }
    if display_control & 0x04 != 0 {
        render_sprites(framebuffer, y, context, sprite_cache, false);
    }
    if display_control & 0x02 != 0 {
        render_foreground(framebuffer, y, context);
    }
    if display_control & 0x04 != 0 {
        render_sprites(framebuffer, y, context, sprite_cache, true);
    }
}

fn fill_scanline(framebuffer: &mut [u8], y: usize, color: [u8; 4]) {
    let row = y * SCREEN_WIDTH * 4;
    for pixel in framebuffer[row..row + SCREEN_WIDTH * 4]
        .as_chunks_mut::<4>()
        .0
    {
        pixel.copy_from_slice(&color);
    }
}

fn render_background(framebuffer: &mut [u8], y: usize, context: RenderContext<'_>) {
    let map_base = background_map_base(context.io, context.mode);
    let scroll_x = context.io.get(0x10).copied().unwrap_or(0);
    let scroll_y = context.io.get(0x11).copied().unwrap_or(0);
    render_tile_layer(
        framebuffer,
        y,
        context,
        TileLayerParams {
            map_base,
            scroll_x,
            scroll_y,
            window: WindowMode::All,
        },
    );
}

fn render_foreground(framebuffer: &mut [u8], y: usize, context: RenderContext<'_>) {
    let window_mode = match (context.io.first().copied().unwrap_or(0) >> 4) & 0x03 {
        2 if line_inside_window(y, context.io, 0x08) => WindowMode::Inside(0x08),
        2 => return,
        3 if line_inside_window(y, context.io, 0x08) => WindowMode::Outside(0x08),
        3 => WindowMode::All,
        1 => return,
        _ => WindowMode::All,
    };
    let map_base = foreground_map_base(context.io, context.mode);
    let scroll_x = context.io.get(0x12).copied().unwrap_or(0);
    let scroll_y = context.io.get(0x13).copied().unwrap_or(0);
    render_tile_layer(
        framebuffer,
        y,
        context,
        TileLayerParams {
            map_base,
            scroll_x,
            scroll_y,
            window: window_mode,
        },
    );
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WindowMode {
    All,
    Inside(u16),
    Outside(u16),
}

fn render_tile_layer(
    framebuffer: &mut [u8],
    y: usize,
    context: RenderContext<'_>,
    params: TileLayerParams,
) {
    let RenderContext { ram, io, mode, .. } = context;
    let TileLayerParams {
        map_base,
        scroll_x,
        scroll_y,
        window,
    } = params;
    let map_row = ((y.wrapping_add(usize::from(scroll_y))) & 0xF8) << 2;
    let map_mask = if mode.color_mode { 0x3FFF } else { 0x1FFF };
    let start_column = usize::from(scroll_x >> 3);
    for column in 0..29 {
        let map_word = (map_base & map_mask) + map_row + ((start_column + column) & 0x1F);
        let tile_data = vram_word(ram, mode, map_word);
        let tile_number = usize::from(tile_data & 0x01FF);
        let tile_palette = ((tile_data >> 9) & 0x0F) as u8;
        let hflip = tile_data & 0x4000 != 0;
        let mut tile_line = (y.wrapping_add(usize::from(scroll_y))) & 0x07;
        if tile_data & 0x8000 != 0 {
            tile_line = 7 - tile_line;
        }

        let bank = tile_data & 0x2000 != 0;
        for pixel in tile_pixels(ram, mode, bank, tile_number, tile_line, hflip) {
            let x_offset = pixel
                .x
                .wrapping_add(column * TILE_SIZE)
                .wrapping_sub(usize::from(scroll_x & 0x07));
            if x_offset >= SCREEN_WIDTH || !window_contains_x(window, x_offset, io) {
                continue;
            }
            draw_pixel(framebuffer, y, x_offset, context, tile_palette, pixel.value);
        }
    }
}

fn render_sprites(
    framebuffer: &mut [u8],
    y: usize,
    context: RenderContext<'_>,
    sprite_cache: &SpriteCache,
    front_priority: bool,
) {
    let RenderContext { ram, io, mode, .. } = context;
    let first = usize::from(sprite_cache.start);
    let count = usize::from(sprite_cache.count);
    if count == 0 {
        return;
    }
    let window_enabled = io.first().copied().unwrap_or(0) & 0x08 != 0;

    for sprite in (first..first.saturating_add(count)).rev() {
        let entry = sprite * 4;
        if entry + 3 >= sprite_cache.table.len() {
            continue;
        }
        let tile_data =
            u16::from_le_bytes([sprite_cache.table[entry], sprite_cache.table[entry + 1]]);
        if (tile_data & 0x2000 != 0) != front_priority {
            continue;
        }
        let sprite_y = usize::from(sprite_cache.table[entry + 2]);
        let sprite_x = usize::from(sprite_cache.table[entry + 3]);
        let mut tile_line = y.wrapping_sub(sprite_y) & 0xFF;
        if tile_line >= TILE_SIZE {
            continue;
        }
        if tile_data & 0x8000 != 0 {
            tile_line = 7 - tile_line;
        }

        let clips_inside_window = tile_data & 0x1000 != 0;
        if window_enabled && !sprite_line_visible(y, io, clips_inside_window) {
            continue;
        }

        let tile_number = usize::from(tile_data & 0x01FF);
        let tile_palette = 8 + ((tile_data >> 9) & 0x07) as u8;
        let hflip = tile_data & 0x4000 != 0;
        for pixel in tile_pixels(ram, mode, false, tile_number, tile_line, hflip) {
            let x_offset = (sprite_x + pixel.x) & 0xFF;
            if x_offset >= SCREEN_WIDTH {
                continue;
            }
            if window_enabled && !sprite_pixel_visible(x_offset, io, clips_inside_window) {
                continue;
            }
            draw_pixel(framebuffer, y, x_offset, context, tile_palette, pixel.value);
        }
    }
}

fn tile_pixels(
    ram: &[u8],
    mode: VideoMode,
    bank: bool,
    tile_number: usize,
    line: usize,
    hflip: bool,
) -> impl Iterator<Item = TilePixel> {
    let mut planes = tile_planes(ram, mode, bank, tile_number, line);
    (0..TILE_SIZE).map(move |sample_x| {
        let value = extract_plane_pixel(&mut planes, mode);
        let x = if hflip { sample_x } else { 7 - sample_x };
        TilePixel { x, value }
    })
}

fn tile_planes(
    ram: &[u8],
    mode: VideoMode,
    bank: bool,
    tile_number: usize,
    line: usize,
) -> [u32; 4] {
    if mode.colors_16 {
        let tile_address = (if bank { 0x4000 } else { 0x2000 }) + tile_number * 16 + line * 2;
        if mode.packed_tiles {
            [
                (u32::from(vram_word(ram, mode, tile_address).swap_bytes()) << 16)
                    | u32::from(vram_word(ram, mode, tile_address + 1).swap_bytes()),
                0,
                0,
                0,
            ]
        } else {
            let word0 = vram_word(ram, mode, tile_address);
            let word1 = vram_word(ram, mode, tile_address + 1);
            [
                u32::from(word0 & 0x00FF),
                u32::from(word0 & 0xFF00) >> 7,
                u32::from(word1 & 0x00FF) << 2,
                u32::from(word1 & 0xFF00) >> 5,
            ]
        }
    } else {
        let tile_address = (if mode.color_mode && bank {
            0x2000
        } else {
            0x1000
        }) + tile_number * 8
            + line;
        if mode.packed_tiles {
            [u32::from(vram_word(ram, mode, tile_address)), 0, 0, 0]
        } else {
            let word = vram_word(ram, mode, tile_address);
            [
                u32::from(word & 0x00FF),
                u32::from(word & 0xFF00) >> 7,
                0,
                0,
            ]
        }
    }
}

fn extract_plane_pixel(planes: &mut [u32; 4], mode: VideoMode) -> u8 {
    if mode.packed_tiles {
        let value = if mode.colors_16 {
            (planes[0] & 0x0F) as u8
        } else {
            (planes[0] & 0x03) as u8
        };
        planes[0] >>= if mode.colors_16 { 4 } else { 2 };
        return value;
    }

    let value =
        ((planes[3] & 0x08) | (planes[2] & 0x04) | (planes[1] & 0x02) | (planes[0] & 0x01)) as u8;
    for plane in planes {
        *plane >>= 1;
    }
    value
}

fn draw_pixel(
    framebuffer: &mut [u8],
    y: usize,
    x: usize,
    context: RenderContext<'_>,
    tile_palette: u8,
    value: u8,
) {
    let RenderContext { ram, io, mode, .. } = context;
    if mode.colors_16 {
        if value == 0 {
            return;
        }
        let pen = tile_palette.wrapping_shl(4) | value;
        set_rgba(
            framebuffer,
            x,
            y,
            color_for_pen(ram, io, mode, pen, context.color_palette),
        );
        return;
    }

    if value == 0 && tile_palette & 0x04 != 0 {
        return;
    }
    let pen = if mode.color_mode {
        tile_palette.wrapping_shl(4) | value
    } else {
        tile_palette.wrapping_shl(2) | value
    };
    set_rgba(
        framebuffer,
        x,
        y,
        color_for_pen(ram, io, mode, pen, context.color_palette),
    );
}

fn set_rgba(framebuffer: &mut [u8], x: usize, y: usize, rgba: [u8; 4]) {
    let pixel = (y * SCREEN_WIDTH + x) * 4;
    framebuffer[pixel..pixel + 4].copy_from_slice(&rgba);
}

fn background_color(context: RenderContext<'_>) -> [u8; 4] {
    let RenderContext {
        ram,
        io,
        mode,
        color_palette,
    } = context;
    let bg_control = io.get(0x01).copied().unwrap_or(0);
    if mode.color_mode {
        color_for_pen(ram, io, mode, bg_control, color_palette)
    } else {
        color_for_pen(ram, io, mode, BG_COLOR_INDEX, color_palette)
    }
}

fn color_for_pen(
    ram: &[u8],
    io: &[u8],
    mode: VideoMode,
    pen: u8,
    color_palette: Option<&[[u8; 4]; 256]>,
) -> [u8; 4] {
    if let Some(color_palette) = color_palette {
        return color_palette[usize::from(pen)];
    }
    color_for_pen_uncached(ram, io, mode, pen)
}

fn color_for_pen_uncached(ram: &[u8], io: &[u8], mode: VideoMode, pen: u8) -> [u8; 4] {
    if mode.color_vdp && mode.color_mode {
        let color = vram_word_by_byte(ram, mode, WSC_PALETTE_BYTES + usize::from(pen) * 2) & 0x0FFF;
        return rgb444(color);
    }

    let luma = mono_luma_for_pen(io, pen);
    [luma, luma, luma, 0xFF]
}

fn mono_luma_for_pen(io: &[u8], pen: u8) -> u8 {
    let palette = if pen == BG_COLOR_INDEX {
        io.get(0x01).copied().unwrap_or(0) & 0x07
    } else {
        let palette_number = usize::from(pen >> 2) & 0x0F;
        let shade = usize::from(pen & 0x03);
        let palette_word = u16::from_le_bytes([
            io.get(0x20 + palette_number * 2).copied().unwrap_or(0),
            io.get(0x21 + palette_number * 2).copied().unwrap_or(0),
        ]);
        ((palette_word >> (shade * 4)) & 0x07) as u8
    };
    main_palette_luma(io, palette)
}

fn main_palette_luma(io: &[u8], index: u8) -> u8 {
    if io
        .get(0x1C..=0x1F)
        .is_some_and(|palette| palette.iter().any(|&value| value != 0))
    {
        let byte = io.get(0x1C + usize::from(index >> 1)).copied().unwrap_or(0);
        let nibble = if index & 1 == 0 {
            byte & 0x0F
        } else {
            byte >> 4
        };
        (15 - nibble.min(15)) * 17
    } else {
        DEFAULT_MONO_LUMA[usize::from(index & 0x07)]
    }
}

fn rgb444(value: u16) -> [u8; 4] {
    let r = ((value >> 8) & 0x0F) as u8 * 17;
    let g = ((value >> 4) & 0x0F) as u8 * 17;
    let b = (value & 0x0F) as u8 * 17;
    [r, g, b, 0xFF]
}

fn background_map_base(io: &[u8], mode: VideoMode) -> usize {
    let value = usize::from(io.get(0x07).copied().unwrap_or(0));
    let mask = if mode.color_vdp { 0x0F } else { 0x07 };
    (value & mask) << 10
}

fn foreground_map_base(io: &[u8], mode: VideoMode) -> usize {
    let value = usize::from(io.get(0x07).copied().unwrap_or(0));
    let mask = if mode.color_vdp { 0xF0 } else { 0x70 };
    (value & mask) << 6
}

fn line_inside_window(y: usize, io: &[u8], base: u16) -> bool {
    let top = usize::from(io.get(usize::from(base + 1)).copied().unwrap_or(0));
    let bottom = usize::from(io.get(usize::from(base + 3)).copied().unwrap_or(0));
    y >= top && y <= bottom
}

fn window_contains_x(window: WindowMode, x: usize, io: &[u8]) -> bool {
    match window {
        WindowMode::All => true,
        WindowMode::Inside(base) => {
            let left = usize::from(io.get(usize::from(base)).copied().unwrap_or(0));
            let right = usize::from(io.get(usize::from(base + 2)).copied().unwrap_or(0));
            x >= left && x <= right
        }
        WindowMode::Outside(base) => {
            let left = usize::from(io.get(usize::from(base)).copied().unwrap_or(0));
            let right = usize::from(io.get(usize::from(base + 2)).copied().unwrap_or(0));
            x < left || x > right
        }
    }
}

fn sprite_line_visible(y: usize, io: &[u8], clips_inside_window: bool) -> bool {
    let top = usize::from(io.get(0x0D).copied().unwrap_or(0));
    let bottom = usize::from(io.get(0x0F).copied().unwrap_or(0));
    if clips_inside_window {
        true
    } else {
        y >= top && y <= bottom
    }
}

fn sprite_pixel_visible(x: usize, io: &[u8], clips_inside_window: bool) -> bool {
    let left = usize::from(io.get(0x0C).copied().unwrap_or(0));
    let right = usize::from(io.get(0x0E).copied().unwrap_or(0));
    let inside = x >= left && x <= right;
    if clips_inside_window { !inside } else { inside }
}

fn vram_word(ram: &[u8], mode: VideoMode, word_index: usize) -> u16 {
    vram_word_by_byte(ram, mode, word_index * 2)
}

fn vram_word_by_byte(ram: &[u8], mode: VideoMode, byte_addr: usize) -> u16 {
    let lo = vram_byte_by_byte(ram, mode, byte_addr);
    let hi = vram_byte_by_byte(ram, mode, byte_addr.wrapping_add(1));
    u16::from_le_bytes([lo, hi])
}

fn vram_byte_by_byte(ram: &[u8], mode: VideoMode, byte_addr: usize) -> u8 {
    if ram.is_empty() {
        return 0;
    }
    if !mode.color_vdp && byte_addr >= 0x4000 {
        return 0x90;
    }
    let mask = if mode.color_vdp { 0xFFFF } else { 0x3FFF };
    ram.get(byte_addr & mask).copied().unwrap_or(0)
}

fn line_compare(io: &[u8]) -> u16 {
    u16::from(io.get(0x03).copied().unwrap_or(0))
}

fn frame_scanlines(io: &[u8]) -> u16 {
    u16::from(
        io.get(0x16)
            .copied()
            .unwrap_or(DEFAULT_FRAME_SCANLINES as u8),
    )
    .max(VISIBLE_SCANLINES)
        + 1
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hardware::constants::CYCLES_PER_FRAME;

    fn default_io() -> Vec<u8> {
        let mut io = vec![0; 0x10000];
        io[0x16] = 0x9E;
        io
    }

    fn sprite_io() -> Vec<u8> {
        let mut io = default_io();
        io[0x00] = 0x04;
        io[0x14] = 0x01;
        io[0x04] = 0x00;
        io[0x05] = 0x00;
        io[0x06] = 0x02;
        io[0x20 + 8 * 2] = 0x70;
        io[0x20 + 9 * 2] = 0x10;
        io
    }

    fn solid_2bpp_tile(ram: &mut [u8], tile: usize) {
        let base = 0x2000 + tile * 16;
        for line in 0..8 {
            ram[base + line * 2] = 0xFF;
            ram[base + line * 2 + 1] = 0x00;
        }
    }

    fn write_sprite(ram: &mut [u8], index: usize, tile: u8, attr: u8, y: u8, x: u8) {
        let base = index * 4;
        ram[base] = tile;
        ram[base + 1] = attr;
        ram[base + 2] = y;
        ram[base + 3] = x;
    }

    fn frame_pixel(framebuffer: &[u8], x: usize, y: usize) -> [u8; 4] {
        let offset = (y * SCREEN_WIDTH + x) * 4;
        [
            framebuffer[offset],
            framebuffer[offset + 1],
            framebuffer[offset + 2],
            framebuffer[offset + 3],
        ]
    }

    #[test]
    fn frame_becomes_ready_after_one_frame_of_cycles() {
        let mut ppu = Ppu::new();
        let ram = vec![0; 0x10000];
        let io = default_io();
        let _ = ppu.step_cycles(CYCLES_PER_FRAME - 1, &ram, &io);
        assert!(!ppu.frame_ready);
        let _ = ppu.step_cycles(1, &ram, &io);
        assert!(ppu.frame_ready);
        assert_eq!(ppu.vcount(), 0);
    }

    #[test]
    fn completed_scanlines_keep_their_register_state_until_next_frame() {
        let mut ppu = Ppu::new();
        let ram = vec![0; 0x10000];
        let mut io = default_io();
        io[0x01] = 7;
        io[0x14] = 1;

        let _ = ppu.step_cycles(CYCLES_PER_SCANLINE, &ram, &io);
        io[0x14] = 0;
        let _ = ppu.step_cycles(CYCLES_PER_FRAME - CYCLES_PER_SCANLINE, &ram, &io);

        assert!(ppu.frame_ready);
        assert_eq!(&ppu.framebuffer()[0..4], &[0x18, 0x18, 0x18, 0xFF]);
        let second_line = SCREEN_WIDTH * 4;
        assert_eq!(
            &ppu.framebuffer()[second_line..second_line + 4],
            &[0xFF, 0xFF, 0xFF, 0xFF]
        );
    }

    #[test]
    fn reports_vblank_start_at_first_vblank_line() {
        let mut ppu = Ppu::new();
        let ram = vec![0; 0x10000];
        let io = default_io();
        assert!(
            !ppu.step_cycles(
                CYCLES_PER_SCANLINE * (VISIBLE_SCANLINES as u32) - 1,
                &ram,
                &io
            )
            .vblank_started
        );
        assert!(ppu.step_cycles(1, &ram, &io).vblank_started);
        assert_eq!(ppu.vcount(), VISIBLE_SCANLINES);
    }

    #[test]
    fn reports_completed_scanlines_and_line_compare() {
        let mut ppu = Ppu::new();
        let ram = vec![0; 0x10000];
        let mut io = default_io();
        io[0x03] = 2;

        let events = ppu.step_cycles(CYCLES_PER_SCANLINE * 2, &ram, &io);

        assert_eq!(events.completed_scanlines, 2);
        assert!(events.line_compare);
        assert_eq!(ppu.vcount(), 2);
    }

    #[test]
    fn line_compare_does_not_wrap_to_visible_scanlines() {
        let mut ppu = Ppu::new();
        let ram = vec![0; 0x10000];
        let mut io = default_io();
        io[0x03] = 200;

        let events = ppu.step_cycles(CYCLES_PER_SCANLINE * SCANLINES_PER_FRAME as u32, &ram, &io);

        assert!(!events.line_compare);
        assert!(ppu.frame_ready);
    }

    #[test]
    fn lcd_vertical_total_controls_frame_length() {
        let mut ppu = Ppu::new();
        let ram = vec![0; 0x10000];
        let mut io = default_io();
        io[0x16] = 144;

        let _ = ppu.step_cycles(CYCLES_PER_SCANLINE * 144, &ram, &io);
        assert!(!ppu.frame_ready);
        let _ = ppu.step_cycles(CYCLES_PER_SCANLINE, &ram, &io);

        assert!(ppu.frame_ready);
        assert_eq!(ppu.vcount(), 0);
    }

    #[test]
    fn framebuffer_has_ws_dimensions() {
        let ppu = Ppu::new();
        assert_eq!(ppu.dimensions(), (SCREEN_WIDTH, SCREEN_HEIGHT));
        assert_eq!(ppu.framebuffer().len(), FRAMEBUFFER_LEN);
    }

    #[test]
    fn earlier_sprites_have_priority_over_later_sprites() {
        let mut ppu = Ppu::new();
        let mut ram = vec![0; 0x10000];
        let io = sprite_io();
        solid_2bpp_tile(&mut ram, 0);
        solid_2bpp_tile(&mut ram, 1);
        write_sprite(&mut ram, 0, 0, 0x00, 0, 0);
        write_sprite(&mut ram, 1, 1, 0x02, 0, 0);

        ppu.render_frame(&ram, &io);

        assert_eq!(
            frame_pixel(ppu.framebuffer(), 0, 0),
            [0x18, 0x18, 0x18, 0xFF]
        );
    }

    #[test]
    fn scanline_rendering_uses_latched_sprite_table() {
        let mut ppu = Ppu::new();
        let mut ram = vec![0; 0x10000];
        let io = sprite_io();
        solid_2bpp_tile(&mut ram, 0);
        write_sprite(&mut ram, 0, 0, 0x00, 0, 0);
        ppu.cache_sprites_for_frame(&ram, &io);
        write_sprite(&mut ram, 0, 0, 0x00, 0, 32);

        let _ = ppu.step_cycles(CYCLES_PER_SCANLINE, &ram, &io);

        assert_eq!(
            frame_pixel(ppu.framebuffer(), 0, 0),
            [0x18, 0x18, 0x18, 0xFF]
        );
        assert_ne!(
            frame_pixel(ppu.framebuffer(), 32, 0),
            [0x18, 0x18, 0x18, 0xFF]
        );
    }
}
