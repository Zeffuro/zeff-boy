#[cfg(test)]
use std::cell::Cell;

use super::super::{bgr555_to_rgb, read_le16};
use super::{ColorEffects, Layer, Ppu, SCREEN_WIDTH, TextBgParams, Windows};

#[cfg(test)]
thread_local! {
    static TEST_ENABLED: Cell<Option<bool>> = const { Cell::new(None) };
}

#[cfg(test)]
pub(super) fn enabled_for_test() -> bool {
    TEST_ENABLED.with(Cell::get).unwrap_or(true)
}

#[cfg(test)]
pub(super) fn with_test_enabled<T>(enabled: bool, test: impl FnOnce() -> T) -> T {
    struct Reset(Option<bool>);

    impl Drop for Reset {
        fn drop(&mut self) {
            TEST_ENABLED.with(|state| state.set(self.0));
        }
    }

    let previous = TEST_ENABLED.with(|state| state.replace(Some(enabled)));
    let _reset = Reset(previous);
    test()
}

#[cfg(test)]
#[derive(Default)]
pub(super) struct TextRowWork {
    pub decoded_rows: usize,
    pub composed_pixels: usize,
}

impl Ppu {
    pub(super) fn try_render_text_rows_line(
        &mut self,
        layers: &[(u16, usize, u16)],
        io: &[u8],
        palette_ram: &[u8],
        vram: &[u8],
        y: usize,
        bg_priorities: &mut [u8; SCREEN_WIDTH],
        bg_layers: &mut [u8; SCREEN_WIDTH],
        bg_second_priorities: &mut [u8; SCREEN_WIDTH],
        bg_second_layers: &mut [Layer; SCREEN_WIDTH],
        bg_second_colors: &mut [u16; SCREEN_WIDTH],
        pixel_layers: &mut [Layer; SCREEN_WIDTH],
        pixel_colors: &mut [u16; SCREEN_WIDTH],
        effects: ColorEffects,
        windows: &Windows,
        #[cfg(test)] work: &mut TextRowWork,
    ) -> bool {
        if layers.is_empty()
            || layers
                .iter()
                .any(|&(_, _, control)| control & (1 << 6) != 0)
        {
            return false;
        }

        debug_assert!(bg_priorities.iter().all(|&priority| priority == 4));
        let controls = windows.scanline_controls(y);
        let mut missing_second = SCREEN_WIDTH;
        for &(priority, bg, control) in layers.iter().rev() {
            let params = TextBgParams::new(control, io, bg);
            let sy = (y + params.vofs) & (params.height - 1);
            let bg_mask = 1 << bg;
            let mut x = 0;
            while x < SCREEN_WIDTH {
                let sx = (x + params.hofs) & (params.width - 1);
                let length = (8 - (sx & 7)).min(SCREEN_WIDTH - x);
                let end = x + length;
                let first = (x..end).find(|&pixel| {
                    controls[pixel] & bg_mask != 0 && bg_second_priorities[pixel] == 4
                });
                if let Some(first) = first {
                    let colors = params.color_indices_row(vram, sx, sy);
                    #[cfg(test)]
                    {
                        work.decoded_rows += 1;
                    }
                    for pixel in first..end {
                        if controls[pixel] & bg_mask == 0 || bg_second_priorities[pixel] != 4 {
                            continue;
                        }
                        let color_index = colors[(sx & 7) + pixel - x];
                        if color_index == 0 {
                            continue;
                        }
                        let color = read_le16(palette_ram, usize::from(color_index) * 2);
                        if bg_priorities[pixel] == 4 {
                            bg_second_colors[pixel] = pixel_colors[pixel];
                            bg_second_layers[pixel] = pixel_layers[pixel];
                            bg_priorities[pixel] = priority as u8;
                            bg_layers[pixel] = bg as u8;
                            pixel_layers[pixel] = Layer::Bg(bg);
                            pixel_colors[pixel] = color;
                        } else {
                            bg_second_priorities[pixel] = priority as u8;
                            bg_second_layers[pixel] = Layer::Bg(bg);
                            bg_second_colors[pixel] = color;
                            missing_second -= 1;
                        }
                    }
                }
                x = end;
            }
            if missing_second == 0 {
                break;
            }
        }

        let start = y * SCREEN_WIDTH * 4;
        let output = &mut self.framebuffer[start..start + SCREEN_WIDTH * 4];
        for (x, pixel) in output.as_chunks_mut::<4>().0.iter_mut().enumerate() {
            if bg_priorities[x] == 4 {
                continue;
            }
            let color = effects.apply_pixel(
                pixel_colors[x],
                pixel_layers[x],
                Some((bg_second_colors[x], bg_second_layers[x])),
                false,
                controls[x] & (1 << 5) != 0,
            );
            let [r, g, b] = bgr555_to_rgb(color);
            *pixel = [r, g, b, 0xFF];
            #[cfg(test)]
            {
                work.composed_pixels += 1;
            }
        }
        true
    }
}

impl TextBgParams {
    pub(super) fn color_indices_row(&self, vram: &[u8], x: usize, y: usize) -> [u8; 8] {
        let entry = read_le16(vram, self.entry_offset(x, y));
        let tile = usize::from(entry & 0x03FF);
        let py = if entry & (1 << 11) == 0 {
            y & 7
        } else {
            7 - (y & 7)
        };
        let mut colors = [0; 8];
        if self.color_256 {
            let offset = self.char_base + tile * 64 + py * 8;
            for (pixel, color) in colors.iter_mut().enumerate() {
                *color = vram.get(offset + pixel).copied().unwrap_or(0);
            }
        } else {
            let offset = self.char_base + tile * 32 + py * 4;
            let palette = ((entry >> 8) & 0xF0) as u8;
            for (pixel, pair) in colors.as_chunks_mut::<2>().0.iter_mut().enumerate() {
                let byte = vram.get(offset + pixel).copied().unwrap_or(0);
                let low = byte & 0x0F;
                let high = byte >> 4;
                pair[0] = if low == 0 { 0 } else { palette | low };
                pair[1] = if high == 0 { 0 } else { palette | high };
            }
        }
        if entry & (1 << 10) != 0 {
            colors.reverse();
        }
        colors
    }
}
