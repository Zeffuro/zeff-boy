use super::EmuLoop;
use crate::emu_thread::{EmuThread, FrameInput};

impl EmuLoop {
    pub(super) fn deliver_md_timed_ports(&mut self, input: FrameInput) -> bool {
        let (result, response, dirty) = EmuThread::handle_md_timed_ports(
            &mut self.backend,
            input,
            self.rewind_buffer.fill_ratio(),
            &mut self.runtime_fault,
            self.audio_recording_capture,
            &mut self.pending_audio_discontinuities,
        );
        if dirty {
            self.battery_flush.mark_potentially_dirty();
        }
        crate::emu_thread::types::publish_backend_framebuffer(
            &self.shared_framebuffer,
            &self.backend,
        );
        if !EmuThread::send_frame(&self.frame_tx, &self.drain_rx, result, true) {
            return false;
        }
        self.send_resp(response)
    }
}
