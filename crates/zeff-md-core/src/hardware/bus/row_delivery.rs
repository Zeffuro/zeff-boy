use crate::constants::*;
use crate::{VdpCaptureError, VdpLineError, VdpScanline};
use zeff_emu_common::time::MasterTicks;

mod state;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct RasterField {
    pub id: u64,
    pub height: usize,
    pub interlaced: bool,
    pub odd: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RowDelivery {
    pending: Option<MasterTicks>,
    completed: Option<Result<VdpScanline, VdpLineError>>,
    framebuffer: Box<[u8]>,
    width: usize,
    height: usize,
    rows: usize,
    frame_count: u64,
    frame_ready: bool,
    raster_error: Option<VdpLineError>,
    field: Option<RasterField>,
    valid_fields: u8,
}

pub(crate) struct VideoCheckpoint {
    pending: Option<MasterTicks>,
    completed: Option<Result<VdpScanline, VdpLineError>>,
    framebuffer: Box<[u8]>,
    width: usize,
    height: usize,
    rows: usize,
    frame_count: u64,
    frame_ready: bool,
    raster_error: Option<VdpLineError>,
    field: Option<RasterField>,
    valid_fields: u8,
}

impl Default for RowDelivery {
    fn default() -> Self {
        Self {
            pending: None,
            completed: None,
            framebuffer: vec![0; FRAMEBUFFER_LEN].into_boxed_slice(),
            width: VDP_H32_FRAME_WIDTH,
            height: VDP_FRAME_HEIGHT,
            rows: 0,
            frame_count: 0,
            frame_ready: false,
            raster_error: None,
            field: None,
            valid_fields: 0,
        }
    }
}

impl RowDelivery {
    pub fn framebuffer(&self) -> &[u8] {
        &self.framebuffer[..self.width * self.height * RGBA_CHANNELS]
    }

    pub fn dimensions(&self) -> (usize, usize) {
        (self.width, self.height)
    }

    pub fn frame_count(&self) -> u64 {
        self.frame_count
    }
    pub fn frame_ready(&self) -> bool {
        self.frame_ready
    }
    pub fn clear_frame_ready(&mut self) {
        self.frame_ready = false;
    }
    pub fn raster_error(&self) -> Option<VdpLineError> {
        self.raster_error
    }

    #[cfg(test)]
    pub fn publish_row(
        &mut self,
        row: usize,
        field: u64,
        height: usize,
        result: Result<VdpScanline, VdpLineError>,
    ) {
        self.publish_field_row(
            row,
            RasterField {
                id: field,
                height,
                interlaced: false,
                odd: false,
            },
            result,
        );
    }

    pub fn publish_field_row(
        &mut self,
        row: usize,
        field: RasterField,
        result: Result<VdpScanline, VdpLineError>,
    ) {
        let line = match result {
            Ok(line) => line,
            Err(error) => {
                self.rows = 0;
                self.raster_error = Some(error);
                self.field = None;
                self.valid_fields = 0;
                return;
            }
        };
        let width = line.colors().len();
        let height = field.height * if field.interlaced { 2 } else { 1 };
        if self.width != width || self.height != height {
            self.width = width;
            self.height = height;
            self.rows = 0;
            self.frame_ready = false;
            self.framebuffer.fill(0);
            self.field = None;
            self.valid_fields = 0;
        }
        if row == 0 {
            let consecutive = self.field.is_some_and(|old| {
                old.interlaced
                    && field.interlaced
                    && old.height == field.height
                    && old.id.checked_add(1) == Some(field.id)
                    && old.odd != field.odd
                    && self.rows == old.height
            });
            if field.interlaced && !consecutive {
                self.valid_fields = 0;
                self.framebuffer.fill(0);
            }
            self.rows = 0;
            self.raster_error = None;
            self.field = Some(field);
        } else if self.field.is_none() {
            self.field = Some(field);
        }
        if self.rows != row || self.field != Some(field) {
            return;
        }
        let destination = if field.interlaced {
            row * 2 + usize::from(field.odd)
        } else {
            row
        };
        let start = destination * width * RGBA_CHANNELS;
        for (pixel, &color) in self.framebuffer[start..start + width * RGBA_CHANNELS]
            .as_chunks_mut::<RGBA_CHANNELS>()
            .0
            .iter_mut()
            .zip(line.colors())
        {
            *pixel = rgba(color);
        }
        if field.interlaced && self.valid_fields & (1 << usize::from(!field.odd)) == 0 {
            let partner = (destination ^ 1) * width * RGBA_CHANNELS;
            self.framebuffer
                .copy_within(start..start + width * RGBA_CHANNELS, partner);
        }
        self.rows += 1;
        if self.rows == field.height {
            self.valid_fields = if field.interlaced {
                self.valid_fields | (1 << usize::from(field.odd))
            } else {
                0
            };
            self.frame_count = field.id.wrapping_add(1);
            self.frame_ready = true;
        }
    }

    pub fn reset(&mut self) {
        self.pending = None;
        self.completed = None;
        self.framebuffer.fill(0);
        self.width = VDP_H32_FRAME_WIDTH;
        self.height = VDP_FRAME_HEIGHT;
        self.rows = 0;
        self.frame_count = 0;
        self.frame_ready = false;
        self.raster_error = None;
        self.field = None;
        self.valid_fields = 0;
    }

    pub fn checkpoint(&self) -> VideoCheckpoint {
        VideoCheckpoint {
            pending: self.pending,
            completed: self.completed.clone(),
            framebuffer: self.framebuffer.clone(),
            width: self.width,
            height: self.height,
            rows: self.rows,
            frame_count: self.frame_count,
            frame_ready: self.frame_ready,
            raster_error: self.raster_error,
            field: self.field,
            valid_fields: self.valid_fields,
        }
    }

    pub fn restore(&mut self, checkpoint: VideoCheckpoint) {
        self.framebuffer.copy_from_slice(&checkpoint.framebuffer);
        self.pending = checkpoint.pending;
        self.completed = checkpoint.completed;
        self.width = checkpoint.width;
        self.height = checkpoint.height;
        self.rows = checkpoint.rows;
        self.frame_count = checkpoint.frame_count;
        self.frame_ready = checkpoint.frame_ready;
        self.raster_error = checkpoint.raster_error;
        self.field = checkpoint.field;
        self.valid_fields = checkpoint.valid_fields;
    }
    pub fn queue(&mut self, at: MasterTicks, now: MasterTicks) -> Result<(), VdpCaptureError> {
        if self.pending.is_some() || self.completed.is_some() {
            return Err(VdpCaptureError::Busy);
        }
        if at <= now {
            return Err(VdpCaptureError::DeadlineNotFuture {
                current: now,
                requested: at,
            });
        }
        self.pending = Some(at);
        Ok(())
    }

    pub fn deadline(&self) -> Option<MasterTicks> {
        self.pending
    }

    pub fn complete(&mut self, result: Result<VdpScanline, VdpLineError>) {
        self.pending = None;
        self.completed = Some(result);
    }

    pub fn take(&mut self) -> Option<Result<VdpScanline, VdpLineError>> {
        self.completed.take()
    }
}

fn rgba(color: u16) -> [u8; RGBA_CHANNELS] {
    let mut pixel = [u8::MAX; RGBA_CHANNELS];
    for (channel, shift) in VIDEO_CHANNEL_SHIFTS.into_iter().enumerate() {
        pixel[channel] = (((color >> shift) & VIDEO_CHANNEL_MASK) * u16::from(u8::MAX)
            / VIDEO_CHANNEL_MAX) as u8;
    }
    pixel
}

#[cfg(test)]
#[path = "row_delivery/tests.rs"]
mod tests;
