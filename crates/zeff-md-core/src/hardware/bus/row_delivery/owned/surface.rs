mod rollback_memory;
use super::*;
mod state;
#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::hardware::bus) struct Surface {
    field: u64,
    pixels: Box<[u32]>,
    seen: Box<[bool]>,
    widths: [u16; 240],
    width: u16,
    height: u16,
    mode5: Option<bool>,
    mixed: bool,
    active: bool,
    closed: bool,
    published: bool,
    count: u64,
    incomplete: u64,
    last: Option<(usize, usize, Box<[u32]>)>,
}

impl Surface {
    pub fn new() -> Self {
        Self {
            field: 0,
            pixels: vec![0; 320 * 240].into_boxed_slice(),
            seen: vec![false; 320 * 240].into_boxed_slice(),
            widths: [0; 240],
            width: 0,
            height: 0,
            mode5: None,
            mixed: false,
            active: false,
            closed: false,
            published: false,
            count: 0,
            incomplete: 0,
            last: None,
        }
    }

    pub fn push(&mut self, pixel: Option<OwnedPixel>) {
        let Some(pixel) = pixel else {
            return;
        };
        let tag = pixel;
        if tag.field == 0 {
            return;
        }
        if tag.field < self.field {
            if !self.closed {
                self.mixed = true;
            }
            return;
        }
        if tag.field != self.field {
            self.finish_interval();
            self.field = tag.field;
            self.pixels.fill(0);
            self.seen.fill(false);
            self.widths.fill(0);
            self.width = 0;
            self.height = 0;
            self.mode5 = None;
            self.mixed = false;
            self.active = false;
            self.closed = false;
            self.published = false;
        }
        if self.closed {
            return;
        }
        if !tag.vertical_active {
            if self.active {
                self.finish_interval();
                self.active = false;
            }
            return;
        }
        self.active = true;
        if self.mode5.is_some_and(|mode| mode != tag.mode5) || tag.mode5 != pixel.output_mode5 {
            self.mixed = true;
        }
        self.mode5 = Some(tag.mode5);
        let width = if tag.wide { 320 } else { 256 };
        if self.width != 0 && (self.width != width || self.height != tag.height) {
            self.mixed = true;
        }
        self.width = width;
        self.height = tag.height;
        if tag.horizontal_active && !tag.rectangular() {
            self.mixed = true;
        }
        if !tag.visible() {
            return;
        }
        let row = usize::from(tag.y);
        self.widths[row] = width;
        let index = row * 320 + usize::from(tag.x);
        self.pixels[index] = pixel.rgb;
        self.seen[index] = true;
    }

    fn complete(&self) -> bool {
        !self.mixed
            && matches!(self.width, 256 | 320)
            && matches!(
                (self.mode5, self.height),
                (Some(false), 192) | (Some(true), 224 | 240)
            )
            && self.widths.iter().enumerate().all(|(row, width)| {
                if row >= usize::from(self.height) {
                    *width == 0
                } else {
                    *width == self.width
                        && self.seen[row * 320..row * 320 + usize::from(*width)]
                            .iter()
                            .all(|seen| *seen)
                }
            })
    }

    fn finish_interval(&mut self) {
        if self.field == 0 || self.closed {
            return;
        }
        self.closed = true;
        if self.complete() {
            self.count += 1;
            self.last = Some((
                usize::from(self.width),
                usize::from(self.height),
                self.pixels.clone(),
            ));
            self.published = true;
        } else {
            self.incomplete += 1;
        }
    }
}

impl Surface {
    pub(super) fn count(&self) -> u64 {
        self.count
    }
    pub(super) fn copy_last(&self, target: &mut [u8]) -> Option<(usize, usize)> {
        let (width, height, pixels) = self.last.as_ref()?;
        for (y, row) in pixels.as_chunks::<320>().0.iter().take(*height).enumerate() {
            for (x, pixel) in row.iter().take(*width).enumerate() {
                let rgba = pixel.to_le_bytes();
                let offset = (y * width + x) * 4;
                target[offset..offset + 3].copy_from_slice(&rgba[..3]);
                target[offset + 3] = 255;
            }
        }
        Some((*width, *height))
    }
}
