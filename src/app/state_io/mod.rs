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
#[cfg_attr(not(target_arch = "wasm32"), allow(unused_imports))]
pub(super) use archive_and_slots::extract_rom_from_zip_bytes;
pub(super) use archive_and_slots::{
    extract_rom_entry_from_zip, extract_rom_entry_path_from_zip, list_rom_entries_in_zip,
};
pub(crate) use rom_loading::detect_and_extract_rom;
#[cfg(not(target_arch = "wasm32"))]
pub(crate) use rom_loading::detect_and_extract_rom_with_zip_witness;
#[cfg(not(target_arch = "wasm32"))]
pub(crate) use rom_loading::is_native_archive_path;

use crate::debug::{DebugUiActions, DebugWindowState, FpsTracker};
use crate::emu_backend::EmuBackend;
use crate::emu_thread::{EmuCommand, EmuResponse, EmuThread};
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
    pub(in crate::app) fn recv_cold_response(&mut self) -> Option<EmuResponse> {
        loop {
            while let Some(result) = self.emu_thread.as_ref()?.try_recv_frame() {
                self.process_frame_result(result);
            }
            #[cfg(not(target_arch = "wasm32"))]
            let response = match self.emu_thread.as_ref()?.recv_checked() {
                Ok(response) => response,
                Err(()) => {
                    self.terminalize_tas_control_response_loss();
                    return None;
                }
            };
            #[cfg(target_arch = "wasm32")]
            let response = self.emu_thread.as_ref()?.recv()?;
            #[cfg(not(target_arch = "wasm32"))]
            let response = match self.consume_tas_control_response(response) {
                Some(response) => response,
                None => continue,
            };
            if self.handle_link_response(&response) {
                continue;
            }
            #[cfg(not(target_arch = "wasm32"))]
            let response = match self.consume_replay_start_response(response) {
                Some(response) => response,
                None => continue,
            };
            #[cfg(not(target_arch = "wasm32"))]
            let response = match self.consume_replay_finalization_response(response) {
                Some(response) => response,
                None => continue,
            };
            let response = match self.consume_media_response(response) {
                Some(response) => response,
                None => continue,
            };
            let response = match self.consume_serial_device_response(response) {
                Some(response) => response,
                None => continue,
            };
            return Some(response);
        }
    }

    pub(super) fn pause_for_dialog(&mut self) {
        self.suppress_unfocus_pause_until_focus = true;
        self.pause_state.begin_dialog();
        self.recompute_pause();
    }

    pub(super) fn resume_after_dialog(&mut self) {
        self.pause_state.end_dialog();
        self.recompute_pause();
    }

    pub(super) fn spawn_emu_thread(&mut self, backend: EmuBackend) {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let Some(next_generation) = next_emu_worker_generation(self.emu_worker_generation)
            else {
                log::error!("Emulator worker generation exhausted; refusing to start a worker");
                return;
            };
            self.emu_worker_generation = next_generation;
        }
        self.emu_thread = Some(EmuThread::spawn(
            backend,
            self.settings.emulation.save_recovery_state,
        ));
        self.pause_state.clear_runtime_fault();
        self.recompute_pause();
        queue_current_layer_policy(&self.debug_windows, &mut self.pending_debug_actions);
        let _ = self.send_emu_command_checked(EmuCommand::SetUncappedBatchSize(
            self.settings.emulation.uncapped_frames_per_tick,
        ));
        if let Some(recorder) = self.recording.audio_recorder.as_ref() {
            let capture = EmuCommand::SetAudioRecordingCapture {
                capture: crate::emu_thread::AudioRecordingCapture {
                    active: true,
                    semantic: recorder.captures_semantics(),
                },
                acknowledged: None,
            };
            let _ = self.send_emu_command_checked(capture);
        }
        if let Some(thread) = &self.emu_thread {
            self.latest_frame = thread.shared_framebuffer().load_full();
        }
        self.fps_tracker = FpsTracker::new();
        self.timing.last_frame_time = Instant::now();

        if self.timing.uncapped_speed && self.recording.allows_uncapped_worker() {
            let _ = self.send_emu_command_checked(EmuCommand::SetUncapped(true));
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn next_emu_worker_generation(current: u64) -> Option<u64> {
    current.checked_add(1)
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

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn worker_generation_increment_fails_closed_at_exhaustion() {
        assert_eq!(next_emu_worker_generation(0), Some(1));
        assert_eq!(next_emu_worker_generation(u64::MAX - 1), Some(u64::MAX));
        assert_eq!(next_emu_worker_generation(u64::MAX), None);
    }
}
