mod rollback_memory;
use crate::constants::*;
use crate::hardware::vdp::InterlaceMode;
use crate::{VdpCaptureError, VdpLineError, VdpScanline};
use zeff_emu_common::time::MasterTicks;

mod owned;
mod scroll;
mod state;
use scroll::ScrollFetch;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct RasterField {
    pub id: u64,
    pub height: usize,
    pub mode: InterlaceMode,
    pub odd: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RowDelivery {
    owned: Option<Box<owned::Surface>>,
    scroll: ScrollFetch,
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
    unknown_field: Option<u64>,
    framebuffer_known: bool,
}

pub(crate) struct VideoCheckpoint {
    owned: Option<Box<owned::Surface>>,
    scroll: ScrollFetch,
    pending: Option<MasterTicks>,
    completed: Option<Result<VdpScanline, VdpLineError>>,
    framebuffer: Option<Box<[u8]>>,
    width: usize,
    height: usize,
    rows: usize,
    frame_count: u64,
    frame_ready: bool,
    raster_error: Option<VdpLineError>,
    field: Option<RasterField>,
    valid_fields: u8,
    unknown_field: Option<u64>,
    framebuffer_known: bool,
}

impl Default for RowDelivery {
    fn default() -> Self {
        Self {
            owned: None,
            scroll: ScrollFetch::default(),
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
            unknown_field: None,
            framebuffer_known: true,
        }
    }
}

impl RowDelivery {
    pub(crate) fn rollback_capture_idle(&self) -> bool {
        self.pending.is_none() && self.completed.is_none()
    }

    pub(super) fn hold_scroll(&mut self, enabled: bool, pair: Option<[u16; 2]>) {
        self.scroll.hold(enabled, pair);
    }

    pub(super) fn fetch_scroll(
        &mut self,
        field: u64,
        row: u16,
        enabled: bool,
        pair: Option<[u16; 2]>,
    ) {
        self.scroll.fetch(field, row, enabled, pair);
    }

    pub(crate) fn horizontal_scroll(&self, field: u64, row: u16) -> Option<[u16; 2]> {
        self.scroll.horizontal(field, row)
    }

    pub(super) fn skip_scroll_rows(&mut self) {
        self.scroll.clear_rows();
    }

    pub(super) fn invalidate_field(&mut self, field: u64) {
        self.unknown_field = Some(field);
        self.framebuffer_known = false;
        self.frame_ready = false;
        self.rows = 0;
        self.field = None;
        self.valid_fields = 0;
        self.scroll.clear_rows();
    }

    pub fn framebuffer(&self) -> &[u8] {
        &self.framebuffer[..self.width * self.height * RGBA_CHANNELS]
    }

    pub fn framebuffer_available(&self) -> bool {
        self.framebuffer_known
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
                mode: InterlaceMode::Progressive,
                odd: false,
            },
            result,
        );
    }

    #[cfg(feature = "first-failure-diagnostic")]
    fn diagnostic_row_state(&self) -> crate::diagnostic::field::RowState {
        crate::diagnostic::field::RowState {
            width: self.width,
            height: self.height,
            rows: self.rows,
            field: self.field.map(|field| crate::diagnostic::field::RowField {
                id: field.id,
                height: field.height,
                mode: field.mode.into(),
                odd: field.odd,
            }),
            valid_fields: self.valid_fields,
            presented: self.frame_count,
            ready: self.frame_ready,
            error: self.raster_error,
        }
    }

    pub fn publish_field_row(
        &mut self,
        row: usize,
        field: RasterField,
        result: Result<VdpScanline, VdpLineError>,
    ) {
        #[cfg(feature = "first-failure-diagnostic")]
        let before = crate::diagnostic::field::enabled().then(|| self.diagnostic_row_state());
        #[cfg(feature = "first-failure-diagnostic")]
        use crate::diagnostic::field::cause;
        #[cfg(feature = "first-failure-diagnostic")]
        let mut causes = 0;
        let line = match result {
            Ok(line) => line,
            Err(VdpLineError::Render(crate::VdpRenderError::UnknownCram(_))) => {
                if row == 0 {
                    self.raster_error = None;
                }
                self.unknown_field = Some(field.id);
                self.framebuffer_known = false;
                self.frame_ready = false;
                self.rows = 0;
                self.field = None;
                self.valid_fields = 0;
                #[cfg(feature = "first-failure-diagnostic")]
                if let Some(before) = before {
                    crate::diagnostic::field::row_result(
                        before,
                        self.diagnostic_row_state(),
                        cause::RENDER_ERROR,
                    );
                }
                return;
            }
            Err(error) => {
                self.unknown_field = Some(field.id);
                self.framebuffer_known = false;
                self.frame_ready = false;
                self.rows = 0;
                self.raster_error = Some(error);
                self.field = None;
                self.valid_fields = 0;
                #[cfg(feature = "first-failure-diagnostic")]
                if let Some(before) = before {
                    crate::diagnostic::field::row_result(
                        before,
                        self.diagnostic_row_state(),
                        cause::RENDER_ERROR,
                    );
                }
                return;
            }
        };
        let width = line.colors().len();
        let height = field.height * if field.mode.interlaced() { 2 } else { 1 };
        if self.width != width || self.height != height {
            #[cfg(feature = "first-failure-diagnostic")]
            {
                causes |= cause::DIMENSIONS;
            }
            self.width = width;
            self.height = height;
            self.rows = 0;
            self.frame_ready = false;
            self.framebuffer.fill(0);
            self.field = None;
            self.valid_fields = 0;
        }
        if row == 0 {
            if self.unknown_field != Some(field.id) {
                self.unknown_field = None;
            }
            #[cfg(feature = "first-failure-diagnostic")]
            {
                causes |= cause::ROW_ZERO;
            }
            let consecutive = self.field.is_some_and(|old| {
                old.mode.interlaced()
                    && field.mode.interlaced()
                    && old.mode == field.mode
                    && old.height == field.height
                    && old.id.checked_add(1) == Some(field.id)
                    && old.odd != field.odd
                    && self.rows == old.height
            });
            if field.mode.interlaced() && !consecutive {
                #[cfg(feature = "first-failure-diagnostic")]
                {
                    causes |= cause::PARTNER_INVALID;
                }
                self.valid_fields = 0;
                self.framebuffer.fill(0);
            }
            self.rows = 0;
            self.raster_error = None;
            self.field = Some(field);
        } else if self.field.is_none() {
            #[cfg(feature = "first-failure-diagnostic")]
            {
                causes |= cause::ADOPT_FIELD;
            }
            self.field = Some(field);
        }
        if self.rows != row || self.field != Some(field) {
            #[cfg(feature = "first-failure-diagnostic")]
            if let Some(before) = before {
                if self.rows != row {
                    causes |= cause::ROWS_MISMATCH;
                }
                if self.field != Some(field) {
                    causes |= cause::FIELD_MISMATCH;
                }
                crate::diagnostic::field::row_result(before, self.diagnostic_row_state(), causes);
            }
            return;
        }
        let destination = if field.mode.interlaced() {
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
        if field.mode.interlaced() && self.valid_fields & (1 << usize::from(!field.odd)) == 0 {
            let partner = (destination ^ 1) * width * RGBA_CHANNELS;
            self.framebuffer
                .copy_within(start..start + width * RGBA_CHANNELS, partner);
        }
        self.rows += 1;
        #[cfg(feature = "first-failure-diagnostic")]
        {
            causes |= cause::ACCEPTED;
        }
        if self.rows == field.height && self.unknown_field != Some(field.id) {
            self.valid_fields = if field.mode.interlaced() {
                self.valid_fields | (1 << usize::from(field.odd))
            } else {
                0
            };
            self.frame_count = field.id.wrapping_add(1);
            self.frame_ready = true;
            self.framebuffer_known = true;
            #[cfg(feature = "first-failure-diagnostic")]
            {
                causes |= cause::PUBLISHED;
            }
        }
        #[cfg(feature = "first-failure-diagnostic")]
        if let Some(before) = before {
            crate::diagnostic::field::row_result(before, self.diagnostic_row_state(), causes);
        }
    }

    pub fn reset(&mut self) {
        self.scroll = ScrollFetch::default();
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
        self.unknown_field = None;
        self.framebuffer_known = true;
    }

    pub fn checkpoint(&self) -> VideoCheckpoint {
        self.checkpoint_for_advance(true)
    }

    pub(super) fn checkpoint_for_advance(&self, pixels_may_change: bool) -> VideoCheckpoint {
        VideoCheckpoint {
            owned: self.owned.clone(),
            scroll: self.scroll,
            pending: self.pending,
            completed: self.completed.clone(),
            framebuffer: pixels_may_change.then(|| self.framebuffer.clone()),
            width: self.width,
            height: self.height,
            rows: self.rows,
            frame_count: self.frame_count,
            frame_ready: self.frame_ready,
            raster_error: self.raster_error,
            field: self.field,
            valid_fields: self.valid_fields,
            unknown_field: self.unknown_field,
            framebuffer_known: self.framebuffer_known,
        }
    }

    pub fn restore(&mut self, checkpoint: VideoCheckpoint) {
        self.owned = checkpoint.owned;
        self.scroll = checkpoint.scroll;
        if let Some(framebuffer) = checkpoint.framebuffer {
            self.framebuffer.copy_from_slice(&framebuffer);
        }
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
        self.unknown_field = checkpoint.unknown_field;
        self.framebuffer_known = checkpoint.framebuffer_known;
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
