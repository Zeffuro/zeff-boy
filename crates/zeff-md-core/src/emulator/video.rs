use super::Emulator;
use crate::{VdpLineError, VdpScanline};
use zeff_emu_common::time::MasterTicks;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VdpCaptureError {
    Busy,
    DeadlineNotFuture {
        current: MasterTicks,
        requested: MasterTicks,
    },
}

impl Emulator {
    pub fn pacing_field_count(&self) -> u64 {
        self.bus.vdp.field_count()
    }

    pub fn nominal_frame_duration_ns(&self) -> u64 {
        let region = self.region();
        let interlaced = self
            .bus
            .vdp
            .field_position(region)
            .expect("validated VDP calendar")
            .interlaced;
        let lines_x2 = region.frame_lines() * 2;
        let lines_x2 = if interlaced {
            if region == crate::Region::Pal {
                lines_x2 - 1
            } else {
                lines_x2 + 1
            }
        } else {
            lines_x2
        };
        let ticks_x2 = lines_x2 * crate::constants::VDP_LINE_TICKS;
        (ticks_x2 * 1_000_000_000 + region.master_hz()) / (region.master_hz() * 2)
    }

    pub fn framebuffer(&self) -> &[u8] {
        self.bus.video.framebuffer()
    }
    pub fn framebuffer_dimensions(&self) -> (usize, usize) {
        self.bus.video.dimensions()
    }
    pub fn frame_ready(&self) -> bool {
        self.bus.video.frame_ready()
    }
    pub fn clear_frame_ready(&mut self) {
        self.bus.video.clear_frame_ready();
    }
    pub fn frame_count(&self) -> u64 {
        self.bus.video.frame_count()
    }
    pub fn video_error(&self) -> Option<VdpLineError> {
        self.bus.video.raster_error()
    }
    /// One caller-selected deadline; retrieve its row or capture error after execution.
    pub fn queue_vdp_capture(&mut self, at: MasterTicks) -> Result<(), VdpCaptureError> {
        self.bus.video.queue(at, self.bus.now)
    }

    pub fn take_vdp_capture(&mut self) -> Option<Result<VdpScanline, VdpLineError>> {
        self.bus.video.take()
    }
}
