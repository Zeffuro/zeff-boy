use super::*;
use zeff_emu_common::save_state::{StateReader, StateWriter};

const PIXELS: usize = 320 * 240;

impl Surface {
    pub(in crate::hardware::bus::row_delivery) fn write_state(
        &self,
        w: &mut StateWriter,
    ) -> Result<()> {
        self.validate_state(u64::MAX)?;
        w.write_u8(3);
        w.write_u64(self.field);
        w.write_u16(self.width);
        w.write_u16(self.height);
        w.write_u8(match self.mode5 {
            None => 0,
            Some(false) => 1,
            Some(true) => 2,
        });
        for value in [self.mixed, self.active, self.closed, self.published] {
            w.write_bool(value);
        }
        w.write_u64(self.count);
        w.write_u64(self.incomplete);
        for width in self.widths {
            w.write_u16(width);
        }
        for (&pixel, &seen) in self.pixels.iter().zip(self.seen.iter()) {
            w.write_u32(pixel);
            w.write_bool(seen);
        }
        w.write_bool(self.last.is_some());
        if let Some((width, height, pixels)) = &self.last {
            w.write_u16(*width as u16);
            w.write_u16(*height as u16);
            for pixel in pixels.iter() {
                w.write_u32(*pixel);
            }
        }
        Ok(())
    }

    pub(in crate::hardware::bus::row_delivery) fn read_state(
        r: &mut StateReader<'_>,
    ) -> Result<Self> {
        ensure!(r.read_u8()? == 3, "unsupported physical frame state");
        let mut frame = Self::new();
        frame.field = r.read_u64()?;
        frame.width = r.read_u16()?;
        frame.height = r.read_u16()?;
        frame.mode5 = match r.read_u8()? {
            0 => None,
            1 => Some(false),
            2 => Some(true),
            _ => anyhow::bail!("invalid frame mode"),
        };
        frame.mixed = r.read_bool()?;
        frame.active = r.read_bool()?;
        frame.closed = r.read_bool()?;
        frame.published = r.read_bool()?;
        frame.count = r.read_u64()?;
        frame.incomplete = r.read_u64()?;
        for width in &mut frame.widths {
            *width = r.read_u16()?;
        }
        for (pixel, seen) in frame.pixels.iter_mut().zip(frame.seen.iter_mut()) {
            *pixel = r.read_u32()?;
            *seen = r.read_bool()?;
        }
        if r.read_bool()? {
            let width = usize::from(r.read_u16()?);
            let height = usize::from(r.read_u16()?);
            let mut pixels = vec![0; PIXELS].into_boxed_slice();
            for pixel in pixels.iter_mut() {
                *pixel = r.read_u32()?;
            }
            frame.last = Some((width, height, pixels));
        }
        frame.validate_state(u64::MAX)?;
        Ok(frame)
    }

    pub(in crate::hardware::bus::row_delivery) fn validate_pixel(
        &self,
        pixel: Option<OwnedPixel>,
    ) -> Result<()> {
        if let Some(pixel) = pixel {
            ensure!(
                self.field >= pixel.field
                    && (self.field == pixel.field || self.closed || self.mixed),
                "frame and RGB field fences differ"
            );
            if self.field == pixel.field && self.field != 0 && pixel.vertical_active && !self.closed
            {
                ensure!(
                    self.active
                        && self.height == pixel.height
                        && self.mode5 == Some(pixel.mode5)
                        && self.width == if pixel.wide { 320 } else { 256 },
                    "frame and delayed RGB geometry differ"
                );
            }
        }
        Ok(())
    }

    pub(in crate::hardware::bus::row_delivery) fn validate_state(
        &self,
        raster_field: u64,
    ) -> Result<()> {
        ensure!(
            self.field <= raster_field && self.pixels.len() == PIXELS && self.seen.len() == PIXELS,
            "invalid physical frame frontier or storage"
        );
        ensure!(
            self.count.checked_add(self.incomplete).is_some_and(
                |n| n <= self.field && (self.field == 0 || self.closed || n < self.field)
            ) && (self.count != 0) == self.last.is_some(),
            "invalid physical frame counters"
        );
        ensure!(
            matches!(self.width, 0 | 256 | 320)
                && matches!(self.height, 0 | 192 | 224 | 240)
                && (self.width == 0) == (self.height == 0),
            "invalid physical frame geometry"
        );
        ensure!(
            (self.width == 0) == self.mode5.is_none(),
            "frame mode has no geometry"
        );
        ensure!(
            self.width == 0
                || matches!(
                    (self.mode5, self.height),
                    (Some(false), 192) | (Some(true), 224 | 240)
                ),
            "frame mode and height differ"
        );
        if self.field == 0 {
            ensure!(
                self.width == 0 && !self.mixed && !self.active && !self.closed && !self.published,
                "invalid physical frame origin"
            );
        } else {
            ensure!(
                (!self.closed || !self.active)
                    && (self.width != 0 || !self.active && !self.published),
                "invalid physical frame interval"
            );
        }
        for (row, width) in self.widths.iter().copied().enumerate() {
            ensure!(matches!(width, 0 | 256 | 320), "invalid physical row width");
            let seen = &self.seen[row * 320..(row + 1) * 320];
            ensure!(
                seen.iter().any(|v| *v) == (width != 0),
                "physical row has no sample provenance"
            );
            if !self.mixed && width != 0 {
                ensure!(
                    width == self.width
                        && row < usize::from(self.height)
                        && seen[usize::from(width)..].iter().all(|v| !v),
                    "physical row exceeds geometry"
                );
            }
        }
        for (&pixel, &seen) in self.pixels.iter().zip(self.seen.iter()) {
            ensure!(
                rgb_valid(pixel) && (seen || pixel == 0),
                "invalid physical pixel or unseen payload"
            );
        }
        ensure!(
            !self.published
                || self.closed && self.count != 0 && self.last.is_some() && self.complete(),
            "published physical interval is incomplete"
        );
        ensure!(
            !self.closed || self.published == self.complete(),
            "closed frame publication differs"
        );
        if let Some((width, height, pixels)) = &self.last {
            ensure!(
                matches!(*width, 256 | 320)
                    && matches!(*height, 192 | 224 | 240)
                    && pixels.len() == PIXELS,
                "invalid last physical capture geometry"
            );
            for (index, row) in pixels.as_chunks::<320>().0.iter().enumerate() {
                ensure!(
                    row.iter().all(|v| rgb_valid(*v))
                        && row[if index < *height { *width } else { 0 }..]
                            .iter()
                            .all(|v| *v == 0),
                    "invalid last physical capture pixels"
                );
            }
        }
        Ok(())
    }
}

fn rgb_valid(pixel: u32) -> bool {
    pixel <= 0xffffff
}
