use super::App;
use crate::debug::FpsTracker;
use crate::emu_thread::{EmuCommand, EmuResponse, EmuThread};
use crate::emulator::Emulator;
use std::path::{Path, PathBuf};
use std::time::Instant;

impl App {
    pub(super) fn save_state_slot(&mut self, slot: u8) {
        if self.emu_thread.is_none() {
            return;
        }
        if let Some(thread) = &self.emu_thread {
            thread.send(EmuCommand::SaveStateSlot(slot));
        }
        match self.recv_cold_response() {
            Some(EmuResponse::SaveStateOk(path)) => {
                log::info!("Saved state to {}", path);
            }
            Some(EmuResponse::SaveStateFailed(err)) => {
                log::error!("Failed to save state in slot {}: {}", slot, err);
            }
            _ => {}
        }
    }

    pub(super) fn load_state_slot(&mut self, slot: u8) {
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
            Some(EmuResponse::LoadStateOk { path, framebuffer }) => {
                self.latest_frame = Some(framebuffer);
                log::info!("Loaded state from {}", path);
            }
            Some(EmuResponse::LoadStateFailed(err)) => {
                log::error!("Failed to load state from slot {}: {}", slot, err);
            }
            _ => {}
        }
    }

    pub(super) fn load_rom(&mut self, path: &Path) {
        // Stop the old emu thread (flushes SRAM via Shutdown command)
        self.stop_emu_thread();

        match Emulator::from_rom_with_mode(path, self.settings.hardware_mode_preference) {
            Ok(mut emu) => {
                if let Some(audio) = &self.audio {
                    emu.bus.io.apu.set_sample_rate(audio.sample_rate());
                }
                // Apply current host input before handing to emu thread
                let buttons = self.host_input.buttons_pressed();
                let dpad = self.host_input.dpad_pressed();
                emu.bus.io.joypad.apply_pressed_masks(buttons, dpad);

                log::info!("Loaded ROM: {}", path.display());

                // Cache metadata
                self.cached_is_mbc7 = emu.is_mbc7_cartridge();
                self.cached_rom_path = Some(emu.rom_path().to_path_buf());

                // Hand ownership to emu thread
                self.emu_thread = Some(EmuThread::spawn(emu));
                self.fps_tracker = FpsTracker::new();
                self.last_frame_time = Instant::now();
            }
            Err(e) => {
                log::error!("Failed to load ROM '{}': {}", path.display(), e);
            }
        }
    }

    pub(super) fn open_file_dialog(&mut self) {
        let file = rfd::FileDialog::new()
            .add_filter("Game Boy ROMs", &["gb", "gbc"])
            .add_filter("All files", &["*"])
            .set_title("Open ROM")
            .pick_file();

        if let Some(path) = file {
            self.load_rom(&path);
        }
    }

    fn default_save_state_dir() -> PathBuf {
        if let Some(config_dir) = dirs::config_dir() {
            return config_dir.join("zeff-boy").join("saves");
        }

        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join("saves")
    }

    fn default_state_file_name(&self) -> String {
        self.cached_rom_path
            .as_ref()
            .and_then(|p| p.file_stem())
            .and_then(|s| s.to_str())
            .map(|stem| format!("{stem}.state"))
            .unwrap_or_else(|| "save.state".to_string())
    }

    fn state_dialog_dir(&self) -> PathBuf {
        if let Some(dir) = &self.last_state_dir {
            return dir.clone();
        }

        if let Some(rom_path) = &self.cached_rom_path {
            if let Some(parent) = rom_path.parent() {
                return parent.to_path_buf();
            }
        }

        Self::default_save_state_dir()
    }

    pub(super) fn save_state_file_dialog(&mut self) {
        if self.emu_thread.is_none() {
            return;
        }

        let file = rfd::FileDialog::new()
            .set_title("Save State As")
            .set_directory(self.state_dialog_dir())
            .add_filter("Zeff Boy Save State", &["state"])
            .set_file_name(&self.default_state_file_name())
            .save_file();

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
            }
            Some(EmuResponse::SaveStateFailed(err)) => {
                log::error!("Failed to save state to {}: {}", path.display(), err);
            }
            _ => {}
        }
    }

    pub(super) fn load_state_file_dialog(&mut self) {
        if self.emu_thread.is_none() {
            return;
        }

        let file = rfd::FileDialog::new()
            .set_title("Load State")
            .set_directory(self.state_dialog_dir())
            .add_filter("Zeff Boy Save State", &["state"])
            .pick_file();

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
            Some(EmuResponse::LoadStateOk { path: p, framebuffer }) => {
                self.latest_frame = Some(framebuffer);
                log::info!("Loaded state from {}", p);
            }
            Some(EmuResponse::LoadStateFailed(err)) => {
                log::error!("Failed to load state from {}: {}", path.display(), err);
            }
            _ => {}
        }
    }

    pub(super) fn handle_dropped_file(&mut self, path: PathBuf) {
        self.load_rom(&path);
    }
}
