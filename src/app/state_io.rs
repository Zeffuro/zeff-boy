use super::App;
use crate::debug::FpsTracker;
use crate::emu_backend::{ActiveSystem, EmuBackend};
use crate::emu_thread::{EmuCommand, EmuResponse, EmuThread};
use zeff_gb_core::emulator::Emulator;
use std::path::{Path, PathBuf};
use std::time::Instant;

pub(super) fn build_slot_labels(rom_hash: Option<[u8; 32]>) -> [String; 10] {
    std::array::from_fn(|i| {
        let slot = i as u8;
        let Some(hash) = rom_hash else {
            return format!("Slot {slot}  (empty)");
        };
        let Ok(path) = zeff_gb_core::save_state::slot_path(hash, slot) else {
            return format!("Slot {slot}  (empty)");
        };
        match std::fs::metadata(&path) {
            Ok(meta) => {
                if let Ok(modified) = meta.modified() {
                    let dt: chrono::DateTime<chrono::Local> = modified.into();
                    let stamp = dt.format("%Y-%m-%d %H:%M");
                    format!("Slot {slot}  ({stamp})")
                } else {
                    format!("Slot {slot}")
                }
            }
            Err(_) => format!("Slot {slot}  (empty)"),
        }
    })
}

impl App {
    fn pause_for_dialog(&mut self) -> bool {
        let was_paused = self.paused;
        self.paused = true;
        was_paused
    }

    fn resume_after_dialog(&mut self, was_paused: bool) {
        self.paused = was_paused;
        self.timing.last_frame_time = Instant::now();
    }

    fn load_rom_with_options(&mut self, path: &Path, auto_load_state: bool) {
        self.stop_emu_thread();

        self.frames_in_flight = 0;
        self.cached_ui_data = None;
        self.recycled.clear();
        self.debug_windows.last_disasm_pc = None;

        let system = ActiveSystem::from_path(path).unwrap_or(ActiveSystem::GameBoy);

        let backend_result: anyhow::Result<EmuBackend> = match system {
            ActiveSystem::GameBoy => {
                Emulator::from_rom_with_mode(path, self.settings.hardware_mode_preference)
                    .map_err(|e| anyhow::anyhow!("{e}"))
                    .map(|mut emu| {
                        if let Some(audio) = &self.audio {
                            emu.bus.set_apu_sample_rate(audio.sample_rate());
                        }
                        let buttons = self.host_input.buttons_pressed();
                        let dpad = self.host_input.dpad_pressed();
                        emu.bus.apply_joypad_pressed_masks(buttons, dpad);
                        EmuBackend::from_gb(emu)
                    })
            }
            ActiveSystem::Nes => {
                let rom_data = std::fs::read(path)
                    .map_err(|e| anyhow::anyhow!("Failed to read NES ROM: {e}"));
                match rom_data {
                    Ok(data) => {
                        let sample_rate = self.audio.as_ref()
                            .map(|a| a.sample_rate() as f64)
                            .unwrap_or(48000.0);
                        zeff_nes_core::emulator::Emulator::new(
                            &data,
                            path.to_path_buf(),
                            sample_rate,
                        ).map(EmuBackend::from_nes)
                    }
                    Err(e) => Err(e),
                }
            }
        };

        match backend_result {
            Ok(backend) => {
                let rom_name = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("ROM")
                    .to_string();
                log::info!("Loaded ROM: {}", path.display());

                self.cached_is_mbc7 = backend.is_mbc7();
                self.cached_rom_path = Some(backend.rom_path().to_path_buf());
                self.cached_rom_hash = backend.rom_hash();
                self.active_system = system;

                // Update framebuffer texture size when system changes
                let (native_w, native_h) = system.screen_size();
                if let Some(gfx) = self.gfx.as_mut() {
                    gfx.set_native_size(native_w, native_h);
                }

                // GB-specific setup (cheats, libretro, header info)
                if let Some(gb_emu) = backend.gb() {
                    let rom_header_title = gb_emu.header.title.clone();
                    let is_gbc = gb_emu.header.is_cgb_compatible || gb_emu.header.is_cgb_exclusive;
                    let rom_crc32 = crc32fast::hash(gb_emu.bus.cartridge.rom_bytes());
                    let libretro_meta = crate::libretro_metadata::lookup_cached(rom_crc32, is_gbc);
                    let search_hints = crate::libretro_metadata::build_cheat_search_hints(
                        &rom_header_title,
                        libretro_meta.as_ref(),
                    );

                    if let Some(ref old_title) = self.debug_windows.cheat.rom_title {
                        crate::cheats::save_game_cheats(
                            Some(old_title),
                            self.debug_windows.cheat.rom_crc32,
                            &self.debug_windows.cheat.user_codes,
                            &self.debug_windows.cheat.libretro_codes,
                        );
                    }

                    self.debug_windows.cheat.rom_title = Some(rom_header_title.clone());
                    self.debug_windows.cheat.rom_crc32 = Some(rom_crc32);
                    self.debug_windows.cheat.rom_metadata_title =
                        libretro_meta.as_ref().map(|m| m.title.clone());
                    self.debug_windows.cheat.rom_metadata_rom_name =
                        libretro_meta.as_ref().map(|m| m.rom_name.clone());
                    self.debug_windows.cheat.rom_is_gbc = is_gbc;
                    self.debug_windows.cheat.libretro_search_hints = search_hints;
                    self.debug_windows.cheat.libretro_search = self
                        .debug_windows
                        .cheat
                        .libretro_search_hints
                        .first()
                        .cloned()
                        .unwrap_or_else(|| rom_header_title.clone());
                    self.debug_windows.cheat.libretro_results.clear();
                    self.debug_windows.cheat.libretro_file_list = None;
                    self.debug_windows.cheat.libretro_status = None;

                    let (user, libretro) =
                        crate::cheats::load_game_cheats(Some(&rom_header_title), Some(rom_crc32));
                    self.debug_windows.cheat.user_codes = user;
                    self.debug_windows.cheat.libretro_codes = libretro;
                    self.debug_windows.cheat.cheats_dirty = true;
                } else {
                    // NES: clear GB-specific cheat state
                    self.debug_windows.cheat.rom_title = None;
                    self.debug_windows.cheat.rom_crc32 = None;
                    self.debug_windows.cheat.rom_metadata_title = None;
                    self.debug_windows.cheat.rom_metadata_rom_name = None;
                    self.debug_windows.cheat.libretro_search_hints.clear();
                    self.debug_windows.cheat.libretro_search.clear();
                    self.debug_windows.cheat.libretro_results.clear();
                    self.debug_windows.cheat.libretro_file_list = None;
                    self.debug_windows.cheat.libretro_status = None;
                    self.debug_windows.cheat.user_codes.clear();
                    self.debug_windows.cheat.libretro_codes.clear();
                }

                self.emu_thread = Some(EmuThread::spawn(backend));
                self.fps_tracker = FpsTracker::new();
                self.timing.last_frame_time = Instant::now();

                if self.timing.uncapped_speed
                    && let Some(thread) = &self.emu_thread {
                        thread.send(EmuCommand::SetUncapped(true));
                    }

                self.settings.add_recent_rom(path);
                self.settings.save();
                self.toast_manager.info(format!("Loaded {rom_name}"));

                if auto_load_state && self.settings.auto_save_state && system == ActiveSystem::GameBoy {
                    if let Some(thread) = &self.emu_thread {
                        thread.send(EmuCommand::AutoLoadState {
                            buttons_pressed: self.host_input.buttons_pressed(),
                            dpad_pressed: self.host_input.dpad_pressed(),
                        });
                    }
                    match self.recv_cold_response() {
                        Some(EmuResponse::LoadStateOk {
                            path: p,
                            framebuffer,
                        }) => {
                            self.latest_frame = Some(framebuffer);
                            log::info!("Auto-loaded state from {}", p);
                            self.toast_manager.success("Resumed from auto-save");
                        }
                        Some(EmuResponse::LoadStateFailed(_)) => {}
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
                self.toast_manager
                    .error(format!("No save found in Slot {slot}"));
            }
            _ => {}
        }
    }

    pub(super) fn load_rom(&mut self, path: &Path) {
        self.load_rom_with_options(path, true);
    }

    pub(super) fn reset_game(&mut self) {
        let Some(path) = self.cached_rom_path.clone() else {
            self.toast_manager.info("No ROM loaded");
            return;
        };

        self.load_rom_with_options(&path, false);
        self.toast_manager.success("Game reset");
    }

    pub(super) fn stop_game(&mut self) {
        if self.cached_rom_path.is_none() && self.emu_thread.is_none() {
            self.toast_manager.info("No ROM loaded");
            return;
        }

        if let Some(ref title) = self.debug_windows.cheat.rom_title {
            crate::cheats::save_game_cheats(
                Some(title),
                self.debug_windows.cheat.rom_crc32,
                &self.debug_windows.cheat.user_codes,
                &self.debug_windows.cheat.libretro_codes,
            );
        }

        self.stop_emu_thread();

        if let Some(gfx) = self.gfx.as_ref() {
            gfx.clear_framebuffer();
        }

        self.frames_in_flight = 0;
        self.cached_ui_data = None;
        self.recycled.clear();
        self.latest_frame = None;
        self.last_displayed_frame = None;
        self.cached_rom_path = None;
        self.cached_rom_hash = None;
        self.cached_is_mbc7 = false;
        self.paused = false;
        self.rewind.held = false;
        self.rewind.fill = 0.0;
        self.rewind.throttle = 0;
        self.rewind.pops = 0;
        self.rewind.pending = false;
        self.rewind.backstep_pending = false;

        self.debug_windows.cheat.rom_title = None;
        self.debug_windows.cheat.rom_crc32 = None;
        self.debug_windows.cheat.rom_metadata_title = None;
        self.debug_windows.cheat.rom_metadata_rom_name = None;
        self.debug_windows.cheat.libretro_search_hints.clear();
        self.debug_windows.cheat.libretro_search.clear();
        self.debug_windows.cheat.libretro_results.clear();
        self.debug_windows.cheat.libretro_file_list = None;
        self.debug_windows.cheat.libretro_status = None;
        self.debug_windows.cheat.user_codes.clear();
        self.debug_windows.cheat.libretro_codes.clear();

        self.toast_manager.set_persistent(
            "paused",
            false,
            "⏸ Paused",
            egui::Color32::from_rgba_unmultiplied(50, 50, 90, 220),
            false,
        );
        self.toast_manager.success("Stopped emulation");
    }

    pub(super) fn open_file_dialog(&mut self) {
        let was_paused = self.pause_for_dialog();
        let file = rfd::FileDialog::new()
            .add_filter("ROMs", &["gb", "gbc", "nes"])
            .add_filter("Game Boy ROMs", &["gb", "gbc"])
            .add_filter("NES ROMs", &["nes"])
            .add_filter("All files", &["*"])
            .set_title("Open ROM")
            .pick_file();

        self.resume_after_dialog(was_paused);
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

        if let Some(rom_path) = &self.cached_rom_path
            && let Some(parent) = rom_path.parent() {
                return parent.to_path_buf();
            }

        Self::default_save_state_dir()
    }

    pub(super) fn save_state_file_dialog(&mut self) {
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

    pub(super) fn load_state_file_dialog(&mut self) {
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
                framebuffer,
            }) => {
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
        let (native_w, native_h) = self.active_system.screen_size();
        let expected_len = (native_w * native_h * 4) as usize;
        let fb = match &self.last_displayed_frame {
            Some(fb) if fb.len() == expected_len => fb,
            _ => {
                self.toast_manager.error("No framebuffer available");
                return;
            }
        };

        let game_name = self
            .cached_rom_path
            .as_ref()
            .and_then(|p| p.file_stem())
            .and_then(|s| s.to_str())
            .unwrap_or("screenshot");

        let now = chrono::Local::now();
        let timestamp = now.format("%Y-%m-%d_%H-%M-%S");
        let filename = format!("{game_name}_{timestamp}.png");

        let dir = Self::screenshots_dir();
        if let Err(e) = std::fs::create_dir_all(&dir) {
            self.toast_manager
                .error(format!("Can't create screenshots dir: {e}"));
            return;
        }
        let path = dir.join(&filename);

        let image = egui::ColorImage::from_rgba_unmultiplied([native_w as usize, native_h as usize], fb);

        match crate::debug::export::export_color_image_as_png(&path, &image) {
            Ok(()) => {
                log::info!("Screenshot saved to {}", path.display());
                self.toast_manager.success(format!("📸 {filename}"));
            }
            Err(err) => {
                log::error!("Failed to save screenshot: {}", err);
                self.toast_manager
                    .error(format!("Screenshot failed: {err}"));
            }
        }
    }

    fn screenshots_dir() -> PathBuf {
        if let Some(config_dir) = dirs::config_dir() {
            return config_dir.join("zeff-boy").join("screenshots");
        }
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join("screenshots")
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

        let was_paused = self.pause_for_dialog();
        let file = rfd::FileDialog::new()
            .set_title("Save Audio Recording")
            .set_directory(self.state_dialog_dir())
            .add_filter(format.label(), &[ext])
            .set_file_name(&default_name)
            .save_file();

        self.resume_after_dialog(was_paused);
        let Some(path) = file else {
            return;
        };

        match crate::audio_recorder::AudioRecorder::start(&path, sample_rate, format) {
            Ok(recorder) => {
                log::info!("Started audio recording to {}", path.display());
                self.toast_manager.info("Recording audio...");
                self.recording.audio_recorder = Some(recorder);
            }
            Err(err) => {
                log::error!("Failed to start recording: {}", err);
                self.toast_manager.error(format!("Record failed: {err}"));
            }
        }
    }

    pub(super) fn stop_audio_recording(&mut self) {
        if let Some(recorder) = self.recording.audio_recorder.take() {
            match recorder.finish() {
                Ok(path) => {
                    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("file");
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

        let was_paused = self.pause_for_dialog();
        let file = rfd::FileDialog::new()
            .set_title("Save Replay")
            .set_directory(self.state_dialog_dir())
            .add_filter("Zeff Boy Replay", &["zrpl"])
            .set_file_name(&default_name)
            .save_file();

        self.resume_after_dialog(was_paused);
        let Some(path) = file else {
            return;
        };

        // Capture current state bytes from emu thread
        if let Some(thread) = &self.emu_thread {
            thread.send(crate::emu_thread::EmuCommand::CaptureStateBytes);
        }
        match self.recv_cold_response() {
            Some(EmuResponse::StateCaptured(state_bytes)) => {
                let recorder = zeff_gb_core::replay::ReplayRecorder::new(path, state_bytes);
                self.recording.replay_recorder = Some(recorder);
                self.toast_manager.set_replay_recording(true);
            }
            Some(EmuResponse::StateCaptureFailed(err)) => {
                log::error!("Failed to capture state for replay: {}", err);
                self.toast_manager
                    .error(format!("Replay start failed: {err}"));
            }
            _ => {}
        }
    }

    pub(super) fn stop_replay_recording(&mut self) {
        if let Some(recorder) = self.recording.replay_recorder.take() {
            self.toast_manager.set_replay_recording(false);
            let frame_count = recorder.frame_count();
            match recorder.finish() {
                Ok(path) => {
                    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("file");
                    log::info!(
                        "Replay saved to {} ({} frames)",
                        path.display(),
                        frame_count
                    );
                    self.toast_manager
                        .success(format!("Saved {name} ({frame_count} frames)"));
                }
                Err(err) => {
                    log::error!("Failed to save replay: {}", err);
                    self.toast_manager
                        .error(format!("Replay save failed: {err}"));
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

        match zeff_gb_core::replay::ReplayPlayer::load(&path) {
            Ok(player) => {
                let total = player.total_frames();
                let state_bytes = player.save_state().to_vec();
                if let Some(thread) = &self.emu_thread {
                    thread.send(EmuCommand::LoadStateBytes {
                        state_bytes,
                        buttons_pressed: 0,
                        dpad_pressed: 0,
                    });
                }
                match self.recv_cold_response() {
                    Some(EmuResponse::LoadStateOk { framebuffer, .. }) => {
                        self.latest_frame = Some(framebuffer);
                        self.recording.replay_player = Some(player);
                        self.toast_manager
                            .info(format!("Playing replay ({total} frames)"));
                    }
                    Some(EmuResponse::LoadStateFailed(err)) => {
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
