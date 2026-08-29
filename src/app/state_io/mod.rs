use super::App;
mod archive_and_slots;
mod audio_recording;
mod cheats_setup;
mod replay;
mod rom_loading;
mod save_states;
mod screenshots;
mod wasm_rom;

pub(crate) use archive_and_slots::SlotInfo;
pub(super) use archive_and_slots::build_slot_info;
pub(crate) use archive_and_slots::extract_rom_from_zip;
#[allow(unused_imports)] // Used on WASM for drag-and-drop ROM loading
pub(super) use archive_and_slots::extract_rom_from_zip_bytes;
pub(super) use archive_and_slots::{
    extract_rom_entry_from_zip, extract_rom_entry_path_from_zip, list_rom_entries_in_zip,
};
pub(crate) use rom_loading::detect_and_extract_rom;
#[cfg(not(target_arch = "wasm32"))]
pub(crate) use rom_loading::is_native_archive_path;

use crate::debug::{DebugUiActions, DebugWindowState, FpsTracker};
use crate::emu_backend::EmuBackend;
use crate::emu_thread::{EmuCommand, EmuThread};
use crate::platform::Instant;

pub(super) fn queue_current_layer_policy(
    debug_windows: &DebugWindowState,
    pending: &mut DebugUiActions,
) {
    pending.layer_toggles = Some((
        debug_windows.layer_enable_bg,
        debug_windows.layer_enable_window,
        debug_windows.layer_enable_sprites,
    ));
    pending.gba_bg_layer_toggles = Some(debug_windows.gba_layer_enable_bg);
}

impl App {
    pub(super) fn pause_for_dialog(&mut self) -> bool {
        let was_paused = self.speed.paused;
        self.suppress_unfocus_pause_until_focus = true;
        self.speed.paused = true;
        was_paused
    }

    pub(super) fn resume_after_dialog(&mut self, was_paused: bool) {
        let was_paused_by_unfocus = self.paused_by_unfocus;
        self.speed.paused = was_paused;
        self.paused_by_unfocus = false;
        if was_paused_by_unfocus && !was_paused {
            self.toast_manager.set_paused(false);
        }
        self.timing.last_frame_time = Instant::now();
    }

    pub(super) fn spawn_emu_thread(&mut self, backend: EmuBackend) {
        self.emu_thread = Some(EmuThread::spawn(
            backend,
            self.settings.emulation.save_recovery_state,
        ));
        queue_current_layer_policy(&self.debug_windows, &mut self.pending_debug_actions);
        if let Some(thread) = &self.emu_thread {
            thread.send(EmuCommand::SetUncappedBatchSize(
                self.settings.emulation.uncapped_frames_per_tick,
            ));
        }
        if let (Some(thread), Some(recorder)) =
            (&self.emu_thread, self.recording.audio_recorder.as_ref())
        {
            thread.send(EmuCommand::SetAudioRecordingCapture {
                capture: crate::emu_thread::AudioRecordingCapture {
                    active: true,
                    semantic: recorder.captures_semantics(),
                },
                acknowledged: None,
            });
        }
        if let Some(thread) = &self.emu_thread {
            self.latest_frame = thread.shared_framebuffer().load_full();
        }
        self.fps_tracker = FpsTracker::new();
        self.timing.last_frame_time = Instant::now();

        if self.timing.uncapped_speed
            && self.recording.allows_uncapped_worker()
            && let Some(thread) = &self.emu_thread
        {
            thread.send(EmuCommand::SetUncapped(true));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_layer_policy_queues_exact_single_slots() {
        let mut debug_windows = DebugWindowState::new();
        debug_windows.layer_enable_bg = false;
        debug_windows.layer_enable_window = true;
        debug_windows.layer_enable_sprites = false;
        debug_windows.gba_layer_enable_bg = [false, true, false, true];
        let mut pending = DebugUiActions::none();

        queue_current_layer_policy(&debug_windows, &mut pending);

        assert_eq!(pending.layer_toggles, Some((false, true, false)));
        assert_eq!(
            pending.gba_bg_layer_toggles,
            Some([false, true, false, true])
        );
    }
}
