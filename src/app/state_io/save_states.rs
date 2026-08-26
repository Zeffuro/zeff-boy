use super::App;
use crate::emu_backend::{ActiveSystem, system_specs};
use crate::emu_thread::{EmuCommand, EmuResponse};
use std::path::PathBuf;

fn all_state_file_extensions() -> Vec<&'static str> {
    let mut extensions = vec!["state"];
    for spec in system_specs() {
        if !extensions.contains(&spec.state_extension) {
            extensions.push(spec.state_extension);
        }
    }
    extensions
}

impl App {
    fn update_undo_save_path(&mut self, path: PathBuf, backup_created: bool) {
        self.undo_save_state_path = backup_created.then_some(path);
    }

    fn refresh_framebuffer_after_load(&mut self) {
        if let Some(thread) = &self.emu_thread {
            self.latest_frame = thread.shared_framebuffer().load_full();
        }
    }

    fn capture_current_state_for_undo(&mut self) -> Option<Vec<u8>> {
        if !self.core_supports_state_capture() {
            return None;
        }
        let Some(thread) = &self.emu_thread else {
            return None;
        };
        thread.send(EmuCommand::CaptureStateBytes);
        match self.recv_cold_response() {
            Some(EmuResponse::StateCaptured(bytes)) => Some(bytes),
            Some(EmuResponse::StateCaptureFailed(err)) => {
                log::warn!("Failed to capture undo state before load: {err}");
                None
            }
            _ => None,
        }
    }

    pub(in crate::app) fn undo_load_state(&mut self) {
        if !self.core_supports_state_capture() {
            self.undo_load_state = None;
            return;
        }
        let Some(state_bytes) = self.undo_load_state.take() else {
            self.toast_manager.info("No loaded state to undo");
            return;
        };
        if self.emu_thread.is_none() {
            self.undo_load_state = Some(state_bytes);
            return;
        }

        let redo_state = self.capture_current_state_for_undo();
        let state_bytes_for_retry = state_bytes.clone();
        if let Some(thread) = &self.emu_thread {
            let (buttons_pressed, dpad_pressed) = self.current_host_joypad_input();
            thread.send(EmuCommand::LoadStateBytes {
                state_bytes,
                buttons_pressed,
                dpad_pressed,
                replay_events: None,
                game_boy_link_start_state: None,
                game_boy_link_coordinator_start_state: None,
                game_boy_link_start_tick: None,
                wonder_swan_link_start_tick: None,
            });
        }
        match self.recv_cold_response() {
            Some(EmuResponse::LoadStateOk {
                path,
                media_slot_snapshot,
                game_boy_serial_device,
            }) => {
                self.media_slot_snapshot = media_slot_snapshot;
                if let Some(device) = game_boy_serial_device {
                    self.game_boy_serial_device = device;
                }
                self.refresh_framebuffer_after_load();
                log::info!("Undid loaded state via {path}");
                self.undo_load_state = redo_state;
                self.toast_manager
                    .success("Restored state from before load");
            }
            Some(EmuResponse::LoadStateFailed(err)) => {
                log::error!("Failed to undo loaded state: {err}");
                self.undo_load_state = Some(state_bytes_for_retry);
                self.toast_manager.error(format!("Undo load failed: {err}"));
            }
            _ => {
                self.undo_load_state = Some(state_bytes_for_retry);
            }
        }
    }

    pub(in crate::app) fn undo_save_state(&mut self) {
        let Some(path) = self.undo_save_state_path.take() else {
            self.toast_manager.info("No saved state to undo");
            return;
        };

        match crate::save_paths::restore_state_file_backup(&path) {
            Ok(()) => {
                log::info!("Undid saved state at {}", path.display());
                self.undo_save_state_path = Some(path);
                self.refresh_slot_info();
                self.toast_manager.success("Restored previous save state");
            }
            Err(err) => {
                log::error!("Failed to undo saved state at {}: {err}", path.display());
                self.toast_manager.error(format!("Undo save failed: {err}"));
            }
        }
    }

    pub(in crate::app) fn save_state_slot(&mut self, slot: u8) {
        if !self.core_supports_save_states() {
            return;
        }
        if let Some(thread) = &self.emu_thread {
            thread.send(EmuCommand::SaveStateSlot(slot));
        }
        match self.recv_cold_response() {
            Some(EmuResponse::SaveStateOk {
                path,
                backup_created,
            }) => {
                log::info!("Saved state to {}", path.display());
                self.update_undo_save_path(path, backup_created);
                self.toast_manager.success(format!("Saved to slot {slot}"));
                self.refresh_slot_info();
            }
            Some(EmuResponse::SaveStateFailed(err)) => {
                log::error!("Failed to save state in slot {}: {}", slot, err);
                self.toast_manager.error(format!("Save failed: {err}"));
            }
            _ => {}
        }
    }

    pub(in crate::app) fn load_state_slot(&mut self, slot: u8) {
        if !self.core_supports_save_states() {
            return;
        }
        let undo_state = self.capture_current_state_for_undo();
        if let Some(thread) = &self.emu_thread {
            let (buttons_pressed, dpad_pressed) = self.current_host_joypad_input();
            thread.send(EmuCommand::LoadStateSlot {
                slot,
                buttons_pressed,
                dpad_pressed,
            });
        }
        match self.recv_cold_response() {
            Some(EmuResponse::LoadStateOk {
                path,
                media_slot_snapshot,
                game_boy_serial_device,
            }) => {
                self.media_slot_snapshot = media_slot_snapshot;
                if let Some(device) = game_boy_serial_device {
                    self.game_boy_serial_device = device;
                }
                self.refresh_framebuffer_after_load();
                log::info!("Loaded state from {}", path);
                self.undo_load_state = undo_state;
                self.toast_manager.success(format!("Loaded slot {slot}"));
            }
            Some(EmuResponse::LoadStateFailed(err)) => {
                log::error!("Failed to load state from slot {}: {}", slot, err);
                let msg = if err.contains("NotFound")
                    || err.contains("not found")
                    || err.contains("cannot find")
                {
                    format!("No save in slot {slot}")
                } else {
                    format!("Load slot {slot} failed: {err}")
                };
                self.toast_manager.error(msg);
            }
            _ => {}
        }
    }

    pub(in crate::app) fn default_save_state_dir(system: ActiveSystem) -> PathBuf {
        crate::platform::save_dir(system.storage_subdir())
    }

    pub(in crate::app) fn default_state_file_name(&self) -> String {
        self.rom_info
            .rom_path
            .as_ref()
            .and_then(|p| p.file_stem())
            .and_then(|s| s.to_str())
            .map(|stem| format!("{stem}.state"))
            .unwrap_or_else(|| "save.state".to_string())
    }

    pub(in crate::app) fn state_dialog_dir(&self) -> PathBuf {
        if let Some(dir) = &self.last_state_dir {
            return dir.clone();
        }

        if let Some(rom_path) = &self.rom_info.rom_path
            && let Some(parent) = rom_path.parent()
        {
            return parent.to_path_buf();
        }

        Self::default_save_state_dir(self.active_system)
    }

    pub(in crate::app) fn save_state_file_dialog(&mut self) {
        if !self.core_supports_save_states() {
            return;
        }

        #[cfg(not(target_arch = "wasm32"))]
        {
            let was_paused = self.pause_for_dialog();
            let state_extensions = all_state_file_extensions();
            let file = crate::platform::FileDialog::new()
                .set_title("Save State As")
                .set_directory(self.state_dialog_dir())
                .add_filter("Zeff Boy Save State", &state_extensions)
                .set_file_name(&self.default_state_file_name())
                .save_file();

            self.resume_after_dialog(was_paused);
            let Some(path) = file else {
                return;
            };

            self.last_state_dir = path.parent().map(|p| p.to_path_buf());

            if let Some(thread) = &self.emu_thread {
                thread.send(EmuCommand::SaveStateToPath(path.clone()));
            }
            match self.recv_cold_response() {
                Some(EmuResponse::SaveStateOk {
                    path,
                    backup_created,
                }) => {
                    log::info!("Saved state to {}", path.display());
                    self.update_undo_save_path(path, backup_created);
                    self.toast_manager.success("State saved to file");
                }
                Some(EmuResponse::SaveStateFailed(err)) => {
                    log::error!("Failed to save state to {}: {}", path.display(), err);
                    self.toast_manager.error(format!("Save failed: {err}"));
                }
                _ => {}
            }
        }

        #[cfg(target_arch = "wasm32")]
        {
            if let Some(thread) = &self.emu_thread {
                thread.send(EmuCommand::CaptureStateBytes);
            }
            match self.recv_cold_response() {
                Some(EmuResponse::StateCaptured(bytes)) => {
                    let filename = self.default_state_file_name();
                    crate::platform::download_file(&filename, &bytes);
                    self.undo_save_state_path = None;
                    self.toast_manager.success("State exported to download");
                }
                Some(EmuResponse::StateCaptureFailed(err)) => {
                    self.toast_manager.error(format!("Export failed: {err}"));
                }
                _ => {}
            }
        }
    }

    pub(in crate::app) fn load_state_file_dialog(&mut self) {
        if !self.core_supports_save_states() {
            return;
        }

        #[cfg(not(target_arch = "wasm32"))]
        {
            let was_paused = self.pause_for_dialog();
            let state_extensions = all_state_file_extensions();
            let file = crate::platform::FileDialog::new()
                .set_title("Load State")
                .set_directory(self.state_dialog_dir())
                .add_filter("Zeff Boy Save State", &state_extensions)
                .pick_file();

            self.resume_after_dialog(was_paused);
            let Some(path) = file else {
                return;
            };

            self.last_state_dir = path.parent().map(|p| p.to_path_buf());
            let undo_state = self.capture_current_state_for_undo();

            if let Some(thread) = &self.emu_thread {
                let (buttons_pressed, dpad_pressed) = self.current_host_joypad_input();
                thread.send(EmuCommand::LoadStateFromPath {
                    path: path.clone(),
                    buttons_pressed,
                    dpad_pressed,
                });
            }
            match self.recv_cold_response() {
                Some(EmuResponse::LoadStateOk {
                    path: p,
                    media_slot_snapshot,
                    game_boy_serial_device,
                }) => {
                    self.media_slot_snapshot = media_slot_snapshot;
                    if let Some(device) = game_boy_serial_device {
                        self.game_boy_serial_device = device;
                    }
                    self.refresh_framebuffer_after_load();
                    log::info!("Loaded state from {}", p);
                    self.undo_load_state = undo_state;
                    self.toast_manager.success("State loaded from file");
                }
                Some(EmuResponse::LoadStateFailed(err)) => {
                    log::error!("Failed to load state from {}: {}", path.display(), err);
                    self.toast_manager.error(format!("Load failed: {err}"));
                }
                _ => {}
            }
        }

        #[cfg(target_arch = "wasm32")]
        {
            let state_extensions = all_state_file_extensions();
            crate::platform::FileDialog::new()
                .add_filter("Save States", &state_extensions)
                .set_title("Load State")
                .pick_file_web(self.pending_state_load.clone());
        }
    }

    /// Check the WASM pending-state-load slot and apply the state if data arrived.
    #[cfg(target_arch = "wasm32")]
    pub(in crate::app) fn check_pending_state_load(&mut self) {
        let data = self.pending_state_load.borrow_mut().take();
        if let Some((name, bytes)) = data {
            if !self.core_supports_save_states() {
                self.toast_manager
                    .error("The active core does not support save states");
                return;
            }
            let undo_state = self.capture_current_state_for_undo();
            if let Some(thread) = &self.emu_thread {
                let (buttons_pressed, dpad_pressed) = self.current_host_joypad_input();
                thread.send(EmuCommand::LoadStateBytes {
                    state_bytes: bytes,
                    buttons_pressed,
                    dpad_pressed,
                    replay_events: None,
                    game_boy_link_start_state: None,
                    game_boy_link_coordinator_start_state: None,
                    game_boy_link_start_tick: None,
                    wonder_swan_link_start_tick: None,
                });
            }
            match self.recv_cold_response() {
                Some(EmuResponse::LoadStateOk {
                    path,
                    media_slot_snapshot,
                    game_boy_serial_device,
                }) => {
                    self.media_slot_snapshot = media_slot_snapshot;
                    if let Some(device) = game_boy_serial_device {
                        self.game_boy_serial_device = device;
                    }
                    self.refresh_framebuffer_after_load();
                    log::info!("Loaded state from file: {name}");
                    self.undo_load_state = undo_state;
                    self.toast_manager
                        .success(format!("State loaded from {name}"));
                }
                Some(EmuResponse::LoadStateFailed(err)) => {
                    log::error!("Failed to load state from {name}: {err}");
                    self.toast_manager.error(format!("Load failed: {err}"));
                }
                _ => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_file_filters_cover_registered_system_state_extensions() {
        let extensions = all_state_file_extensions();

        assert!(extensions.contains(&"state"));
        for spec in system_specs() {
            assert!(
                extensions.contains(&spec.state_extension),
                "missing state extension for {}",
                spec.short_code
            );
        }
    }
}
