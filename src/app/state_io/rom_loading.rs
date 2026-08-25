use super::App;
use crate::emu_backend::{
    ActiveSystem, BackendLoadConfig, EmuBackend, ROM_AND_ARCHIVE_EXTENSIONS, archive_extensions,
    load_backend_from_rom_source, system_specs,
};
use crate::emu_thread::{EmuCommand, EmuResponse};
use crate::rom_archive::PendingArchiveSelection;
use anyhow::Context;
use std::path::{Path, PathBuf};
#[cfg(not(target_arch = "wasm32"))]
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use zeff_emu_common::time::MachineTiming;
use zeff_ws_core::hardware::cartridge::RomOrientation;

fn is_zip_path(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("zip"))
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn is_native_seven_zip_path(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("7z"))
}

#[cfg(not(target_arch = "wasm32"))]
enum RomPreparationPoll {
    Pending,
    Complete(super::super::RomPreparationOutcome),
    Disconnected,
}

#[cfg(not(target_arch = "wasm32"))]
fn cancel_rom_preparation_slot(slot: &mut Option<super::super::PendingRomPreparation>) -> bool {
    let Some(pending) = slot.take() else {
        return false;
    };
    pending.cancel.store(true, Ordering::Release);
    true
}

#[cfg(not(target_arch = "wasm32"))]
fn poll_rom_preparation_slot(
    slot: &mut Option<super::super::PendingRomPreparation>,
) -> RomPreparationPoll {
    loop {
        let Some(pending) = slot.as_ref() else {
            return RomPreparationPoll::Pending;
        };
        match pending.receiver.try_recv() {
            Ok(result) if result.request_id == pending.request_id => {
                slot.take();
                return RomPreparationPoll::Complete(result.outcome);
            }
            Ok(_) => {}
            Err(std::sync::mpsc::TryRecvError::Empty) => {
                return RomPreparationPoll::Pending;
            }
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                slot.take();
                return RomPreparationPoll::Disconnected;
            }
        }
    }
}

fn automatic_symbol_loading_available(backend: &EmuBackend) -> bool {
    backend.supports_symbol_loading()
}

struct PreparedRomLoad {
    source_path: PathBuf,
    rom_path: PathBuf,
    system: ActiveSystem,
    auto_load_state: bool,
    backend: EmuBackend,
    original_crc: u32,
}

fn dismiss_archive_selection_for_new_load(slot: &mut Option<PendingArchiveSelection>) {
    *slot = None;
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
    fn backend_load_config(&self, system: ActiveSystem) -> BackendLoadConfig {
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
            pce_console_wiring: self.settings.emulation.pce_console_wiring.forced_wiring(),
            pce_hucard_board: None,
            pce_arcade_card_mode: self.settings.emulation.pce_arcade_card.core_mode(),
            pce_cd_archive_memory_limit_mib: self
                .settings
                .emulation
                .pce_cd_archive_memory_limit
                .mib(),
            pce_load_battery_bram: true,
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
            #[cfg(test)]
            pce_cd_system_card_override: None,
            #[cfg(test)]
            pce_cd_system_card_sha256_override: None,
        }
    }

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
            self.backend_load_config(system),
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
        let (backend, original_crc) =
            match self.init_backend(system, source_path, &rom_path, preloaded_data) {
                Ok(result) => result,
                Err(e) => {
                    log::error!("Failed to load ROM '{}': {}", source_path.display(), e);
                    self.toast_manager.error(format!("Failed to load ROM: {e}"));
                    return;
                }
            };
        self.commit_prepared_rom(PreparedRomLoad {
            source_path: source_path.to_path_buf(),
            rom_path,
            system,
            auto_load_state,
            backend,
            original_crc,
        });
    }

    fn commit_prepared_rom(&mut self, prepared: PreparedRomLoad) {
        #[cfg(not(target_arch = "wasm32"))]
        self.release_pce_mouse(false);
        let PreparedRomLoad {
            source_path,
            rom_path,
            system,
            auto_load_state,
            backend,
            original_crc,
        } = prepared;
        let is_same_rom_reset = !auto_load_state
            && self.rom_info.source_path.as_deref() == Some(source_path.as_path())
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
        if let Some(audio) = &mut self.audio {
            audio.discard_queued_samples();
        }
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

        self.settings.add_recent_rom(&source_path);
        self.settings.save();
        if is_same_rom_reset {
            self.toast_manager.success("Game reset");
        } else {
            self.toast_manager.info(format!("Loaded {rom_name}"));
        }

        if auto_load_state
            && self.settings.emulation.auto_save_state
            && self.core_supports_save_states()
        {
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

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn cancel_pending_rom_preparation(&mut self, notify: bool) {
        if cancel_rom_preparation_slot(&mut self.pending_rom_preparation) && notify {
            self.toast_manager.info("Archive load canceled");
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn begin_seven_zip_preparation(
        &mut self,
        source_path: &Path,
        selected_entry_index: Option<usize>,
        expected_rom_path: Option<PathBuf>,
        auto_load_state: bool,
    ) {
        self.cancel_pending_rom_preparation(false);
        let request_id = self.next_rom_preparation_id;
        self.next_rom_preparation_id = self.next_rom_preparation_id.wrapping_add(1);
        let cancel = Arc::new(AtomicBool::new(false));
        let progress =
            Arc::new(crate::emu_backend::pce_cd_archive::PceCdPackageProgress::default());
        let mut config = self.backend_load_config(ActiveSystem::Pce);
        config.initial_input = None;
        let source_path = source_path.to_path_buf();
        let worker_source_path = source_path.clone();
        let worker_cancel = Arc::clone(&cancel);
        let worker_progress = Arc::clone(&progress);
        let (sender, receiver) = std::sync::mpsc::channel();
        let worker = std::thread::Builder::new()
            .name("zeff-seven-zip-load".to_owned())
            .spawn(move || {
                let result = crate::emu_backend::loader::prepare_seven_zip_backend(
                    &worker_source_path,
                    selected_entry_index,
                    expected_rom_path.as_deref(),
                    &config,
                    &worker_cancel,
                    &worker_progress,
                );
                let outcome = if worker_cancel.load(Ordering::Acquire) {
                    super::super::RomPreparationOutcome::Cancelled
                } else {
                    match result {
                        Ok(crate::emu_backend::loader::PreparedSevenZipBackend::Ready {
                            rom_path,
                            system,
                            loaded,
                        }) => super::super::RomPreparationOutcome::Ready {
                            source_path: worker_source_path,
                            rom_path,
                            system,
                            auto_load_state,
                            loaded,
                        },
                        Ok(crate::emu_backend::loader::PreparedSevenZipBackend::Selection(
                            entries,
                        )) => super::super::RomPreparationOutcome::ArchiveSelection {
                            source_path: worker_source_path,
                            entries,
                        },
                        Err(error) => {
                            super::super::RomPreparationOutcome::Failed(format!("{error:#}"))
                        }
                    }
                };
                let _ = sender.send(super::super::RomPreparationResult {
                    request_id,
                    outcome,
                });
            });
        match worker {
            Ok(_) => {
                self.pending_rom_preparation = Some(super::super::PendingRomPreparation {
                    request_id,
                    source_path,
                    started_at: std::time::Instant::now(),
                    cancel,
                    progress,
                    receiver,
                });
            }
            Err(error) => {
                self.toast_manager
                    .error(format!("Couldn't start archive loader: {error}"));
            }
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(in crate::app) fn poll_rom_preparation(&mut self) {
        let outcome = match poll_rom_preparation_slot(&mut self.pending_rom_preparation) {
            RomPreparationPoll::Pending => return,
            RomPreparationPoll::Complete(outcome) => outcome,
            RomPreparationPoll::Disconnected => {
                self.toast_manager
                    .error("Archive loader stopped unexpectedly");
                return;
            }
        };
        match outcome {
            super::super::RomPreparationOutcome::Ready {
                source_path,
                rom_path,
                system,
                auto_load_state,
                mut loaded,
            } => {
                let sample_rate = self
                    .audio
                    .as_ref()
                    .map_or(crate::audio::DEFAULT_AUDIO_SAMPLE_RATE, |audio| {
                        audio.sample_rate()
                    });
                loaded.backend.set_sample_rate(sample_rate);
                let (buttons, dpad) = self.host_joypad_input_for_system(system);
                loaded.backend.set_input(buttons, dpad);
                self.commit_prepared_rom(PreparedRomLoad {
                    source_path,
                    rom_path,
                    system,
                    auto_load_state,
                    backend: loaded.backend,
                    original_crc: loaded.original_crc32,
                });
            }
            super::super::RomPreparationOutcome::ArchiveSelection {
                source_path,
                entries,
            } => {
                self.pending_archive_selection = Some(PendingArchiveSelection {
                    archive_path: source_path,
                    entries,
                });
                self.toast_manager
                    .info("Archive contains multiple ROMs; choose one to load");
            }
            super::super::RomPreparationOutcome::Failed(error) => {
                log::error!("Failed to load archive: {error}");
                self.toast_manager
                    .error(format!("Failed to load archive: {error}"));
            }
            super::super::RomPreparationOutcome::Cancelled => {}
        }
    }

    fn load_rom_with_options(&mut self, path: &Path, auto_load_state: bool) {
        #[cfg(not(target_arch = "wasm32"))]
        if is_native_seven_zip_path(path) {
            self.begin_seven_zip_preparation(path, None, None, auto_load_state);
            return;
        }
        #[cfg(not(target_arch = "wasm32"))]
        self.cancel_pending_rom_preparation(false);
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
        #[cfg(not(target_arch = "wasm32"))]
        if is_native_seven_zip_path(archive_path) {
            self.begin_seven_zip_preparation(
                archive_path,
                Some(entry_index),
                None,
                auto_load_state,
            );
            return;
        }
        #[cfg(not(target_arch = "wasm32"))]
        self.cancel_pending_rom_preparation(false);
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
        #[cfg(not(target_arch = "wasm32"))]
        if is_native_seven_zip_path(archive_path) {
            self.begin_seven_zip_preparation(
                archive_path,
                None,
                Some(virtual_rom_path.to_path_buf()),
                auto_load_state,
            );
            return;
        }
        #[cfg(not(target_arch = "wasm32"))]
        self.cancel_pending_rom_preparation(false);
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
        dismiss_archive_selection_for_new_load(&mut self.pending_archive_selection);
        #[cfg(not(target_arch = "wasm32"))]
        self.cancel_pending_rom_preparation(false);
        if self.begin_archive_selection_if_needed(path) {
            return;
        }
        self.load_rom_with_options(path, true);
    }

    pub(in crate::app) fn reset_game(&mut self) {
        if !self.debug_windows.mod_state.needs_reload
            && self.recording.replay_player.is_none()
            && self.recording.replay_recorder.is_none()
            && let Some(thread) = &self.emu_thread
        {
            thread.send(EmuCommand::Reset);
            if let Some(audio) = &mut self.audio {
                audio.discard_queued_samples();
            }
            self.frames_in_flight = 0;
            self.cached_ui_data = None;
            self.latest_frame = None;
            self.last_core_frame = None;
            self.last_displayed_frame = None;
            self.undo_load_state = None;
            self.rewind = super::super::RewindState {
                held: false,
                fill: 0.0,
                throttle: 0,
                pops: 0,
                pending: false,
                backstep_pending: false,
            };
            self.speed.paused = false;
            self.timing.last_frame_time = crate::platform::Instant::now();
            self.toast_manager.success("Game reset");
            return;
        }

        let Some(path) = self.rom_info.source_path.clone() else {
            self.toast_manager.info("No ROM loaded");
            return;
        };

        #[cfg(not(target_arch = "wasm32"))]
        if is_native_seven_zip_path(&path) {
            self.begin_seven_zip_preparation(&path, None, self.rom_info.rom_path.clone(), false);
            return;
        }

        if is_zip_path(&path)
            && let Some(rom_path) = self.rom_info.rom_path.clone()
            && rom_path != path
        {
            self.load_archive_entry_path_with_options(&path, &rom_path, false);
        } else {
            self.load_rom_with_options(&path, false);
        }
    }

    pub(in crate::app) fn stop_game(&mut self) {
        #[cfg(not(target_arch = "wasm32"))]
        let preparation_pending = self.pending_rom_preparation.is_some();
        #[cfg(target_arch = "wasm32")]
        let preparation_pending = false;
        if self.rom_info.rom_path.is_none() && self.emu_thread.is_none() && !preparation_pending {
            self.toast_manager.info("No ROM loaded");
            return;
        }

        #[cfg(not(target_arch = "wasm32"))]
        self.cancel_pending_rom_preparation(false);
        #[cfg(not(target_arch = "wasm32"))]
        self.release_pce_mouse(false);

        self.save_current_cheats();

        self.stop_emu_thread();
        if let Some(audio) = &mut self.audio {
            audio.discard_queued_samples();
        }
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
        self.rom_info.pce_controller_profile_hash = None;
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
                .add_filter("Archives", archive_extensions())
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
        self.rom_info.pce_controller_profile_hash = backend.pce_controller_profile_hash();
        self.rom_info.replay_metadata = Some(backend.replay_metadata());
        self.media_slot_snapshot = backend.media_slot_snapshot();
        #[cfg(not(target_arch = "wasm32"))]
        self.start_symbol_load(backend);
        #[cfg(target_arch = "wasm32")]
        {
            self.symbols = if automatic_symbol_loading_available(backend) {
                crate::symbols::SymbolSession::load_for_backend(backend)
            } else {
                crate::symbols::SymbolSession::default()
            };
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
            automatic_symbol_loading_available(backend),
        );
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(in crate::app) fn start_symbol_load_for_paths(
        &mut self,
        system: ActiveSystem,
        rom_path: PathBuf,
        source_path: PathBuf,
        rom_hash: [u8; 32],
        supports_symbol_loading: bool,
    ) {
        if !supports_symbol_loading {
            self.pending_symbol_load = None;
            self.symbols = crate::symbols::SymbolSession::default();
            return;
        }
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
        if !self.core_supports_debugger() {
            self.pending_symbol_load = None;
            self.symbols = crate::symbols::SymbolSession::default();
            return;
        }
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

#[cfg(test)]
mod tests {
    use super::{
        RomPreparationPoll, automatic_symbol_loading_available, cancel_rom_preparation_slot,
        dismiss_archive_selection_for_new_load, is_native_seven_zip_path,
        poll_rom_preparation_slot,
    };
    use crate::emu_backend::{EmuBackend, PceBackend};
    use std::path::PathBuf;
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    };

    fn pending_preparation(
        request_id: u64,
        cancel: Arc<AtomicBool>,
        receiver: mpsc::Receiver<super::super::super::RomPreparationResult>,
    ) -> super::super::super::PendingRomPreparation {
        super::super::super::PendingRomPreparation {
            request_id,
            source_path: PathBuf::from("disc.7z"),
            started_at: std::time::Instant::now(),
            cancel,
            progress: Arc::new(crate::emu_backend::pce_cd_archive::PceCdPackageProgress::default()),
            receiver,
        }
    }

    #[test]
    fn pce_disassembly_capability_enables_automatic_symbol_loading() {
        let mut rom = vec![0xEA; 0x2000];
        rom[..4].copy_from_slice(&[0xD4, 0xEA, 0x80, 0xFD]);
        rom[0x1FFE..].copy_from_slice(&0xE000_u16.to_le_bytes());
        let backend =
            EmuBackend::from_pce(PceBackend::new(rom, PathBuf::from("symbols.pce")).unwrap());

        assert!(automatic_symbol_loading_available(&backend));
    }

    #[test]
    fn native_seven_zip_route_is_separate_from_system_detection() {
        assert!(is_native_seven_zip_path(&PathBuf::from("games.7Z")));
        assert_eq!(
            crate::emu_backend::ActiveSystem::from_path(&PathBuf::from("disc.7z")),
            None
        );
    }

    #[test]
    fn newer_top_level_load_dismisses_an_open_zip_chooser() {
        let mut selection = Some(crate::rom_archive::PendingArchiveSelection {
            archive_path: PathBuf::from("older.zip"),
            entries: Vec::new(),
        });

        dismiss_archive_selection_for_new_load(&mut selection);

        assert!(selection.is_none());
    }

    #[test]
    fn replacing_or_stopping_preparation_cancels_and_drops_its_receiver() {
        let cancel = Arc::new(AtomicBool::new(false));
        let (sender, receiver) = mpsc::channel();
        let mut slot = Some(pending_preparation(3, Arc::clone(&cancel), receiver));

        assert!(cancel_rom_preparation_slot(&mut slot));
        assert!(cancel.load(Ordering::Acquire));
        assert!(slot.is_none());
        assert!(
            sender
                .send(super::super::super::RomPreparationResult {
                    request_id: 3,
                    outcome: super::super::super::RomPreparationOutcome::Cancelled,
                })
                .is_err()
        );
    }

    #[test]
    fn coordinator_drops_stale_results_and_delivers_current_once() {
        let cancel = Arc::new(AtomicBool::new(false));
        let (sender, receiver) = mpsc::channel();
        let mut slot = Some(pending_preparation(8, cancel, receiver));
        sender
            .send(super::super::super::RomPreparationResult {
                request_id: 7,
                outcome: super::super::super::RomPreparationOutcome::Failed("stale".to_owned()),
            })
            .unwrap();
        sender
            .send(super::super::super::RomPreparationResult {
                request_id: 8,
                outcome: super::super::super::RomPreparationOutcome::Cancelled,
            })
            .unwrap();

        assert!(matches!(
            poll_rom_preparation_slot(&mut slot),
            RomPreparationPoll::Complete(super::super::super::RomPreparationOutcome::Cancelled)
        ));
        assert!(slot.is_none());
        assert!(matches!(
            poll_rom_preparation_slot(&mut slot),
            RomPreparationPoll::Pending
        ));
    }

    #[test]
    fn failed_preparation_is_reported_without_a_commit_payload() {
        let cancel = Arc::new(AtomicBool::new(false));
        let (sender, receiver) = mpsc::channel();
        let mut slot = Some(pending_preparation(11, cancel, receiver));
        sender
            .send(super::super::super::RomPreparationResult {
                request_id: 11,
                outcome: super::super::super::RomPreparationOutcome::Failed("broken".to_owned()),
            })
            .unwrap();

        assert!(matches!(
            poll_rom_preparation_slot(&mut slot),
            RomPreparationPoll::Complete(super::super::super::RomPreparationOutcome::Failed(
                error
            )) if error == "broken"
        ));
        assert!(slot.is_none());
    }
}
