use super::App;
mod archive_and_slots;
mod audio_recording;
mod cheats_setup;
mod replay;
mod rom_loading;
mod save_states;
mod screenshots;
mod wasm_rom;

pub(crate) use archive_and_slots::SlotInfo;
pub(super) use archive_and_slots::build_slot_info;
pub(crate) use archive_and_slots::extract_rom_from_zip;
#[allow(unused_imports)] // Used on WASM for drag-and-drop ROM loading
pub(super) use archive_and_slots::extract_rom_from_zip_bytes;
pub(super) use archive_and_slots::{
    extract_rom_entry_from_zip, extract_rom_entry_path_from_zip, list_rom_entries_in_zip,
};
pub(crate) use rom_loading::detect_and_extract_rom;
#[cfg(not(target_arch = "wasm32"))]
pub(crate) use rom_loading::is_native_seven_zip_path;

use crate::debug::FpsTracker;
use crate::emu_backend::EmuBackend;
use crate::emu_thread::{EmuCommand, EmuThread};
use crate::platform::Instant;

impl App {
    pub(super) fn pause_for_dialog(&mut self) -> bool {
        let was_paused = self.speed.paused;
        self.suppress_unfocus_pause_until_focus = true;
        self.speed.paused = true;
        was_paused
    }

    pub(super) fn resume_after_dialog(&mut self, was_paused: bool) {
        let was_paused_by_unfocus = self.paused_by_unfocus;
        self.speed.paused = was_paused;
        self.paused_by_unfocus = false;
        if was_paused_by_unfocus && !was_paused {
            self.toast_manager.set_paused(false);
        }
        self.timing.last_frame_time = Instant::now();
    }

    pub(super) fn spawn_emu_thread(&mut self, backend: EmuBackend) {
        self.emu_thread = Some(EmuThread::spawn(backend));
        if let (Some(thread), Some(recorder)) =
            (&self.emu_thread, self.recording.audio_recorder.as_ref())
        {
            thread.send(EmuCommand::SetAudioRecordingCapture {
                capture: crate::emu_thread::AudioRecordingCapture {
                    active: true,
                    semantic: recorder.captures_semantics(),
                },
                acknowledged: None,
            });
        }
        if let Some(thread) = &self.emu_thread {
            self.latest_frame = thread.shared_framebuffer().load_full();
        }
        self.fps_tracker = FpsTracker::new();
        self.timing.last_frame_time = Instant::now();

        if self.timing.uncapped_speed
            && self.recording.allows_uncapped_worker()
            && let Some(thread) = &self.emu_thread
        {
            thread.send(EmuCommand::SetUncapped(true));
        }
    }
}
