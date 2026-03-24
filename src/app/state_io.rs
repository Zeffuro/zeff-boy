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
                self.toast_manager.success(format!("Saved to slot {slot}"));
            }
            Some(EmuResponse::SaveStateFailed(err)) => {
                log::error!("Failed to save state in slot {}: {}", slot, err);
                self.toast_manager.error(format!("Save failed: {err}"));
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
                self.toast_manager.success(format!("Loaded slot {slot}"));
            }
            Some(EmuResponse::LoadStateFailed(err)) => {
                log::error!("Failed to load state from slot {}: {}", slot, err);
                self.toast_manager.error(format!("Load failed: {err}"));
            }
            _ => {}
        }
    }

    pub(super) fn load_rom(&mut self, path: &Path) {
        self.stop_emu_thread();

        self.frames_in_flight = 0;
        self.cached_ui_data = None;
        self.recycled_framebuffer = None;
        self.recycled_audio_buffer = None;
        self.recycled_vram_buffer = None;
        self.recycled_oam_buffer = None;
        self.recycled_memory_page = None;
        self.debug_windows.last_disasm_pc = None;

        match Emulator::from_rom_with_mode(path, self.settings.hardware_mode_preference) {
            Ok(mut emu) => {
                if let Some(audio) = &self.audio {
                    emu.bus.io.apu.set_sample_rate(audio.sample_rate());
                }
                let buttons = self.host_input.buttons_pressed();
                let dpad = self.host_input.dpad_pressed();
                emu.bus.io.joypad.apply_pressed_masks(buttons, dpad);

                let rom_name = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("ROM")
                    .to_string();
                log::info!("Loaded ROM: {}", path.display());
                
                self.cached_is_mbc7 = emu.is_mbc7_cartridge();
                self.cached_rom_path = Some(emu.rom_path().to_path_buf());

                let rom_header_title = emu.header.title.clone();
                let is_gbc = emu.header.is_cgb_compatible || emu.header.is_cgb_exclusive;

                if let Some(ref old_title) = self.debug_windows.cheat.rom_title {
                    crate::cheats::save_game_cheats(
                        old_title,
                        &self.debug_windows.cheat.user_codes,
                        &self.debug_windows.cheat.libretro_codes,
                    );
                }

                self.debug_windows.cheat.rom_title = Some(rom_header_title.clone());
                self.debug_windows.cheat.rom_is_gbc = is_gbc;
                self.debug_windows.cheat.libretro_search = rom_header_title.clone();
                self.debug_windows.cheat.libretro_results.clear();
                self.debug_windows.cheat.libretro_file_list = None;
                self.debug_windows.cheat.libretro_status = None;

                let (user, libretro) = crate::cheats::load_game_cheats(&rom_header_title);
                self.debug_windows.cheat.user_codes = user;
                self.debug_windows.cheat.libretro_codes = libretro;

                self.emu_thread = Some(EmuThread::spawn(emu));
                self.fps_tracker = FpsTracker::new();
                self.last_frame_time = Instant::now();

                if self.uncapped_speed {
                    if let Some(thread) = &self.emu_thread {
                        thread.send(EmuCommand::SetUncapped(true));
                    }
                }

                self.settings.add_recent_rom(path);
                self.settings.save();
                self.toast_manager.info(format!("Loaded {rom_name}"));

                if self.settings.auto_save_state {
                    if let Some(thread) = &self.emu_thread {
                        thread.send(EmuCommand::AutoLoadState {
                            buttons_pressed: self.host_input.buttons_pressed(),
                            dpad_pressed: self.host_input.dpad_pressed(),
                        });
                    }
                    match self.recv_cold_response() {
                        Some(EmuResponse::LoadStateOk { path: p, framebuffer }) => {
                            self.latest_frame = Some(framebuffer);
                            log::info!("Auto-loaded state from {}", p);
                            self.toast_manager.success("Resumed from auto-save");
                        }
                        Some(EmuResponse::LoadStateFailed(_)) => {
                        }
                        _ => {}
                    }
                }
            }
            Err(e) => {
                log::error!("Failed to load ROM '{}': {}", path.display(), e);
                self.toast_manager.error(format!("Failed to load ROM: {e}"));
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
                self.toast_manager.success("State saved to file");
            }
            Some(EmuResponse::SaveStateFailed(err)) => {
                log::error!("Failed to save state to {}: {}", path.display(), err);
                self.toast_manager.error(format!("Save failed: {err}"));
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
                self.toast_manager.success("State loaded from file");
            }
            Some(EmuResponse::LoadStateFailed(err)) => {
                log::error!("Failed to load state from {}: {}", path.display(), err);
                self.toast_manager.error(format!("Load failed: {err}"));
            }
            _ => {}
        }
    }

    pub(super) fn handle_dropped_file(&mut self, path: PathBuf) {
        self.load_rom(&path);
    }
    pub(super) fn take_screenshot(&mut self) {
        let fb = match &self.last_displayed_frame {
            Some(fb) if fb.len() == 160 * 144 * 4 => fb,
            _ => {
                self.toast_manager.error("No framebuffer available");
                return;
            }
        };

        let default_name = self
            .cached_rom_path
            .as_ref()
            .and_then(|p| p.file_stem())
            .and_then(|s| s.to_str())
            .map(|stem| format!("{stem}.png"))
            .unwrap_or_else(|| "screenshot.png".to_string());

        let file = rfd::FileDialog::new()
            .set_title("Save Screenshot")
            .set_directory(self.state_dialog_dir())
            .add_filter("PNG Image", &["png"])
            .set_file_name(&default_name)
            .save_file();

        let Some(path) = file else {
            return;
        };

        let image = egui::ColorImage::from_rgba_unmultiplied([160, 144], fb);

        match crate::debug::export::export_color_image_as_png(&path, &image) {
            Ok(()) => {
                let name = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("file");
                log::info!("Screenshot saved to {}", path.display());
                self.toast_manager.success(format!("Saved {name}"));
            }
            Err(err) => {
                log::error!("Failed to save screenshot: {}", err);
                self.toast_manager.error(format!("Screenshot failed: {err}"));
            }
        }
    }

    pub(super) fn start_audio_recording(&mut self) {
        let sample_rate = self
            .audio
            .as_ref()
            .map(|a| a.sample_rate())
            .unwrap_or(48_000);

        let format = self.settings.audio_recording_format;
        let ext = format.extension();

        let default_name = self
            .cached_rom_path
            .as_ref()
            .and_then(|p| p.file_stem())
            .and_then(|s| s.to_str())
            .map(|stem| format!("{stem}.{ext}"))
            .unwrap_or_else(|| format!("recording.{ext}"));

        let file = rfd::FileDialog::new()
            .set_title("Save Audio Recording")
            .set_directory(self.state_dialog_dir())
            .add_filter(format.label(), &[ext])
            .set_file_name(&default_name)
            .save_file();

        let Some(path) = file else {
            return;
        };

        match crate::audio_recorder::AudioRecorder::start(&path, sample_rate, format) {
            Ok(recorder) => {
                log::info!("Started audio recording to {}", path.display());
                self.toast_manager.info("Recording audio...");
                self.audio_recorder = Some(recorder);
            }
            Err(err) => {
                log::error!("Failed to start recording: {}", err);
                self.toast_manager.error(format!("Record failed: {err}"));
            }
        }
    }

    pub(super) fn stop_audio_recording(&mut self) {
        if let Some(recorder) = self.audio_recorder.take() {
            match recorder.finish() {
                Ok(path) => {
                    let name = path
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("file");
                    log::info!("Audio saved to {}", path.display());
                    self.toast_manager.success(format!("Saved {name}"));
                }
                Err(err) => {
                    log::error!("Failed to finalize recording: {}", err);
                    self.toast_manager.error(format!("Recording error: {err}"));
                }
            }
        }
    }

    pub(super) fn start_replay_recording(&mut self) {
        if self.emu_thread.is_none() {
            return;
        }

        let default_name = self
            .cached_rom_path
            .as_ref()
            .and_then(|p| p.file_stem())
            .and_then(|s| s.to_str())
            .map(|stem| format!("{stem}.zrpl"))
            .unwrap_or_else(|| "replay.zrpl".to_string());

        let file = rfd::FileDialog::new()
            .set_title("Save Replay")
            .set_directory(self.state_dialog_dir())
            .add_filter("Zeff Boy Replay", &["zrpl"])
            .set_file_name(&default_name)
            .save_file();

        let Some(path) = file else {
            return;
        };

        // Capture current state bytes from emu thread
        if let Some(thread) = &self.emu_thread {
            thread.send(crate::emu_thread::EmuCommand::CaptureStateBytes);
        }
        match self.recv_cold_response() {
            Some(crate::emu_thread::EmuResponse::StateCaptured(state_bytes)) => {
                let recorder = crate::replay::ReplayRecorder::new(path, state_bytes);
                self.replay_recorder = Some(recorder);
                self.toast_manager.info("Recording replay...");
            }
            Some(crate::emu_thread::EmuResponse::StateCaptureFailed(err)) => {
                log::error!("Failed to capture state for replay: {}", err);
                self.toast_manager.error(format!("Replay start failed: {err}"));
            }
            _ => {}
        }
    }

    pub(super) fn stop_replay_recording(&mut self) {
        if let Some(recorder) = self.replay_recorder.take() {
            let frame_count = recorder.frame_count();
            match recorder.finish() {
                Ok(path) => {
                    let name = path
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("file");
                    log::info!("Replay saved to {} ({} frames)", path.display(), frame_count);
                    self.toast_manager
                        .success(format!("Saved {name} ({frame_count} frames)"));
                }
                Err(err) => {
                    log::error!("Failed to save replay: {}", err);
                    self.toast_manager.error(format!("Replay save failed: {err}"));
                }
            }
        }
    }

    pub(super) fn load_and_play_replay(&mut self) {
        if self.emu_thread.is_none() {
            return;
        }

        let file = rfd::FileDialog::new()
            .set_title("Load Replay")
            .set_directory(self.state_dialog_dir())
            .add_filter("Zeff Boy Replay", &["zrpl"])
            .pick_file();

        let Some(path) = file else {
            return;
        };

        match crate::replay::ReplayPlayer::load(&path) {
            Ok(player) => {
                let total = player.total_frames();
                // Load the save state from the replay
                let state_bytes = player.save_state().to_vec();
                if let Some(thread) = &self.emu_thread {
                    thread.send(crate::emu_thread::EmuCommand::LoadStateBytes {
                        state_bytes,
                        buttons_pressed: 0,
                        dpad_pressed: 0,
                    });
                }
                match self.recv_cold_response() {
                    Some(crate::emu_thread::EmuResponse::LoadStateOk { framebuffer, .. }) => {
                        self.latest_frame = Some(framebuffer);
                        self.replay_player = Some(player);
                        self.toast_manager
                            .info(format!("Playing replay ({total} frames)"));
                    }
                    Some(crate::emu_thread::EmuResponse::LoadStateFailed(err)) => {
                        log::error!("Failed to load replay state: {}", err);
                        self.toast_manager
                            .error(format!("Replay load failed: {err}"));
                    }
                    _ => {}
                }
            }
            Err(err) => {
                log::error!("Failed to load replay: {}", err);
                self.toast_manager
                    .error(format!("Replay load failed: {err}"));
            }
        }
    }
}
