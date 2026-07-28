use super::effects::Mosaic;
use super::obj::{
    ObjColorParams, obj_affine_params, obj_color_index, obj_dimensions, obj_y_coord, sign_obj_coord,
};
use super::read_le16;
use crate::hardware::constants::{SCREEN_HEIGHT, SCREEN_WIDTH};

#[derive(Clone)]
pub(super) struct Windows {
    win0_enabled: bool,
    win1_enabled: bool,
    obj_window_enabled: bool,
    win0: WindowRect,
    win1: WindowRect,
    winin: u16,
    winout: u16,
    obj_window_mask: ObjWindowMask,
}

impl Windows {
    pub(super) fn disabled() -> Self {
        Self {
            win0_enabled: false,
            win1_enabled: false,
            obj_window_enabled: false,
            win0: WindowRect::empty(),
            win1: WindowRect::empty(),
            winin: 0,
            winout: 0,
            obj_window_mask: ObjWindowMask::Disabled,
        }
    }

    pub(super) fn from_io(
        dispcnt: u16,
        io: &[u8],
        vram: &[u8],
        oam: &[u8],
        mosaic: Mosaic,
    ) -> Self {
        if !windows_enabled(dispcnt) {
            return Self::disabled();
        }

        let obj_window_mask = if dispcnt & (1 << 15) != 0 {
            ObjWindowMask::Full(build_obj_window_mask(dispcnt, vram, oam, mosaic))
        } else {
            ObjWindowMask::Disabled
        };
        Self::from_io_with_obj_mask(dispcnt, io, obj_window_mask)
    }

    pub(super) fn from_io_scanline(
        dispcnt: u16,
        io: &[u8],
        vram: &[u8],
        oam: &[u8],
        mosaic: Mosaic,
        y: usize,
    ) -> Self {
        if !windows_enabled(dispcnt) {
            return Self::disabled();
        }

        let obj_window_mask = if dispcnt & (1 << 15) != 0 {
            ObjWindowMask::Line {
                y,
                pixels: build_obj_window_line_mask(dispcnt, vram, oam, mosaic, y),
            }
        } else {
            ObjWindowMask::Disabled
        };
        Self::from_io_with_obj_mask(dispcnt, io, obj_window_mask)
    }

    fn from_io_with_obj_mask(dispcnt: u16, io: &[u8], obj_window_mask: ObjWindowMask) -> Self {
        Self {
            win0_enabled: dispcnt & (1 << 13) != 0,
            win1_enabled: dispcnt & (1 << 14) != 0,
            obj_window_enabled: dispcnt & (1 << 15) != 0,
            win0: WindowRect::from_io(io, 0x40, 0x44),
            win1: WindowRect::from_io(io, 0x42, 0x46),
            winin: read_le16(io, 0x48),
            winout: read_le16(io, 0x4A),
            obj_window_mask,
        }
    }

    pub(super) fn allows_bg(&self, bg: usize, x: usize, y: usize) -> bool {
        if !self.enabled() {
            return true;
        }
        self.control(x, y) & (1 << bg) != 0
    }

    pub(super) fn allows_obj(&self, x: usize, y: usize) -> bool {
        if !self.enabled() {
            return true;
        }
        self.control(x, y) & (1 << 4) != 0
    }

    pub(super) fn allows_effect(&self, x: usize, y: usize) -> bool {
        if !self.enabled() {
            return true;
        }
        self.control(x, y) & (1 << 5) != 0
    }

    fn enabled(&self) -> bool {
        self.win0_enabled || self.win1_enabled || self.obj_window_enabled
    }

    fn control(&self, x: usize, y: usize) -> u16 {
        if self.win0_enabled && self.win0.contains(x, y) {
            self.winin & 0x3F
        } else if self.win1_enabled && self.win1.contains(x, y) {
            (self.winin >> 8) & 0x3F
        } else if self.obj_window_enabled && self.obj_window_mask.contains(x, y) {
            (self.winout >> 8) & 0x3F
        } else {
            self.winout & 0x3F
        }
    }
}

fn windows_enabled(dispcnt: u16) -> bool {
    dispcnt & ((1 << 13) | (1 << 14) | (1 << 15)) != 0
}

#[derive(Clone)]
#[allow(clippy::large_enum_variant)]
enum ObjWindowMask {
    Disabled,
    Full(Vec<bool>),
    Line {
        y: usize,
        pixels: [bool; SCREEN_WIDTH],
    },
}

impl ObjWindowMask {
    fn contains(&self, x: usize, y: usize) -> bool {
        match self {
            Self::Disabled => false,
            Self::Full(mask) => mask.get(y * SCREEN_WIDTH + x).copied().unwrap_or(false),
            Self::Line { y: line_y, pixels } => {
                *line_y == y && pixels.get(x).copied().unwrap_or(false)
            }
        }
    }
}

fn build_obj_window_mask(dispcnt: u16, vram: &[u8], oam: &[u8], mosaic: Mosaic) -> Vec<bool> {
    let mut mask = vec![false; SCREEN_WIDTH * SCREEN_HEIGHT];
    visit_obj_window_pixels(dispcnt, vram, oam, mosaic, None, |x, y| {
        mask[y * SCREEN_WIDTH + x] = true;
    });
    mask
}

fn build_obj_window_line_mask(
    dispcnt: u16,
    vram: &[u8],
    oam: &[u8],
    mosaic: Mosaic,
    line_y: usize,
) -> [bool; SCREEN_WIDTH] {
    let mut mask = [false; SCREEN_WIDTH];
    visit_obj_window_pixels(dispcnt, vram, oam, mosaic, Some(line_y), |x, _| {
        mask[x] = true;
    });
    mask
}

fn visit_obj_window_pixels(
    dispcnt: u16,
    vram: &[u8],
    oam: &[u8],
    mosaic: Mosaic,
    line_y: Option<usize>,
    mut mark: impl FnMut(usize, usize),
) {
    let one_dimensional = dispcnt & (1 << 6) != 0;
    for obj in (0..128usize).rev() {
        let base = obj * 8;
        let attr0 = read_le16(oam, base);
        let attr1 = read_le16(oam, base + 2);
        let attr2 = read_le16(oam, base + 4);
        let mode = (attr0 >> 10) & 0x3;
        if mode != 2 {
            continue;
        }
        let affine = attr0 & (1 << 8) != 0;
        if !affine && attr0 & (1 << 9) != 0 {
            continue;
        }
        let color_256 = attr0 & (1 << 13) != 0;
        let shape = (attr0 >> 14) & 0x3;
        let size = (attr1 >> 14) & 0x3;
        let Some((width, height)) = obj_dimensions(shape, size) else {
            continue;
        };
        let double_size = affine && attr0 & (1 << 9) != 0;
        let draw_width = if double_size { width * 2 } else { width };
        let draw_height = if double_size { height * 2 } else { height };
        let y = obj_y_coord(attr0 & 0x00FF);
        let x = sign_obj_coord(attr1 & 0x01FF, 512);
        let hflip = !affine && attr1 & (1 << 12) != 0;
        let vflip = !affine && attr1 & (1 << 13) != 0;
        let tile_base = usize::from(attr2 & 0x03FF);
        let affine_params = affine.then(|| obj_affine_params(oam, (attr1 >> 9) & 0x1F));
        let use_mosaic = attr0 & (1 << 12) != 0;
        for py in 0..draw_height {
            let screen_y = y + py as i32;
            if !(0..SCREEN_HEIGHT as i32).contains(&screen_y) {
                continue;
            }
            if let Some(line_y) = line_y
                && screen_y as usize != line_y
            {
                continue;
            }
            for px in 0..draw_width {
                let screen_x = x + px as i32;
                if !(0..SCREEN_WIDTH as i32).contains(&screen_x) {
                    continue;
                }
                let (sample_px, sample_py) = mosaic.obj_sample(px, py, use_mosaic);
                let (src_x, src_y) = if let Some((pa, pb, pc, pd)) = affine_params {
                    let rel_x = sample_px as i32 - draw_width as i32 / 2;
                    let rel_y = sample_py as i32 - draw_height as i32 / 2;
                    let src_x = ((pa * rel_x + pb * rel_y) >> 8) + width as i32 / 2;
                    let src_y = ((pc * rel_x + pd * rel_y) >> 8) + height as i32 / 2;
                    if !(0..width as i32).contains(&src_x) || !(0..height as i32).contains(&src_y) {
                        continue;
                    }
                    (src_x as usize, src_y as usize)
                } else {
                    let src_x = if hflip {
                        width - 1 - sample_px
                    } else {
                        sample_px
                    };
                    let src_y = if vflip {
                        height - 1 - sample_py
                    } else {
                        sample_py
                    };
                    (src_x, src_y)
                };
                let color_index = obj_color_index(ObjColorParams {
                    vram,
                    tile_base,
                    x: src_x,
                    y: src_y,
                    width,
                    color_256,
                    one_dimensional,
                    bitmap_obj_tiles: dispcnt & 0x7 >= 3,
                });
                if color_index != 0 {
                    mark(screen_x as usize, screen_y as usize);
                }
            }
        }
    }
}

#[derive(Clone, Copy)]
struct WindowRect {
    left: usize,
    right: usize,
    top: usize,
    bottom: usize,
}

impl WindowRect {
    fn empty() -> Self {
        Self {
            left: 0,
            right: 0,
            top: 0,
            bottom: 0,
        }
    }

    fn from_io(io: &[u8], horizontal_offset: usize, vertical_offset: usize) -> Self {
        let horizontal = read_le16(io, horizontal_offset);
        let vertical = read_le16(io, vertical_offset);
        Self {
            left: usize::from((horizontal >> 8) as u8).min(SCREEN_WIDTH),
            right: usize::from(horizontal as u8).min(SCREEN_WIDTH),
            top: usize::from((vertical >> 8) as u8).min(SCREEN_HEIGHT),
            bottom: usize::from(vertical as u8).min(SCREEN_HEIGHT),
        }
    }

    fn contains(self, x: usize, y: usize) -> bool {
        x >= self.left && x < self.right && y >= self.top && y < self.bottom
    }
}
