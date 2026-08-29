use super::SharedFramebuffer;

#[derive(Default)]
pub(super) struct SpeculationBoundary {
    #[cfg(test)]
    forced_frames: usize,
    #[cfg(test)]
    completed_runs: usize,
    #[cfg(test)]
    committed_frames: usize,
    #[cfg(test)]
    force_operational_failure: bool,
    #[cfg(test)]
    force_wrong_framebuffer_len: bool,
}

pub(super) struct DetachedFrameOutput {
    pixels: Vec<u8>,
}

pub(super) struct DetachedFrameRequest {
    frames: usize,
}

pub(super) struct TerminalPersistenceReady {
    _private: (),
    #[cfg(test)]
    invalidated: bool,
}

trait FramebufferCommitSink {
    fn commit_framebuffer(&mut self, pixels: &[u8]);
}

struct SharedFramebufferSink<'a>(&'a SharedFramebuffer);

impl FramebufferCommitSink for SharedFramebufferSink<'_> {
    fn commit_framebuffer(&mut self, pixels: &[u8]) {
        super::types::publish_framebuffer(self.0, pixels);
    }
}

impl SpeculationBoundary {
    pub(super) fn request_detached_frame(
        &self,
        backend: &crate::emu_backend::EmuBackend,
        input: &super::FrameInput,
        cheats: &[crate::cheats::CheatPatch],
        uncapped_mode: bool,
        local_context: bool,
    ) -> Option<DetachedFrameRequest> {
        let frames = self.requested_frames();
        if frames == 0
            || input.frames != 1
            || input.speculation_blockers.any()
            || input.replay_joypad_frames.is_some()
            || input.debug_actions.has_pending()
            || input.debug_step
            || input.debug_continue
            || input.debug_suspend_after_frame
            || !cheats.is_empty()
            || uncapped_mode
            || !local_context
            || !backend.is_running()
        {
            return None;
        }

        if !backend.supports_detached_speculation() {
            return None;
        }
        Some(DetachedFrameRequest { frames })
    }

    pub(super) fn run_detached_frame(
        &mut self,
        backend: &crate::emu_backend::EmuBackend,
        request: Option<DetachedFrameRequest>,
        result: &super::FrameResult,
        runtime_fault_clear: bool,
    ) -> Option<DetachedFrameOutput> {
        let request = request?;
        if result.advanced_frames != 1 || result.runtime_fault.is_some() || !runtime_fault_clear {
            return None;
        }
        let mut detached = backend.fork_detached_for_speculation()?;
        detached.disable_audio_output();
        #[cfg(test)]
        if self.force_operational_failure {
            detached.force_operational_failure_for_test();
        }
        if !detached.step_frames(request.frames) {
            return None;
        }
        let pixels = detached.framebuffer().to_vec();
        #[cfg(test)]
        let pixels = if self.force_wrong_framebuffer_len {
            let mut pixels = pixels;
            pixels.pop();
            pixels
        } else {
            pixels
        };
        if pixels.len() != backend.framebuffer().len() {
            return None;
        }
        #[cfg(test)]
        {
            self.completed_runs += 1;
        }
        Some(DetachedFrameOutput { pixels })
    }

    pub(super) fn commit_primary_frame(
        &mut self,
        shared_framebuffer: &SharedFramebuffer,
        primary_framebuffer: &[u8],
        detached_frame: Option<DetachedFrameOutput>,
    ) {
        #[cfg(test)]
        {
            self.committed_frames += 1;
        }
        self.commit_selected_frame(
            &mut SharedFramebufferSink(shared_framebuffer),
            primary_framebuffer,
            detached_frame,
        );
    }

    pub(super) fn invalidate(&mut self) {}

    fn requested_frames(&self) -> usize {
        #[cfg(test)]
        {
            self.forced_frames
        }
        #[cfg(not(test))]
        {
            0
        }
    }

    pub(super) fn prepare_terminal_persistence(&mut self) -> TerminalPersistenceReady {
        self.invalidate();
        TerminalPersistenceReady {
            _private: (),
            #[cfg(test)]
            invalidated: true,
        }
    }

    fn commit_selected_frame(
        &mut self,
        sink: &mut impl FramebufferCommitSink,
        primary_framebuffer: &[u8],
        detached_frame: Option<DetachedFrameOutput>,
    ) {
        if let Some(frame) = detached_frame {
            sink.commit_framebuffer(&frame.pixels);
            return;
        }
        sink.commit_framebuffer(primary_framebuffer);
    }

    #[cfg(test)]
    pub(super) fn force_frames_for_test(&mut self, frames: usize) {
        self.forced_frames = frames;
    }

    #[cfg(test)]
    pub(super) fn completed_runs_for_test(&self) -> usize {
        self.completed_runs
    }

    #[cfg(test)]
    pub(super) fn committed_frames_for_test(&self) -> usize {
        self.committed_frames
    }

    #[cfg(test)]
    pub(super) fn force_operational_failure_for_test(&mut self) {
        self.force_operational_failure = true;
    }

    #[cfg(test)]
    pub(super) fn force_wrong_framebuffer_len_for_test(&mut self) {
        self.force_wrong_framebuffer_len = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct FramebufferCommitLedger {
        commits: usize,
        framebuffer: Vec<u8>,
        terminal_persistence: usize,
    }

    impl FramebufferCommitLedger {
        fn terminal_persistence(&mut self, ready: TerminalPersistenceReady) {
            assert!(ready.invalidated);
            self.terminal_persistence += 1;
        }
    }

    impl FramebufferCommitSink for FramebufferCommitLedger {
        fn commit_framebuffer(&mut self, pixels: &[u8]) {
            self.commits += 1;
            self.framebuffer.clear();
            self.framebuffer.extend_from_slice(pixels);
        }
    }

    #[test]
    fn detached_run_can_commit_only_one_selected_framebuffer() {
        let mut boundary = SpeculationBoundary::default();
        let mut ledger = FramebufferCommitLedger::default();

        boundary.commit_selected_frame(
            &mut ledger,
            &[1, 2, 3],
            Some(DetachedFrameOutput {
                pixels: vec![9, 8, 7],
            }),
        );

        assert_eq!(ledger.commits, 1);
        assert_eq!(ledger.framebuffer, [9, 8, 7]);
    }

    #[test]
    fn detached_abort_discards_output_and_commits_primary_framebuffer() {
        let mut boundary = SpeculationBoundary::default();
        let mut ledger = FramebufferCommitLedger::default();

        boundary.commit_selected_frame(&mut ledger, &[1, 2, 3], None);

        assert_eq!(ledger.commits, 1);
        assert_eq!(ledger.framebuffer, [1, 2, 3]);
    }

    #[test]
    fn terminal_persistence_requires_prior_invalidation() {
        let mut boundary = SpeculationBoundary::default();
        let mut ledger = FramebufferCommitLedger::default();

        let ready = boundary.prepare_terminal_persistence();
        ledger.terminal_persistence(ready);
        boundary.commit_selected_frame(&mut ledger, &[1, 2, 3], None);

        assert_eq!(ledger.terminal_persistence, 1);
        assert_eq!(ledger.commits, 1);
        assert_eq!(ledger.framebuffer, [1, 2, 3]);
    }
}
