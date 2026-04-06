use super::App;
use crate::emu_backend::ActiveSystem;
use crate::emu_thread::{EmuCommand, EmuResponse};
use std::path::PathBuf;

impl App {
    pub(in crate::app) fn save_state_slot(&mut self, slot: u8) {
        if self.emu_thread.is_none() {
            return;
        }
        if let Some(thread) = &self.emu_thread {
            thread.send(EmuCommand::SaveStateSlot(slot));
        }
        match self.recv_cold_response() {
            Some(EmuResponse::SaveStateOk(path)) => {
                log::info!("Saved state to {}", path);
                self.toast_manager.success(format!("Saved to slot {slot}"));
            }
            Some(EmuResponse::SaveStateFailed(err)) => {
                log::error!("Failed to save state in slot {}: {}", slot, err);
                self.toast_manager.error(format!("Save failed: {err}"));
            }
            _ => {}
        }
    }

    pub(in crate::app) fn load_state_slot(&mut self, slot: u8) {
        if self.emu_thread.is_none() {
            return;
        }
        if let Some(thread) = &self.emu_thread {
            thread.send(EmuCommand::LoadStateSlot {
                slot,
                buttons_pressed: self.host_input.buttons_pressed(),
                dpad_pressed: self.host_input.dpad_pressed(),
            });
        }
        match self.recv_cold_response() {
            Some(EmuResponse::LoadStateOk { path }) => {
                if let Some(thread) = &self.emu_thread {
                    self.latest_frame = thread.shared_framebuffer().load_full();
                }
                log::info!("Loaded state from {}", path);
                self.toast_manager.success(format!("Loaded slot {slot}"));
            }
            Some(EmuResponse::LoadStateFailed(err)) => {
                log::error!("Failed to load state from slot {}: {}", slot, err);
                let msg = if err.contains("NotFound") || err.contains("not found") || err.contains("cannot find") {
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
        #[cfg(not(target_arch = "wasm32"))]
        {
            if let Some(config_dir) = dirs::config_dir() {
                return config_dir
                    .join("zeff-boy")
                    .join("saves")
                    .join(system.storage_subdir());
            }

            std::env::current_dir()
                .unwrap_or_else(|_| PathBuf::from("."))
                .join("saves")
                .join(system.storage_subdir())
        }

        #[cfg(target_arch = "wasm32")]
        {
            PathBuf::from(system.storage_subdir())
        }
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

    #[cfg(not(target_arch = "wasm32"))]
    pub(in crate::app) fn save_state_file_dialog(&mut self) {
        if self.emu_thread.is_none() {
            return;
        }

        let was_paused = self.pause_for_dialog();
        let file = rfd::FileDialog::new()
            .set_title("Save State As")
            .set_directory(self.state_dialog_dir())
            .add_filter("Zeff Boy Save State", &["state"])
            .set_file_name(self.default_state_file_name())
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
            Some(EmuResponse::SaveStateOk(saved)) => {
                log::info!("Saved state to {}", saved);
                self.toast_manager.success("State saved to file");
            }
            Some(EmuResponse::SaveStateFailed(err)) => {
                log::error!("Failed to save state to {}: {}", path.display(), err);
                self.toast_manager.error(format!("Save failed: {err}"));
            }
            _ => {}
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(in crate::app) fn load_state_file_dialog(&mut self) {
        if self.emu_thread.is_none() {
            return;
        }

        let was_paused = self.pause_for_dialog();
        let file = rfd::FileDialog::new()
            .set_title("Load State")
            .set_directory(self.state_dialog_dir())
            .add_filter("Zeff Boy Save State", &["state"])
            .pick_file();

        self.resume_after_dialog(was_paused);
        let Some(path) = file else {
            return;
        };

        self.last_state_dir = path.parent().map(|p| p.to_path_buf());

        if let Some(thread) = &self.emu_thread {
            thread.send(EmuCommand::LoadStateFromPath {
                path: path.clone(),
                buttons_pressed: self.host_input.buttons_pressed(),
                dpad_pressed: self.host_input.dpad_pressed(),
            });
        }
        match self.recv_cold_response() {
            Some(EmuResponse::LoadStateOk {
                path: p,
            }) => {
                if let Some(thread) = &self.emu_thread {
                    self.latest_frame = thread.shared_framebuffer().load_full();
                }
                log::info!("Loaded state from {}", p);
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
    pub(in crate::app) fn save_state_file_dialog(&mut self) {
        self.toast_manager.info("File save dialog not available on web");
    }

    #[cfg(target_arch = "wasm32")]
    pub(in crate::app) fn load_state_file_dialog(&mut self) {
        self.toast_manager.info("File load dialog not available on web");
    }
}
