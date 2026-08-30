use super::App;
#[cfg(target_arch = "wasm32")]
use crate::emu_thread::EmuResponse;

impl App {
    pub(super) fn stop_emu_thread(&mut self) {
        self.retire_emu_thread(false);
    }

    fn retire_emu_thread(&mut self, notify_stop: bool) {
        #[cfg(not(target_arch = "wasm32"))]
        let _ = notify_stop;
        #[cfg(not(target_arch = "wasm32"))]
        if self.recording.audio_recorder.is_some()
            && !self.synchronize_audio_recording_capture(
                crate::emu_thread::AudioRecordingCapture::default(),
            )
        {
            self.toast_manager
                .error("Audio capture could not be synchronized while stopping emulation");
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.stop_replay_recording_for_teardown();
            self.recording.pending_replay_start = None;
            self.retire_tas_control_worker();
        }
        if let Some(mut thread) = self.emu_thread.take() {
            thread.shutdown();
            #[cfg(target_arch = "wasm32")]
            self.wasm_retired_threads.push((thread, notify_stop));
        }
    }

    #[cfg(target_arch = "wasm32")]
    pub(super) fn stop_emu_thread_for_user_stop(&mut self) {
        self.retire_emu_thread(true);
        for (_, notify_stop) in &mut self.wasm_retired_threads {
            *notify_stop = true;
        }
    }

    #[cfg(target_arch = "wasm32")]
    pub(super) fn poll_retired_wasm_threads(&mut self) {
        let mut retained = Vec::with_capacity(self.wasm_retired_threads.len());
        for (thread, notify_stop) in self.wasm_retired_threads.drain(..) {
            let mut complete = false;
            while let Some(response) = thread.try_recv_response() {
                match response {
                    EmuResponse::SramFlushed(path) => {
                        if let Some(path) = path {
                            log::info!("Committed browser battery data to {path}");
                        }
                    }
                    EmuResponse::SramFlushFailed(error) => {
                        log::error!("Browser battery save failed: {error}");
                        self.toast_manager
                            .error(format!("Battery save failed: {error}"));
                    }
                    EmuResponse::RecoverySaved(path) => {
                        log::info!("Committed browser recovery state to {}", path.display());
                    }
                    EmuResponse::RecoverySaveFailed(error) => {
                        log::error!("Browser recovery save failed: {error}");
                        self.toast_manager
                            .error(format!("Recovery save failed: {error}"));
                    }
                    EmuResponse::SaveStateOk { path, .. } => {
                        log::info!("Committed browser save state to {}", path.display());
                        self.toast_manager.success("State saved");
                    }
                    EmuResponse::SaveStateFailed(error) => {
                        self.toast_manager.error(format!("Save failed: {error}"));
                    }
                    EmuResponse::StateBackupRestored(_) => {
                        self.toast_manager.success("Restored previous save state");
                    }
                    EmuResponse::StateBackupRestoreFailed(error) => {
                        self.toast_manager
                            .error(format!("Undo save failed: {error}"));
                    }
                    EmuResponse::ShutdownComplete => {
                        complete = true;
                        #[cfg(all(test, feature = "wasm-browser-tests"))]
                        super::browser_speculation_test::observe_retired_shutdown(&thread);
                        if notify_stop {
                            self.toast_manager.success("Stopped emulation");
                        }
                    }
                    _ => log::debug!("Ignoring response from retired browser emulator"),
                }
            }
            if !complete {
                retained.push((thread, notify_stop));
            }
        }
        self.wasm_retired_threads = retained;
        if self.wasm_retired_threads.is_empty()
            && let Some((name, bytes)) = self.pending_wasm_rom_after_flush.take()
        {
            self.load_rom_from_bytes(name, bytes);
        }
    }

    pub(super) fn perform_shutdown(&mut self) {
        if self.shutdown_performed {
            return;
        }
        self.shutdown_performed = true;

        #[cfg(not(target_arch = "wasm32"))]
        self.release_pce_mouse(false);

        #[cfg(not(target_arch = "wasm32"))]
        self.cancel_pending_rom_preparation(false);

        self.finish_audio_recording_for_teardown();
        self.stop_replay_recording_for_teardown();
        #[cfg(not(target_arch = "wasm32"))]
        self.wait_for_replay_finalization_on_shutdown();

        #[cfg(not(target_arch = "wasm32"))]
        {
            if let Err(error) = self.debug_windows.tas_editor.autosave_before_shutdown() {
                log::error!("Failed to autosave the TAS project during shutdown: {error:#}");
            }
            self.persist_debugger_window_geometry();
            self.persist_settings_window_geometry();
            self.persist_mods_window_geometry();
            self.persist_cheats_window_geometry();
            self.persist_printer_window_geometry();
        }
        self.persist_current_dock_layout();
        self.settings.ui.open_debug_tabs = crate::debug::save_open_tabs(&self.debug_dock);
        self.settings.save();

        self.save_current_cheats();

        self.stop_emu_thread();
        self.stop_camera_capture();

        self.gfx = None;
        self.audio = None;
        self.window_id = None;
        self.latest_frame = None;
        self.last_core_frame = None;
        self.last_displayed_frame = None;
    }
}
