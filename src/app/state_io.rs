use super::App;
use crate::debug::FpsTracker;
use crate::emulator::Emulator;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Instant;

impl App {
    pub(super) fn save_state_slot(&mut self, slot: u8) {
        let Some(emu) = self.emulator.as_ref() else {
            return;
        };
        let emu = emu.lock().expect("emulator mutex poisoned");
        match emu.save_state(slot) {
            Ok(path) => log::info!("Saved state to {}", path),
            Err(err) => log::error!("Failed to save state in slot {}: {}", slot, err),
        }
    }

    pub(super) fn load_state_slot(&mut self, slot: u8) {
        let Some(emu) = self.emulator.as_ref() else {
            return;
        };
        let mut emu = emu.lock().expect("emulator mutex poisoned");
        match emu.load_state(slot) {
            Ok(path) => {
                self.apply_host_input_to_joypad(&mut emu);
                self.latest_frame = Some(emu.framebuffer().to_vec());
                log::info!("Loaded state from {}", path);
            }
            Err(err) => log::error!("Failed to load state from slot {}: {}", slot, err),
        }
    }

    pub(super) fn load_rom(&mut self, path: &Path) {
        self.stop_emu_thread();
        if let Some(current) = self.emulator.as_ref() {
            let current = current.lock().expect("emulator mutex poisoned");
            match current.flush_battery_sram() {
                Ok(Some(saved)) => log::info!("Saved battery RAM to {}", saved),
                Ok(None) => {}
                Err(err) => log::error!("Failed to save battery RAM before ROM switch: {}", err),
            }
        }

        match Emulator::from_rom_with_mode(path, self.settings.hardware_mode_preference) {
            Ok(mut emu) => {
                if let Some(audio) = &self.audio {
                    emu.bus.io.apu.set_sample_rate(audio.sample_rate());
                }
                self.apply_host_input_to_joypad(&mut emu);
                log::info!("Loaded ROM: {}", path.display());
                self.emulator = Some(Arc::new(Mutex::new(emu)));
                self.ensure_emu_thread();
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
        let Some(emu) = self.emulator.as_ref() else {
            return "save.state".to_string();
        };
        let emu = emu.lock().expect("emulator mutex poisoned");
        emu.rom_path()
            .file_stem()
            .and_then(|s| s.to_str())
            .map(|stem| format!("{stem}.state"))
            .unwrap_or_else(|| "save.state".to_string())
    }

    fn state_dialog_dir(&self) -> PathBuf {
        if let Some(dir) = &self.last_state_dir {
            return dir.clone();
        }

        if let Some(emu) = self.emulator.as_ref() {
            let emu = emu.lock().expect("emulator mutex poisoned");
            if let Some(parent) = emu.rom_path().parent() {
                return parent.to_path_buf();
            }
        }

        Self::default_save_state_dir()
    }

    pub(super) fn save_state_file_dialog(&mut self) {
        let Some(_emu) = self.emulator.as_ref() else {
            return;
        };

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

        let Some(emu) = self.emulator.as_ref() else {
            return;
        };
        let emu = emu.lock().expect("emulator mutex poisoned");

        match emu.save_state_to_path(&path) {
            Ok(()) => log::info!("Saved state to {}", path.display()),
            Err(err) => log::error!("Failed to save state to {}: {}", path.display(), err),
        }
    }

    pub(super) fn load_state_file_dialog(&mut self) {
        let Some(_emu) = self.emulator.as_ref() else {
            return;
        };

        let file = rfd::FileDialog::new()
            .set_title("Load State")
            .set_directory(self.state_dialog_dir())
            .add_filter("Zeff Boy Save State", &["state"])
            .pick_file();

        let Some(path) = file else {
            return;
        };

        self.last_state_dir = path.parent().map(|p| p.to_path_buf());

        let Some(emu) = self.emulator.as_ref() else {
            return;
        };
        let mut emu = emu.lock().expect("emulator mutex poisoned");

        match emu.load_state_from_path(&path) {
            Ok(()) => {
                self.apply_host_input_to_joypad(&mut emu);
                self.latest_frame = Some(emu.framebuffer().to_vec());
                log::info!("Loaded state from {}", path.display());
            }
            Err(err) => log::error!("Failed to load state from {}: {}", path.display(), err),
        }
    }

    pub(super) fn handle_dropped_file(&mut self, path: PathBuf) {
        self.load_rom(&path);
    }
}
