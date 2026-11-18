use super::*;
use crate::emu_thread::types::publish_owned_framebuffer;

impl EmuThread {
    fn invalidate_rewind_audio(
        backend: &EmuBackend,
        rewind_buffer: &mut zeff_emu_common::rewind::RewindBuffer,
    ) {
        if backend.take_rewind_audio_change() {
            rewind_buffer.clear();
        }
    }

    pub(crate) fn reset_rewind_for_loaded_state(
        rewind_buffer: &mut zeff_emu_common::rewind::RewindBuffer,
        rewind_seconds: usize,
        backend: &EmuBackend,
    ) -> u64 {
        let frame_duration_ns = backend.nominal_frame_duration_ns();
        *rewind_buffer = zeff_emu_common::rewind::RewindBuffer::new_with_frame_duration(
            rewind_seconds,
            crate::emu_thread::REWIND_CAPTURE_INTERVAL_FRAMES,
            frame_duration_ns,
        );
        frame_duration_ns
    }

    pub(crate) fn handle_rewind(
        backend: &mut EmuBackend,
        rewind_buffer: &mut zeff_emu_common::rewind::RewindBuffer,
        shared_fb: &SharedFramebuffer,
        steps: usize,
    ) -> EmuResponse {
        if !backend.supports_rewind() {
            return EmuResponse::RewindFailed("rewind is not supported by this core".to_string());
        }
        Self::invalidate_rewind_audio(backend, rewind_buffer);
        let rewind_rollback = rewind_buffer.clone();
        if rewind_buffer.latest_snapshot_frame_matches_current() && rewind_buffer.len() > 1 {
            let current_state = backend.encode_rewind_checkpoint().ok();
            if current_state.as_ref().is_some_and(|current| {
                rewind_buffer
                    .peek()
                    .is_some_and(|frame| frame.state_bytes == *current)
            }) {
                rewind_buffer.discard_latest();
            }
        }
        if let Some(rewind_frame) = rewind_buffer.pop_steps(steps.max(1)) {
            match backend.restore_rewind_checkpoint(rewind_frame.state_bytes) {
                Ok(_) => {
                    if rewind_frame.framebuffer.is_empty() {
                        publish_backend_framebuffer(shared_fb, backend);
                    } else {
                        publish_owned_framebuffer(shared_fb, rewind_frame.framebuffer);
                    }
                    return EmuResponse::RewindOk {
                        media_slot_snapshot: backend.media_slot_snapshot(),
                        game_boy_serial_device: backend.game_boy_serial_device(),
                        rewound_frames: rewind_frame.rewound_frames,
                    };
                }
                Err(err) => {
                    *rewind_buffer = rewind_rollback;
                    log::warn!("Rewind restore failed: {}", err);
                    return EmuResponse::RewindFailed("rewind restore failed".to_string());
                }
            }
        }

        *rewind_buffer = rewind_rollback;
        EmuResponse::RewindFailed("no rewind data".to_string())
    }

    pub(crate) fn capture_rewind_snapshot(
        backend: &EmuBackend,
        rewind_buffer: &mut zeff_emu_common::rewind::RewindBuffer,
        enabled: bool,
        advanced_frames: usize,
    ) {
        Self::invalidate_rewind_audio(backend, rewind_buffer);
        if enabled
            && backend.supports_rewind()
            && rewind_buffer.advance_frames(advanced_frames)
            && let Ok(bytes) = backend.encode_rewind_checkpoint()
        {
            rewind_buffer.push(&bytes, backend.rewind_framebuffer());
        }
    }
}
