use super::App;
use crate::emu_backend::{
    ActiveSystem, BackendLoadConfig, EmuBackend, ROM_AND_ARCHIVE_EXTENSIONS, archive_extensions,
    load_backend_from_rom_source, system_specs,
};
use crate::emu_thread::{EmuCommand, EmuResponse};
use crate::rom_archive::PendingArchiveSelection;
use anyhow::Context;
use std::path::{Path, PathBuf};
use zeff_emu_common::time::MachineTiming;
use zeff_ws_core::hardware::cartridge::RomOrientation;

fn is_zip_path(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("zip"))
}

pub(crate) fn detect_and_extract_rom(
    path: &Path,
) -> anyhow::Result<(PathBuf, Option<Vec<u8>>, ActiveSystem)> {
    let (rom_path, preloaded_data) = if is_zip_path(path) {
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

fn detect_and_extract_archive_entry(
    archive_path: &Path,
    entry_index: usize,
) -> anyhow::Result<(PathBuf, Option<Vec<u8>>, ActiveSystem)> {
    let (rom_path, data) = super::extract_rom_entry_from_zip(archive_path, entry_index)
        .with_context(|| format!("Failed to extract ROM from '{}'", archive_path.display()))?;
    detect_system_for_loaded_path(rom_path, Some(data))
}

fn detect_and_extract_archive_entry_path(
    archive_path: &Path,
    virtual_rom_path: &Path,
) -> anyhow::Result<(PathBuf, Option<Vec<u8>>, ActiveSystem)> {
    let (rom_path, data) =
        super::extract_rom_entry_path_from_zip(archive_path, virtual_rom_path)
            .with_context(|| format!("Failed to extract ROM from '{}'", archive_path.display()))?;
    detect_system_for_loaded_path(rom_path, Some(data))
}

fn detect_system_for_loaded_path(
    rom_path: PathBuf,
    preloaded_data: Option<Vec<u8>>,
) -> anyhow::Result<(PathBuf, Option<Vec<u8>>, ActiveSystem)> {
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
        #[cfg(not(target_arch = "wasm32"))]
        let configured_firmware_dir = self.settings.emulation.firmware_directory_path();
        #[cfg(not(target_arch = "wasm32"))]
        let firmware_inventory = if !self.debug_windows.firmware_inventory.needs_refresh
            && self.debug_windows.firmware_inventory.directory.as_deref()
                == configured_firmware_dir.as_deref()
        {
            self.debug_windows.firmware_inventory.inventory.clone()
        } else {
            None
        };
        #[cfg(target_arch = "wasm32")]
        let firmware_inventory = Some(crate::platform::firmware_inventory_snapshot());
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
                firmware_search_dirs: self.settings.emulation.firmware_search_dirs(),
                firmware_inventory,
                gb_use_external_boot_rom: matches!(
                    self.settings.emulation.gb_boot_rom_mode,
                    crate::settings::GbBootRomMode::External
                ),
                gba_use_external_bios: matches!(
                    self.settings.emulation.gba_bios_mode,
                    crate::settings::GbaBiosMode::External
                ),
                sega8_use_external_boot_rom: matches!(
                    self.settings.emulation.sega_boot_rom_mode,
                    crate::settings::SegaBootRomMode::External
                ),
                #[cfg(test)]
                fds_bios_override: None,
            },
        )?;
        Ok((loaded.backend, loaded.original_crc32))
    }

    fn load_prepared_rom_with_options(
        &mut self,
        source_path: &Path,
        rom_path: PathBuf,
        preloaded_data: Option<Vec<u8>>,
        system: ActiveSystem,
        auto_load_state: bool,
    ) {
        let is_same_rom_reset = !auto_load_state
            && self.rom_info.source_path.as_deref() == Some(source_path)
            && self.rom_info.rom_path.as_deref() == Some(rom_path.as_path());
        let previous_audio_context = self
            .emu_thread
            .as_ref()
            .and_then(crate::emu_thread::EmuThread::audio_recording_context);
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.tcp_link_active = false;
        }
        self.stop_emu_thread();
        self.stop_camera_capture();
        self.media_slot_snapshot = None;
        self.recording.pending_media_commands.clear();

        self.frames_in_flight = 0;
        self.cached_ui_data = None;
        self.debug_windows.trace.clear();
        self.debug_windows.execution_coverage.clear();
        self.recycled.clear();
        self.latest_frame = None;
        self.last_core_frame = None;
        self.last_displayed_frame = None;
        self.debug_windows.last_disasm_pc = None;
        self.debug_windows.last_disasm_mapping = None;
        self.debug_windows.disasm_target = None;
        self.undo_load_state = None;

        let (backend, original_crc) =
            match self.init_backend(system, source_path, &rom_path, preloaded_data) {
                Ok(result) => result,
                Err(e) => {
                    self.stop_audio_recording();
                    log::error!("Failed to load ROM '{}': {}", source_path.display(), e);
                    self.toast_manager.error(format!("Failed to load ROM: {e}"));
                    return;
                }
            };

        if self.recording.audio_recorder.is_some() {
            let next_audio_context = backend.audio_topology().map(|topology| {
                crate::audio_tooling::AudioRecordingContext {
                    system: backend.system(),
                    topology,
                    clock_rate: backend.timing_snapshot().rate(),
                }
            });
            if is_same_rom_reset && previous_audio_context == next_audio_context {
                if let Some(recorder) = &mut self.recording.audio_recorder {
                    recorder.begin_semantic_timeline_epoch(
                        crate::audio_recorder::AudioTimelineDiscontinuity::Reset,
                    );
                }
            } else {
                self.stop_audio_recording();
            }
        }

        let rom_name = rom_path
            .file_name()
            .or_else(|| source_path.file_name())
            .and_then(|n| n.to_str())
            .unwrap_or("ROM")
            .to_string();
        log::info!("Loaded ROM: {}", rom_path.display());

        self.finalize_rom_load(
            &backend,
            system,
            backend.rom_path().to_path_buf(),
            backend.source_path().to_path_buf(),
        );

        self.setup_cheats_for_rom(system, &rom_path, &backend);
        self.setup_mods_for_rom(system, original_crc);

        self.spawn_emu_thread(backend);

        self.settings.add_recent_rom(source_path);
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
                Some(EmuResponse::LoadStateOk {
                    path: p,
                    media_slot_snapshot,
                    game_boy_serial_device,
                }) => {
                    self.media_slot_snapshot = media_slot_snapshot;
                    if let Some(device) = game_boy_serial_device {
                        self.game_boy_serial_device = device;
                    }
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

    fn load_rom_with_options(&mut self, path: &Path, auto_load_state: bool) {
        let (rom_path, preloaded_data, system) = match detect_and_extract_rom(path) {
            Ok(result) => result,
            Err(e) => {
                let msg = format!("{e:#}");
                log::warn!("{msg}");
                self.toast_manager.error(msg);
                return;
            }
        };

        self.load_prepared_rom_with_options(
            path,
            rom_path,
            preloaded_data,
            system,
            auto_load_state,
        );
    }

    pub(in crate::app) fn load_archive_entry_with_options(
        &mut self,
        archive_path: &Path,
        entry_index: usize,
        auto_load_state: bool,
    ) {
        let (rom_path, preloaded_data, system) =
            match detect_and_extract_archive_entry(archive_path, entry_index) {
                Ok(result) => result,
                Err(e) => {
                    let msg = format!("{e:#}");
                    log::warn!("{msg}");
                    self.toast_manager.error(msg);
                    return;
                }
            };

        self.load_prepared_rom_with_options(
            archive_path,
            rom_path,
            preloaded_data,
            system,
            auto_load_state,
        );
    }

    fn load_archive_entry_path_with_options(
        &mut self,
        archive_path: &Path,
        virtual_rom_path: &Path,
        auto_load_state: bool,
    ) {
        let (rom_path, preloaded_data, system) =
            match detect_and_extract_archive_entry_path(archive_path, virtual_rom_path) {
                Ok(result) => result,
                Err(e) => {
                    let msg = format!("{e:#}");
                    log::warn!("{msg}");
                    self.toast_manager.error(msg);
                    return;
                }
            };

        self.load_prepared_rom_with_options(
            archive_path,
            rom_path,
            preloaded_data,
            system,
            auto_load_state,
        );
    }

    pub(in crate::app) fn load_rom(&mut self, path: &Path) {
        if self.begin_archive_selection_if_needed(path) {
            return;
        }
        self.load_rom_with_options(path, true);
    }

    pub(in crate::app) fn reset_game(&mut self) {
        let Some(path) = self.rom_info.source_path.clone() else {
            self.toast_manager.info("No ROM loaded");
            return;
        };

        if is_zip_path(&path)
            && let Some(rom_path) = self.rom_info.rom_path.clone()
            && rom_path != path
        {
            self.load_archive_entry_path_with_options(&path, &rom_path, false);
        } else {
            self.load_rom_with_options(&path, false);
        }
        self.toast_manager.success("Game reset");
    }

    pub(in crate::app) fn stop_game(&mut self) {
        if self.rom_info.rom_path.is_none() && self.emu_thread.is_none() {
            self.toast_manager.info("No ROM loaded");
            return;
        }

        self.save_current_cheats();

        self.stop_emu_thread();
        self.stop_audio_recording();
        self.stop_camera_capture();
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.tcp_link_active = false;
        }

        if let Some(gfx) = self.gfx.as_ref() {
            gfx.clear_framebuffer();
        }

        self.frames_in_flight = 0;
        self.cached_ui_data = None;
        self.debug_windows.trace.clear();
        self.debug_windows.execution_coverage.clear();
        self.recycled.clear();
        self.latest_frame = None;
        self.last_core_frame = None;
        self.last_displayed_frame = None;
        self.undo_load_state = None;
        self.media_slot_snapshot = None;
        self.recording.pending_media_commands.clear();
        self.rom_info.rom_path = None;
        self.rom_info.source_path = None;
        self.rom_info.rom_hash = None;
        self.rom_info.replay_metadata = None;
        self.symbols = crate::symbols::SymbolSession::default();
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.pending_symbol_load = None;
        }
        self.rom_info.is_mbc7 = false;
        self.rom_info.is_pocket_camera = false;
        self.debug_windows.last_disasm_pc = None;
        self.debug_windows.last_disasm_mapping = None;
        self.debug_windows.disasm_target = None;
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
        self.pending_archive_selection = None;

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

    fn begin_archive_selection_if_needed(&mut self, path: &Path) -> bool {
        if !is_zip_path(path) {
            return false;
        }
        let entries = match super::list_rom_entries_in_zip(path) {
            Ok(entries) => entries,
            Err(_) => return false,
        };
        if entries.len() <= 1 {
            return false;
        }
        self.pending_archive_selection = Some(PendingArchiveSelection {
            archive_path: path.to_path_buf(),
            entries,
        });
        self.toast_manager
            .info("Archive contains multiple ROMs; choose one to load");
        true
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
        self.rom_info.replay_metadata = Some(backend.replay_metadata());
        self.media_slot_snapshot = backend.media_slot_snapshot();
        #[cfg(not(target_arch = "wasm32"))]
        self.start_symbol_load(backend);
        #[cfg(target_arch = "wasm32")]
        {
            self.symbols = crate::symbols::SymbolSession::load_for_backend(backend);
        }
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

    #[cfg(not(target_arch = "wasm32"))]
    pub(in crate::app) fn start_symbol_load(&mut self, backend: &EmuBackend) {
        self.start_symbol_load_for_paths(
            backend.system(),
            backend.rom_path().to_path_buf(),
            backend.source_path().to_path_buf(),
            backend.rom_hash(),
        );
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(in crate::app) fn start_symbol_load_for_paths(
        &mut self,
        system: ActiveSystem,
        rom_path: PathBuf,
        source_path: PathBuf,
        rom_hash: [u8; 32],
    ) {
        self.start_symbol_load_request(system, rom_path, source_path, rom_hash, None);
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn start_symbol_load_request(
        &mut self,
        system: ActiveSystem,
        rom_path: PathBuf,
        source_path: PathBuf,
        rom_hash: [u8; 32],
        sidecar_path: Option<PathBuf>,
    ) {
        self.pending_symbol_load = None;
        self.symbols = crate::symbols::SymbolSession::loading();
        let request_id = self.next_symbol_load_id;
        self.next_symbol_load_id = self.next_symbol_load_id.wrapping_add(1);
        let (sender, receiver) = std::sync::mpsc::channel();
        let started = std::time::Instant::now();
        let worker = std::thread::Builder::new()
            .name("zeff-symbol-load".to_owned())
            .spawn(move || {
                let session = sidecar_path.map_or_else(
                    || {
                        crate::symbols::SymbolSession::load_for_paths(
                            system,
                            &rom_path,
                            &source_path,
                            rom_hash,
                        )
                    },
                    |path| {
                        crate::symbols::SymbolSession::load_for_paths_with_sidecar(
                            system,
                            &rom_path,
                            &source_path,
                            rom_hash,
                            &path,
                        )
                    },
                );
                let _ = sender.send(super::super::SymbolLoadResult {
                    request_id,
                    elapsed: started.elapsed(),
                    session,
                });
            });
        match worker {
            Ok(_) => {
                self.pending_symbol_load = Some(super::super::PendingSymbolLoad {
                    request_id,
                    receiver,
                });
                self.toast_manager.info("Loading symbols...");
            }
            Err(error) => {
                self.symbols = crate::symbols::SymbolSession::default();
                self.toast_manager
                    .error(format!("Couldn't start symbol loader: {error}"));
            }
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(in crate::app) fn open_symbol_file_dialog(&mut self) {
        let (Some(rom_path), Some(source_path), Some(rom_hash)) = (
            self.rom_info.rom_path.clone(),
            self.rom_info.source_path.clone(),
            self.rom_info.rom_hash,
        ) else {
            self.toast_manager.error("Load a ROM first");
            return;
        };
        let was_paused = self.pause_for_dialog();
        let path = crate::platform::FileDialog::new()
            .add_filter(
                "Symbol files",
                &["elf", "axf", "sym", "map", "dbg", "nl", "json"],
            )
            .add_filter("All files", &["*"])
            .set_title("Load Symbol File")
            .pick_file();
        self.resume_after_dialog(was_paused);
        if let Some(path) = path {
            self.start_symbol_load_request(
                self.active_system,
                rom_path,
                source_path,
                rom_hash,
                Some(path),
            );
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(in crate::app) fn poll_symbol_load(&mut self) {
        let Some(pending) = self.pending_symbol_load.as_ref() else {
            return;
        };
        let result = match pending.receiver.try_recv() {
            Ok(result) => result,
            Err(std::sync::mpsc::TryRecvError::Empty) => return,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.pending_symbol_load = None;
                self.symbols = crate::symbols::SymbolSession::default();
                self.toast_manager
                    .error("Symbol loader stopped unexpectedly");
                return;
            }
        };
        if result.request_id != pending.request_id {
            return;
        }
        self.pending_symbol_load = None;
        let imported_symbol_count = result
            .session
            .modules
            .iter()
            .filter(|module| !module.is_builtin())
            .map(|module| module.symbol_count)
            .sum::<usize>();
        self.symbols = result.session;
        if imported_symbol_count > 0 {
            self.toast_manager.info(format!(
                "Loaded {imported_symbol_count} symbols in {} ms",
                result.elapsed.as_millis()
            ));
        } else if let Some(diagnostic) = self.symbols.diagnostics.first() {
            self.toast_manager
                .info(format!("Symbol load skipped: {diagnostic}"));
        }
    }
}
