use super::*;

impl EmuThread {
    pub(super) fn step_md_frames(
        backend: &mut EmuBackend,
        input: &mut FrameInput,
        cheats: &[crate::cheats::CheatPatch],
        runtime_fault: &mut WorkerRuntimeFault,
        advance: &mut PrimaryAdvanceResult,
    ) {
        let mut samples = input.buffers.audio.take().unwrap_or_default();
        samples.clear();
        for index in 0..input.frames {
            let replay_input = input
                .replay_joypad_frames
                .as_deref()
                .and_then(|frames| frames.get(index..));
            let advanced = Self::step_n_frames_with_runtime_fault(
                backend,
                1,
                cheats,
                input.audio.recording_capture.semantic,
                &mut advance.audio_semantic_frames,
                replay_input,
                runtime_fault,
            );
            backend.drain_audio_samples_into(&mut samples);
            advance.advanced_frames += advanced;
            if advanced == 0 || !runtime_fault.can_step() || backend.is_suspended() {
                break;
            }
        }
        advance.audio_samples = Some(samples);
    }
}
