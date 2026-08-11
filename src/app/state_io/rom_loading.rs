use super::App;
use crate::emu_backend::{
    ActiveSystem, BackendLoadConfig, EmuBackend, ROM_AND_ARCHIVE_EXTENSIONS, archive_extensions,
    load_backend_from_rom_source, system_specs,
};
use crate::emu_thread::{EmuCommand, EmuResponse};
use anyhow::Context;
use std::path::{Path, PathBuf};
use zeff_ws_core::hardware::cartridge::RomOrientation;

pub(crate) fn detect_and_extract_rom(
    path: &Path,
) -> anyhow::Result<(PathBuf, Option<Vec<u8>>, ActiveSystem)> {
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
    pub(super) fn init_backend(
        &self,
        system: ActiveSystem,
        path: &Path,
        rom_path: &Path,
        preloaded_data: Option<Vec<u8>>,
    ) -> anyhow::Result<(EmuBackend, u32)> {
        let loaded = load_backend_from_rom_source(
            system,
            path,
            rom_path,
            preloaded_data,
            BackendLoadConfig {
                gb_hardware_mode_preference: self.settings.emulation.hardware_mode_preference,
                sample_rate: self.audio.as_ref().map(|audio| audio.sample_rate()),
                apply_mods: true,
                initial_input: Some(self.host_joypad_input_for_system(system)),
                sega8_video_standard: self
                    .settings
                    .emulation
                    .sega8_video_standard
                    .forced_standard(),
                sega8_console_region: self.settings.emulation.sega8_console_region.forced_region(),
            },
        )?;
        Ok((loaded.backend, loaded.original_crc32))
    }

    fn load_rom_with_options(&mut self, path: &Path, auto_load_state: bool) {
        self.stop_emu_thread();
        self.stop_camera_capture();

        self.frames_in_flight = 0;
        self.cached_ui_data = None;
        self.recycled.clear();
        self.latest_frame = None;
        self.last_core_frame = None;
        self.last_displayed_frame = None;
        self.debug_windows.last_disasm_pc = None;
        self.undo_load_state = None;

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

        self.finalize_rom_load(
            &backend,
            system,
            backend.rom_path().to_path_buf(),
            backend.source_path().to_path_buf(),
        );

        self.setup_cheats_for_rom(system, path, &backend);
        self.setup_mods_for_rom(system, original_crc);

        self.spawn_emu_thread(backend);

        self.settings.add_recent_rom(path);
        self.settings.save();
        self.toast_manager.info(format!("Loaded {rom_name}"));

        if auto_load_state && self.settings.emulation.auto_save_state {
            if let Some(thread) = &self.emu_thread {
                let (buttons_pressed, dpad_pressed) = self.current_host_joypad_input();
                thread.send(EmuCommand::AutoLoadState {
                    buttons_pressed,
                    dpad_pressed,
                });
            }
            match self.recv_cold_response() {
                Some(EmuResponse::LoadStateOk { path: p }) => {
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
        self.refresh_slot_info();
    }

    pub(in crate::app) fn load_rom(&mut self, path: &Path) {
        self.load_rom_with_options(path, true);
    }

    pub(in crate::app) fn reset_game(&mut self) {
        let Some(path) = self.rom_info.source_path.clone() else {
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

        self.save_current_cheats();

        self.stop_emu_thread();
        self.stop_camera_capture();

        if let Some(gfx) = self.gfx.as_ref() {
            gfx.clear_framebuffer();
        }

        self.frames_in_flight = 0;
        self.cached_ui_data = None;
        self.recycled.clear();
        self.latest_frame = None;
        self.last_core_frame = None;
        self.last_displayed_frame = None;
        self.undo_load_state = None;
        self.rom_info.rom_path = None;
        self.rom_info.source_path = None;
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

        self.ws_display_rotated = false;
        self.toast_manager.set_paused(false);
        self.toast_manager.success("Stopped emulation");
        self.refresh_slot_info();
    }

    pub(in crate::app) fn open_file_dialog(&mut self) {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let was_paused = self.pause_for_dialog();
            let mut dialog =
                crate::platform::FileDialog::new().add_filter("ROMs", ROM_AND_ARCHIVE_EXTENSIONS);
            for spec in system_specs() {
                dialog = dialog.add_filter(spec.file_dialog_filter_name, spec.rom_extensions);
            }
            let file = dialog
                .add_filter("ZIP Archives", archive_extensions())
                .add_filter("All files", &["*"])
                .set_title("Open ROM")
                .pick_file();

            self.resume_after_dialog(was_paused);
            if let Some(path) = file {
                self.load_rom(&path);
            }
        }
        #[cfg(target_arch = "wasm32")]
        {
            crate::platform::FileDialog::new()
                .add_filter("ROMs", ROM_AND_ARCHIVE_EXTENSIONS)
                .set_title("Open ROM")
                .pick_file_web(self.pending_rom_load.clone());
        }
    }

    pub(in crate::app) fn handle_dropped_file(&mut self, path: PathBuf) {
        self.load_rom(&path);
    }

    pub(in crate::app) fn finalize_rom_load(
        &mut self,
        backend: &EmuBackend,
        system: ActiveSystem,
        rom_path_buf: PathBuf,
        source_path_buf: PathBuf,
    ) {
        self.rom_info.is_mbc7 = backend.is_mbc7();
        self.rom_info.is_pocket_camera = backend.is_pocket_camera();
        self.rom_info.rom_path = Some(rom_path_buf);
        self.rom_info.source_path = Some(source_path_buf);
        self.rom_info.rom_hash = Some(backend.rom_hash());
        self.active_system = system;
        self.ws_display_rotated = backend
            .ws()
            .is_some_and(|ws| ws.preferred_orientation() == RomOrientation::Vertical);
        if system == ActiveSystem::WonderSwan {
            log::info!(
                "WonderSwan display orientation: {}",
                if self.ws_display_rotated {
                    "vertical"
                } else {
                    "horizontal"
                }
            );
        }
        self.debug_windows.memory.configure_for_system(system);

        let (native_w, native_h) = self.active_display_size();
        if let Some(gfx) = self.gfx.as_mut() {
            gfx.set_native_size(native_w, native_h);
        }
    }
}
