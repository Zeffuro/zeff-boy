use std::path::{Path, PathBuf};

use crate::emu_backend::{ROM_AND_ARCHIVE_EXTENSIONS, archive_extensions, system_specs};
use crate::emu_thread::EmuCommand;
use crate::rom_archive::PendingArchiveSelection;

#[cfg(not(target_arch = "wasm32"))]
use super::is_native_archive_path;
use super::{App, is_zip_path};

impl App {
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
            self.rewind = super::super::super::RewindState {
                held: false,
                fill: 0.0,
                frames_rewound: 0,
                pending: false,
                backstep_pending: false,
                pacer: super::super::super::RewindPacer::default(),
                pace_updated_at: None,
                scheduled_frames: 0,
                active_mode: None,
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
        if is_native_archive_path(&path) {
            self.begin_native_archive_preparation(
                &path,
                None,
                self.rom_info.rom_path.clone(),
                false,
            );
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
        self.undo_save_state_path = None;
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
        self.rewind.frames_rewound = 0;
        self.rewind.pending = false;
        self.rewind.backstep_pending = false;
        self.rewind.pacer.reset();
        self.rewind.pace_updated_at = None;
        self.rewind.scheduled_frames = 0;
        self.rewind.active_mode = None;

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

    pub(super) fn begin_archive_selection_if_needed(&mut self, path: &Path) -> bool {
        if !is_zip_path(path) {
            return false;
        }
        let entries = match super::super::list_rom_entries_in_zip(path) {
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
}
