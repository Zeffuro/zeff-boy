use super::rows::TextRowWork;
use super::*;

#[derive(Clone)]
struct TextLine {
    ppu: Ppu,
    priorities: [u8; SCREEN_WIDTH],
    backgrounds: [u8; SCREEN_WIDTH],
    second_priorities: [u8; SCREEN_WIDTH],
    second_layers: [Layer; SCREEN_WIDTH],
    second_colors: [u16; SCREEN_WIDTH],
    layers: [Layer; SCREEN_WIDTH],
    colors: [u16; SCREEN_WIDTH],
}

impl TextLine {
    fn new(y: usize, palette: &[u8], effects: ColorEffects, windows: &Windows) -> Self {
        let mut line = Self {
            ppu: Ppu::new(),
            priorities: [4; SCREEN_WIDTH],
            backgrounds: [4; SCREEN_WIDTH],
            second_priorities: [4; SCREEN_WIDTH],
            second_layers: [Layer::Backdrop; SCREEN_WIDTH],
            second_colors: [0; SCREEN_WIDTH],
            layers: [Layer::Backdrop; SCREEN_WIDTH],
            colors: [0; SCREEN_WIDTH],
        };
        line.ppu.framebuffer.fill(0xA5);
        line.ppu.fill_backdrop_line(
            y,
            palette,
            effects,
            windows,
            &mut line.layers,
            &mut line.colors,
        );
        line
    }

    fn scalar(&mut self, fixture: &Fixture, backgrounds: &[(u16, usize, u16)], windows: &Windows) {
        for &(priority, bg, control) in backgrounds {
            self.ppu.render_text_bg_line(
                bg,
                control,
                &fixture.io,
                &fixture.palette,
                &fixture.vram,
                fixture.y,
                &mut self.priorities,
                &mut self.backgrounds,
                &mut self.second_priorities,
                &mut self.second_layers,
                &mut self.second_colors,
                &mut self.layers,
                &mut self.colors,
                priority as u8,
                ColorEffects::from_io(&fixture.io),
                windows,
                Mosaic::from_io(&fixture.io),
            );
        }
    }

    fn candidate(
        &mut self,
        fixture: &Fixture,
        backgrounds: &[(u16, usize, u16)],
        windows: &Windows,
        work: &mut TextRowWork,
    ) -> bool {
        self.ppu.try_render_text_rows_line(
            backgrounds,
            &fixture.io,
            &fixture.palette,
            &fixture.vram,
            fixture.y,
            &mut self.priorities,
            &mut self.backgrounds,
            &mut self.second_priorities,
            &mut self.second_layers,
            &mut self.second_colors,
            &mut self.layers,
            &mut self.colors,
            ColorEffects::from_io(&fixture.io),
            windows,
            work,
        )
    }

    fn assert_same(&self, other: &Self) {
        assert_eq!(
            self.ppu
                .framebuffer()
                .iter()
                .zip(other.ppu.framebuffer())
                .position(|(left, right)| left != right),
            None,
            "framebuffer mismatch"
        );
        assert_eq!(self.ppu.state(), other.ppu.state());
        assert_eq!(self.priorities, other.priorities);
        assert_eq!(self.backgrounds, other.backgrounds);
        assert_eq!(self.second_priorities, other.second_priorities);
        assert_eq!(self.second_colors, other.second_colors);
        assert_eq!(self.colors, other.colors);
        assert_eq!(
            self.second_layers.map(layer_id),
            other.second_layers.map(layer_id)
        );
        assert_eq!(self.layers.map(layer_id), other.layers.map(layer_id));
    }
}

struct Fixture {
    io: [u8; 0x60],
    palette: Vec<u8>,
    vram: Vec<u8>,
    oam: Vec<u8>,
    y: usize,
}

impl Fixture {
    fn opaque() -> Self {
        let mut fixture = Self {
            io: [0; 0x60],
            palette: vec![0; 0x400],
            vram: vec![0x11; 0x18000],
            oam: vec![0; 0x400],
            y: 0,
        };
        write16(&mut fixture.io, 0, 0x0F00);
        for bg in 0..4 {
            write16(
                &mut fixture.io,
                0x08 + bg * 2,
                ((24 + bg) as u16) << 8 | bg as u16,
            );
        }
        for color in 0..512 {
            write16(
                &mut fixture.palette,
                color * 2,
                (color as u16).wrapping_mul(0x6C9D),
            );
        }
        for obj in 0..128 {
            write16(&mut fixture.oam, obj * 8, 1 << 9);
        }
        fixture
    }

    fn layers(&self, enabled: [bool; 4]) -> Vec<(u16, usize, u16)> {
        let dispcnt = read_le16(&self.io, 0);
        let count = if dispcnt & 7 == 0 { 4 } else { 2 };
        let mut layers = Vec::new();
        for (bg, enabled) in enabled.into_iter().enumerate().take(count) {
            if enabled && dispcnt & (1 << (8 + bg)) != 0 {
                let control = read_le16(&self.io, 0x08 + bg * 2);
                layers.push((control & 3, bg, control));
            }
        }
        layers.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| b.1.cmp(&a.1)));
        layers
    }

    fn windows(&self) -> Windows {
        Windows::from_io_scanline(
            read_le16(&self.io, 0),
            &self.io,
            &self.vram,
            &self.oam,
            Mosaic::from_io(&self.io),
            self.y,
        )
    }

    fn compare(&self, enabled: [bool; 4]) -> TextRowWork {
        let layers = self.layers(enabled);
        let windows = self.windows();
        let mut scalar = TextLine::new(
            self.y,
            &self.palette,
            ColorEffects::from_io(&self.io),
            &windows,
        );
        let mut candidate = scalar.clone();
        let mut work = TextRowWork::default();
        let admitted = candidate.candidate(self, &layers, &windows, &mut work);
        assert_eq!(
            admitted,
            !layers.is_empty()
                && layers
                    .iter()
                    .all(|&(_, _, control)| control & (1 << 6) == 0)
        );
        if !admitted {
            scalar.assert_same(&candidate);
            candidate.scalar(self, &layers, &windows);
        }
        scalar.scalar(self, &layers, &windows);
        scalar.assert_same(&candidate);
        work
    }
}

fn layer_id(layer: Layer) -> usize {
    match layer {
        Layer::Bg(bg) => bg,
        Layer::Obj => 4,
        Layer::Backdrop => 5,
    }
}

fn write16(bytes: &mut [u8], offset: usize, value: u16) {
    bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}

fn next(seed: &mut u32) -> u32 {
    *seed ^= *seed << 13;
    *seed ^= *seed >> 17;
    *seed ^= *seed << 5;
    *seed
}

#[test]
fn text_rows_decoder_matches_scalar_at_map_tile_and_slice_boundaries() {
    let mut fixture = Fixture::opaque();
    let mut seed = 0x5EED_7141;
    for byte in &mut fixture.vram {
        *byte = next(&mut seed) as u8;
    }
    for size in 0..4 {
        for char_base in 0..4 {
            for color_256 in [false, true] {
                let control = size << 14 | 31 << 8 | char_base << 2 | u16::from(color_256) << 7;
                let params = TextBgParams::new(control, &fixture.io, 0);
                for y in [0, 1, 7, 8, 247, 255, 256, 263, 504, 511] {
                    for x in [0, 8, 248, 256, 264, 504] {
                        for length in [0, 1, 0xFFFF, 0x10000, 0x17FFF, 0x18000] {
                            let vram = &fixture.vram[..length];
                            let colors = params.color_indices_row(vram, x, y);
                            for (pixel, &color) in colors.iter().enumerate() {
                                assert_eq!(
                                    u16::from(color),
                                    params.color_index(vram, x + pixel, y).unwrap_or(0),
                                    "control={control:04X} x={x} y={y} length={length} pixel={pixel}"
                                );
                            }
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn text_rows_match_scalar_randomized_modes_windows_effects_and_debug_layers() {
    let mut fixture = Fixture::opaque();
    let mut seed = 0xC01D_7839;
    for byte in &mut fixture.vram {
        *byte = next(&mut seed) as u8;
    }
    for case in 0..768 {
        for byte in &mut fixture.io {
            *byte = next(&mut seed) as u8;
        }
        for byte in &mut fixture.palette {
            *byte = next(&mut seed) as u8;
        }
        let dispcnt = (case & 1) | 0x1F00 | ((case & 7) << 13);
        write16(&mut fixture.io, 0, dispcnt);
        fixture.y = (next(&mut seed) as usize) % SCREEN_HEIGHT;
        for bg in 0..4 {
            let control = read_le16(&fixture.io, 0x08 + bg * 2) & !(1 << 6);
            write16(&mut fixture.io, 0x08 + bg * 2, control);
        }
        for obj in 0..4 {
            let x = ((next(&mut seed) as u16) & 0x1FF) | (3 << 14);
            write16(&mut fixture.oam, obj * 8, fixture.y as u16 | (2 << 10));
            write16(&mut fixture.oam, obj * 8 + 2, x);
            write16(&mut fixture.oam, obj * 8 + 4, next(&mut seed) as u16);
        }
        fixture.vram[(next(&mut seed) as usize) % 0x18000] = next(&mut seed) as u8;
        let enabled = std::array::from_fn(|bg| case & (1 << (bg + 3)) == 0);
        fixture.compare(enabled);
    }
}

#[test]
fn text_rows_preserve_raw_lower_colors_backdrop_and_effect_coefficients() {
    let mut fixture = Fixture::opaque();
    write16(&mut fixture.palette, 0, 0xA53C);
    for effect in 0..4 {
        for first in [0, 1, 2, 4, 8, 0x20, 0x3F] {
            for second in [0, 1, 2, 4, 8, 0x20, 0x3F] {
                for coefficient in [0, 1, 7, 16, 31] {
                    write16(&mut fixture.io, 0x50, first | effect << 6 | second << 8);
                    write16(&mut fixture.io, 0x52, coefficient | (31 - coefficient) << 8);
                    write16(&mut fixture.io, 0x54, coefficient);
                    fixture.compare([true; 4]);
                    fixture.compare([false, false, false, true]);
                }
            }
        }
    }
}

#[test]
fn text_rows_skip_covered_layers_and_compose_once() {
    let fixture = Fixture::opaque();
    let work = fixture.compare([true; 4]);
    assert_eq!(work.decoded_rows, 60);
    assert_eq!(work.composed_pixels, SCREEN_WIDTH);
    assert_eq!(4 * SCREEN_WIDTH / work.composed_pixels, 4);
}

#[test]
fn text_rows_preserve_transparent_zero_priority_ties_and_scroll_wraps() {
    let mut fixture = Fixture::opaque();
    fixture.vram[..0x200].fill(0);
    for bg in 0..4 {
        write16(
            &mut fixture.io,
            0x08 + bg * 2,
            (24 + bg as u16) << 8 | 3 << 14,
        );
        for tile in 0..1024 {
            let entry = if tile & 1 == 0 { 0xF000 } else { 0xFC20 };
            write16(&mut fixture.vram, (24 + bg) * 0x800 + tile * 2, entry);
        }
    }
    for offset in [0, 1, 7, 8, 247, 255, 256, 263, 504, 511] {
        for bg in 0..4 {
            write16(&mut fixture.io, 0x10 + bg * 4, offset);
            write16(&mut fixture.io, 0x12 + bg * 4, offset);
        }
        for y in [0, 1, 7, 8, 159] {
            fixture.y = y;
            fixture.compare([true; 4]);
        }
    }
}

#[test]
fn text_rows_reject_mosaic_without_touching_outputs() {
    let mut fixture = Fixture::opaque();
    write16(&mut fixture.io, 0x4C, 0x00FF);
    for bg in 0..4 {
        let offset = 0x08 + bg * 2;
        let control = read_le16(&fixture.io, offset);
        write16(&mut fixture.io, offset, control | 1 << 6);
        let work = fixture.compare([true; 4]);
        assert_eq!(work.decoded_rows, 0);
        assert_eq!(work.composed_pixels, 0);
        write16(&mut fixture.io, offset, control);
    }
    fixture.compare([false; 4]);
}

#[test]
fn text_rows_match_full_mode1_affine_and_obj_composition() {
    let mut fixture = Fixture::opaque();
    fixture.y = 5;
    fixture.vram.fill(0);

    write16(&mut fixture.io, 0, 1 | 0x0700 | 0x1000);
    write16(&mut fixture.io, 0x08, 1 | 24 << 8);
    write16(&mut fixture.io, 0x0A, 3 | 1 << 2 | 25 << 8);
    write16(&mut fixture.io, 0x0C, 2 << 2 | 26 << 8);
    write16(&mut fixture.io, 0x20, 0x0100);
    write16(&mut fixture.io, 0x26, 0x0100);
    write16(
        &mut fixture.io,
        0x50,
        1 << 2 | 1 << 6 | 1 << (8 + 2) | 1 << (8 + 4),
    );
    write16(&mut fixture.io, 0x52, 12 | 4 << 8);

    fixture.vram[..32].fill(0x11);
    fixture.vram[0x4000..0x4020].fill(0x22);
    fixture.vram[0x8000..0x8040].fill(3);
    fixture.vram[0x10000..0x10020].fill(0x11);
    write16(&mut fixture.palette, 2, 0x001F);
    write16(&mut fixture.palette, 4, 0x03E0);
    write16(&mut fixture.palette, 6, 0x7C00);
    write16(&mut fixture.palette, 0x202, 0x001F);

    write16(&mut fixture.oam, 0, fixture.y as u16 | 1 << 10);
    write16(&mut fixture.oam, 2, 0);
    write16(&mut fixture.oam, 4, 1 << 10);
    write16(&mut fixture.oam, 8, fixture.y as u16 | 1 << 10);
    write16(&mut fixture.oam, 10, 16);
    write16(&mut fixture.oam, 12, 0);

    let render = |enabled| {
        let mut ppu = Ppu::new();
        rows::with_test_enabled(enabled, || {
            ppu.render_scanline(
                fixture.y,
                &fixture.io,
                &fixture.palette,
                &fixture.vram,
                &fixture.oam,
            );
        });
        ppu
    };
    let scalar = render(false);
    let candidate = render(true);

    assert_eq!(candidate.framebuffer(), scalar.framebuffer());
    assert_eq!(candidate.state(), scalar.state());
    let pixel = |ppu: &Ppu, x| {
        let offset = (fixture.y * SCREEN_WIDTH + x) * 4;
        <[u8; 4]>::try_from(&ppu.framebuffer()[offset..offset + 4]).unwrap()
    };
    assert_eq!(pixel(&candidate, 0), [57, 0, 189, 0xFF]);
    assert_eq!(pixel(&candidate, 16), [189, 0, 57, 0xFF]);
    assert_eq!(pixel(&candidate, 32), [0, 0, 0xFF, 0xFF]);
}
