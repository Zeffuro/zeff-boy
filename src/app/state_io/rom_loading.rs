use super::App;
use crate::debug::FpsTracker;
use crate::emu_backend::{ActiveSystem, EmuBackend};
use crate::emu_thread::{EmuCommand, EmuResponse, EmuThread};
use anyhow::Context;
use std::path::{Path, PathBuf};
#[cfg(not(target_arch = "wasm32"))]
use std::time::Instant;
#[cfg(target_arch = "wasm32")]
use web_time::Instant;
use zeff_gb_core::emulator::Emulator;

#[cfg(not(target_arch = "wasm32"))]
fn apply_mods_if_any(system: ActiveSystem, rom_data: &mut Vec<u8>) -> u32 {
    let crc = crc32fast::hash(rom_data);
    let dir = crate::mods::mods_dir_for_rom(system, crc);
    let mods = crate::mods::load_mod_config(&dir);
    let enabled = mods.iter().filter(|m| m.enabled).count();
    if enabled > 0 {
        let warnings = crate::mods::apply_enabled_mods(rom_data, &dir, &mods);
        for w in &warnings {
            log::warn!("Mod warning: {w}");
        }
        log::info!(
            "Applied {enabled} IPS mod(s) to ROM ({} warnings)",
            warnings.len()
        );
    }
    crc
}

#[cfg(target_arch = "wasm32")]
fn apply_mods_if_any(_system: ActiveSystem, rom_data: &mut Vec<u8>) -> u32 {
    crc32fast::hash(rom_data)
}

fn detect_and_extract_rom(path: &Path) -> anyhow::Result<(PathBuf, Option<Vec<u8>>, ActiveSystem)> {
    let is_zip = path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("zip"));

    let (rom_path, preloaded_data) = if is_zip {
        let (virtual_path, data) = super::extract_rom_from_zip(path)
            .with_context(|| format!("Failed to extract ROM from '{}'", path.display()))?;
        log::info!(
            "Extracted ROM '{}' ({} bytes) from ZIP",
            virtual_path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy(),
            data.len()
        );
        (virtual_path, Some(data))
    } else if !path.exists() {
        anyhow::bail!(
            "File not found: '{}'. Check that the path is correct.",
            path.display()
        );
    } else {
        (path.to_path_buf(), None)
    };

    let system = ActiveSystem::from_path(&rom_path).ok_or_else(|| {
        let ext = rom_path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("(none)");
        anyhow::anyhow!(
            "Unsupported file type '.{ext}'. Supported extensions: {}",
            ActiveSystem::supported_extensions()
        )
    })?;

    Ok((rom_path, preloaded_data, system))
}

impl App {
    fn init_backend(
        &self,
        system: ActiveSystem,
        path: &Path,
        rom_path: &Path,
        preloaded_data: Option<Vec<u8>>,
    ) -> anyhow::Result<(EmuBackend, u32)> {
        match system {
            ActiveSystem::GameBoy => {
                let mut rom_data = match preloaded_data {
                    Some(data) => data,
                    None => std::fs::read(path).context("Failed to read GB ROM")?,
                };
                let original_crc = apply_mods_if_any(system, &mut rom_data);
                Emulator::from_rom_data(&rom_data, self.settings.emulation.hardware_mode_preference)
                    .map(|mut emu| {
                        if let Some(audio) = &self.audio {
                            emu.set_sample_rate(audio.sample_rate());
                        }
                        let buttons = self.host_input.buttons_pressed();
                        let dpad = self.host_input.dpad_pressed();
                        emu.set_input(buttons, dpad);
                        if let Some(sram_path) =
                            crate::emu_backend::gb::try_load_battery_sram(&mut emu, rom_path)
                                .unwrap_or_else(|e| {
                                    log::warn!("Failed to load battery save: {e}");
                                    None
                                })
                        {
                            log::info!("Loaded battery save from {}", sram_path);
                        }
                        (
                            EmuBackend::from_gb(emu, rom_path.to_path_buf()),
                            original_crc,
                        )
                    })
            }
            ActiveSystem::Nes => {
                let rom_data = match preloaded_data {
                    Some(data) => Ok(data),
                    None => std::fs::read(path).context("Failed to read NES ROM"),
                };
                match rom_data {
                    Ok(mut data) => {
                        let original_crc = apply_mods_if_any(system, &mut data);
                        let sample_rate = self
                            .audio
                            .as_ref()
                            .map(|a| a.sample_rate() as f64)
                            .unwrap_or(zeff_nes_core::emulator::DEFAULT_SAMPLE_RATE);
                        zeff_nes_core::emulator::Emulator::new(&data, sample_rate).map(|mut emu| {
                            if let Some(sram_path) =
                                crate::emu_backend::nes::try_load_battery_sram(&mut emu, rom_path)
                                    .unwrap_or_else(|e| {
                                        log::warn!("Failed to load battery save: {e}");
                                        None
                                    })
                            {
                                log::info!("Loaded battery save from {}", sram_path);
                            }
                            (
                                EmuBackend::from_nes(emu, rom_path.to_path_buf()),
                                original_crc,
                            )
                        })
                    }
                    Err(e) => Err(e),
                }
            }
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn setup_cheats_for_rom(&mut self, system: ActiveSystem, path: &Path, backend: &EmuBackend) {
        if let Some(ref old_title) = self.debug_windows.cheat.rom_title {
            crate::cheats::save_game_cheats(
                self.debug_windows.cheat.active_system,
                Some(old_title),
                self.debug_windows.cheat.rom_crc32,
                &self.debug_windows.cheat.user_codes,
                &self.debug_windows.cheat.libretro_codes,
            );
        }

        self.debug_windows.cheat.active_system = system;

        if let Some(gb) = backend.gb() {
            let rom_header_title = gb.emu.header().title.clone();
            let is_gbc = gb.emu.header().is_cgb_compatible || gb.emu.header().is_cgb_exclusive;
            let rom_crc32 = crc32fast::hash(gb.emu.cartridge_rom_bytes());
            let platform = if is_gbc {
                crate::libretro_common::LibretroPlatform::Gbc
            } else {
                crate::libretro_common::LibretroPlatform::Gb
            };
            let libretro_meta = crate::libretro_metadata::lookup_cached(rom_crc32, platform);
            let search_hints = crate::libretro_metadata::build_cheat_search_hints(
                &rom_header_title,
                libretro_meta.as_ref(),
            );

            self.debug_windows.cheat.rom_title = Some(rom_header_title.clone());
            self.debug_windows.cheat.rom_crc32 = Some(rom_crc32);
            self.debug_windows.cheat.rom_metadata_title =
                libretro_meta.as_ref().map(|m| m.title.clone());
            self.debug_windows.cheat.rom_metadata_rom_name =
                libretro_meta.as_ref().map(|m| m.rom_name.clone());
            self.debug_windows.cheat.libretro_platform = platform;
            self.debug_windows.cheat.libretro_search_hints = search_hints;
            self.debug_windows.cheat.libretro_search = self
                .debug_windows
                .cheat
                .libretro_search_hints
                .first()
                .cloned()
                .unwrap_or_else(|| rom_header_title.clone());

            let (user, libretro) =
                crate::cheats::load_game_cheats(system, Some(&rom_header_title), Some(rom_crc32));
            self.debug_windows.cheat.user_codes = user;
            self.debug_windows.cheat.libretro_codes = libretro;
        } else if system == ActiveSystem::Nes {
            let rom_title = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("NES ROM")
                .to_string();
            let rom_crc32 = backend.nes().map(|nes| nes.emu.rom_crc32());
            let platform = crate::libretro_common::LibretroPlatform::Nes;
            let libretro_meta =
                rom_crc32.and_then(|crc| crate::libretro_metadata::lookup_cached(crc, platform));
            let search_hints = crate::libretro_metadata::build_cheat_search_hints(
                &rom_title,
                libretro_meta.as_ref(),
            );

            self.debug_windows.cheat.rom_title = Some(rom_title.clone());
            self.debug_windows.cheat.rom_crc32 = rom_crc32;
            self.debug_windows.cheat.rom_metadata_title =
                libretro_meta.as_ref().map(|m| m.title.clone());
            self.debug_windows.cheat.rom_metadata_rom_name =
                libretro_meta.as_ref().map(|m| m.rom_name.clone());
            self.debug_windows.cheat.libretro_platform = platform;
            self.debug_windows.cheat.libretro_search_hints = search_hints;
            self.debug_windows.cheat.libretro_search = self
                .debug_windows
                .cheat
                .libretro_search_hints
                .first()
                .cloned()
                .unwrap_or_else(|| rom_title.clone());

            let (user, libretro) =
                crate::cheats::load_game_cheats(system, Some(&rom_title), rom_crc32);
            self.debug_windows.cheat.user_codes = user;
            self.debug_windows.cheat.libretro_codes = libretro;
        } else {
            self.debug_windows.cheat.rom_title = None;
            self.debug_windows.cheat.rom_crc32 = None;
            self.debug_windows.cheat.rom_metadata_title = None;
            self.debug_windows.cheat.rom_metadata_rom_name = None;
            self.debug_windows.cheat.libretro_search_hints.clear();
            self.debug_windows.cheat.libretro_search.clear();
            self.debug_windows.cheat.user_codes.clear();
            self.debug_windows.cheat.libretro_codes.clear();
        }

        self.debug_windows.cheat.libretro_results.clear();
        self.debug_windows.cheat.libretro_file_list = None;
        self.debug_windows.cheat.libretro_status = None;
        self.debug_windows.cheat.cheats_dirty = true;
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn setup_mods_for_rom(&mut self, system: ActiveSystem, original_crc: u32) {
        let dir = crate::mods::mods_dir_for_rom(system, original_crc);
        let entries = crate::mods::load_mod_config(&dir);
        self.debug_windows.mod_state.entries = entries;
        self.debug_windows.mod_state.mods_dir = Some(dir);
        self.debug_windows.mod_state.needs_reload = false;
        self.debug_windows.mod_state.status_message = None;
    }

    fn load_rom_with_options(&mut self, path: &Path, auto_load_state: bool) {
        self.stop_emu_thread();
        self.stop_camera_capture();

        self.frames_in_flight = 0;
        self.cached_ui_data = None;
        self.recycled.clear();
        self.debug_windows.last_disasm_pc = None;

        let (rom_path, preloaded_data, system) = match detect_and_extract_rom(path) {
            Ok(result) => result,
            Err(e) => {
                let msg = format!("{e:#}");
                log::warn!("{msg}");
                self.toast_manager.error(msg);
                return;
            }
        };

        let (backend, original_crc) =
            match self.init_backend(system, path, &rom_path, preloaded_data) {
                Ok(result) => result,
                Err(e) => {
                    log::error!("Failed to load ROM '{}': {}", path.display(), e);
                    self.toast_manager.error(format!("Failed to load ROM: {e}"));
                    return;
                }
            };

        let rom_name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("ROM")
            .to_string();
        log::info!("Loaded ROM: {}", path.display());

        self.rom_info.is_mbc7 = backend.is_mbc7();
        self.rom_info.is_pocket_camera = backend.is_pocket_camera();
        self.rom_info.rom_path = Some(backend.rom_path().to_path_buf());
        self.rom_info.rom_hash = Some(backend.rom_hash());
        self.active_system = system;

        let (native_w, native_h) = system.screen_size();
        if let Some(gfx) = self.gfx.as_mut() {
            gfx.set_native_size(native_w, native_h);
        }

        #[cfg(not(target_arch = "wasm32"))]
        self.setup_cheats_for_rom(system, path, &backend);
        #[cfg(not(target_arch = "wasm32"))]
        self.setup_mods_for_rom(system, original_crc);

        self.emu_thread = Some(EmuThread::spawn(backend));
        self.fps_tracker = FpsTracker::new();
        self.timing.last_frame_time = Instant::now();

        if self.timing.uncapped_speed
            && let Some(thread) = &self.emu_thread
        {
            thread.send(EmuCommand::SetUncapped(true));
        }

        self.settings.add_recent_rom(path);
        self.settings.save();
        self.toast_manager.info(format!("Loaded {rom_name}"));

        if auto_load_state && self.settings.emulation.auto_save_state {
            if let Some(thread) = &self.emu_thread {
                thread.send(EmuCommand::AutoLoadState {
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
                    log::info!("Auto-loaded state from {}", p);
                    self.toast_manager.success("Resumed from auto-save");
                }
                Some(EmuResponse::LoadStateFailed(_)) => {}
                _ => {}
            }
        }
    }

    pub(in crate::app) fn load_rom(&mut self, path: &Path) {
        self.load_rom_with_options(path, true);
    }

    pub(in crate::app) fn reset_game(&mut self) {
        let Some(path) = self.rom_info.rom_path.clone() else {
            self.toast_manager.info("No ROM loaded");
            return;
        };

        self.load_rom_with_options(&path, false);
        self.toast_manager.success("Game reset");
    }

    pub(in crate::app) fn stop_game(&mut self) {
        if self.rom_info.rom_path.is_none() && self.emu_thread.is_none() {
            self.toast_manager.info("No ROM loaded");
            return;
        }

        if let Some(ref title) = self.debug_windows.cheat.rom_title {
            crate::cheats::save_game_cheats(
                self.active_system,
                Some(title),
                self.debug_windows.cheat.rom_crc32,
                &self.debug_windows.cheat.user_codes,
                &self.debug_windows.cheat.libretro_codes,
            );
        }

        self.stop_emu_thread();
        self.stop_camera_capture();

        if let Some(gfx) = self.gfx.as_ref() {
            gfx.clear_framebuffer();
        }

        self.frames_in_flight = 0;
        self.cached_ui_data = None;
        self.recycled.clear();
        self.latest_frame = None;
        self.last_displayed_frame = None;
        self.rom_info.rom_path = None;
        self.rom_info.rom_hash = None;
        self.rom_info.is_mbc7 = false;
        self.rom_info.is_pocket_camera = false;
        self.speed.paused = false;
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

        self.debug_windows.mod_state.clear();

        self.toast_manager.set_paused(false);
        self.toast_manager.success("Stopped emulation");
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(in crate::app) fn open_file_dialog(&mut self) {
        let was_paused = self.pause_for_dialog();
        let file = rfd::FileDialog::new()
            .add_filter("ROMs", &["gb", "gbc", "nes", "zip"])
            .add_filter("Game Boy ROMs", &["gb", "gbc"])
            .add_filter("NES ROMs", &["nes"])
            .add_filter("ZIP Archives", &["zip"])
            .add_filter("All files", &["*"])
            .set_title("Open ROM")
            .pick_file();

        self.resume_after_dialog(was_paused);
        if let Some(path) = file {
            self.load_rom(&path);
        }
    }

    #[cfg(target_arch = "wasm32")]
    pub(in crate::app) fn open_file_dialog(&mut self) {
        self.toast_manager.info("Drop a ROM file onto the window to load it");
    }

    pub(in crate::app) fn handle_dropped_file(&mut self, path: PathBuf) {
        self.load_rom(&path);
    }
}
