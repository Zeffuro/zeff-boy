use super::*;
use crate::emu_thread::{
    AudioRecordingCapture, MdTimedPortFailure, MdTimedPortFailureKind, MdTimedPortReply,
};

impl EmuThread {
    pub(in crate::emu_thread) fn handle_md_timed_ports(
        backend: &mut EmuBackend,
        mut input: FrameInput,
        rewind_fill: f32,
        runtime_fault: &mut WorkerRuntimeFault,
        capture: AudioRecordingCapture,
        markers: &mut Vec<crate::audio_recorder::AudioTimelineDiscontinuity>,
    ) -> (FrameResult, EmuResponse, bool) {
        let request = input.debug_actions.md_timed_ports.take().unwrap();
        let queued_pcm_samples_before = backend.md_timed_port_queued_pcm();
        let result = if input.frames != 0
            || input.debug_step
            || input.debug_continue
            || input.debug_suspend_after_frame
            || input.replay_joypad_frames.is_some()
            || input.debug_actions.requires_emulator_authority()
        {
            Err(MdTimedPortFailure::new(
                MdTimedPortFailureKind::Conflict,
                "timed ports require an isolated zero-frame request with a read-only snapshot",
            ))
        } else if !runtime_fault.can_step() {
            Err(MdTimedPortFailure::new(
                MdTimedPortFailureKind::Lifecycle,
                "the Mega Drive worker is faulted",
            ))
        } else {
            backend.execute_md_timed_ports(&request.writes)
        };
        let accepted = result.as_ref().is_ok_and(|report| report.writes != 0);
        // Ordinary delivery may drain old PCM and an already pending fault notification.
        let mut frame = Self::build_inert_frame_result(backend, input, rewind_fill, runtime_fault);
        let mut delivery_uncapped = false;
        let dirty = super::super::commands::finalize_step_result(
            &mut frame,
            accepted,
            false,
            capture,
            markers,
            runtime_fault,
            &mut delivery_uncapped,
        );
        let response = EmuResponse::MdTimedPortsFinished(MdTimedPortReply {
            id: request.id,
            result,
            queued_pcm_samples_before,
            delivered_pcm_samples: frame.audio_samples.len(),
        });
        (frame, response, dirty)
    }
}
