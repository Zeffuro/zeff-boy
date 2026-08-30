use super::App;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct PauseState {
    user_paused: bool,
    user_revision: u64,
    focus: bool,
    dialog_depth: u32,
    runtime_fault: bool,
}

impl PauseState {
    pub(super) fn new() -> Self {
        Self::default()
    }

    pub(super) fn set_user_paused(&mut self, paused: bool) -> bool {
        if self.user_paused == paused {
            return true;
        }
        let Some(revision) = self.user_revision.checked_add(1) else {
            self.user_paused = true;
            return false;
        };
        self.user_paused = paused;
        self.user_revision = revision;
        true
    }

    pub(super) fn toggle_user_paused(&mut self) -> bool {
        self.set_user_paused(!self.user_paused)
    }

    pub(super) fn set_focus(&mut self, paused: bool) {
        self.focus = paused;
    }

    pub(super) fn focus_paused(&self) -> bool {
        self.focus
    }

    pub(super) fn begin_dialog(&mut self) {
        self.dialog_depth = self.dialog_depth.saturating_add(1);
    }

    pub(super) fn end_dialog(&mut self) {
        self.dialog_depth = self.dialog_depth.saturating_sub(1);
    }

    pub(super) fn latch_runtime_fault(&mut self) {
        self.runtime_fault = true;
    }

    pub(super) fn clear_runtime_fault(&mut self) {
        self.runtime_fault = false;
    }

    pub(super) fn effective(&self, tas_fenced: bool) -> bool {
        self.user_paused || self.focus || self.dialog_depth != 0 || self.runtime_fault || tas_fenced
    }
}

impl App {
    pub(in crate::app) fn recompute_pause(&mut self) {
        #[cfg(not(target_arch = "wasm32"))]
        let tas_fenced = !self.worker_gameplay_commands_allowed();
        #[cfg(target_arch = "wasm32")]
        let tas_fenced = false;
        let paused = self.pause_state.effective(tas_fenced);
        if self.speed.paused && !paused {
            self.timing.last_frame_time = crate::platform::Instant::now();
        }
        self.speed.paused = paused;
        self.toast_manager.set_paused(paused);
    }

    pub(in crate::app) fn set_user_paused(&mut self, paused: bool) {
        self.pause_state.set_user_paused(paused);
        self.recompute_pause();
    }

    pub(in crate::app) fn toggle_user_paused(&mut self) {
        self.pause_state.toggle_user_paused();
        self.recompute_pause();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn causes_compose_and_dialogs_nest() {
        let mut state = PauseState::new();
        state.begin_dialog();
        state.begin_dialog();
        state.end_dialog();
        assert!(state.effective(false));
        state.end_dialog();
        assert!(!state.effective(false));
        state.set_focus(true);
        assert!(state.effective(false));
        state.set_focus(false);
        state.latch_runtime_fault();
        assert!(state.effective(false));
        state.clear_runtime_fault();
        assert!(state.effective(true));
    }

    #[test]
    fn user_revision_changes_only_with_user_intent() {
        let mut state = PauseState::new();
        assert_eq!(state.user_revision, 0);
        state.set_focus(true);
        state.begin_dialog();
        state.latch_runtime_fault();
        assert_eq!(state.user_revision, 0);
        assert!(state.set_user_paused(true));
        assert_eq!(state.user_revision, 1);
        assert!(state.set_user_paused(true));
        assert_eq!(state.user_revision, 1);
    }

    #[test]
    fn user_revision_exhaustion_fails_paused() {
        let mut state = PauseState {
            user_paused: true,
            user_revision: u64::MAX,
            ..PauseState::new()
        };
        assert!(!state.set_user_paused(false));
        assert!(state.user_paused);
    }
}
