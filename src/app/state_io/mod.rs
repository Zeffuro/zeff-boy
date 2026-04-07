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

use crate::platform::Instant;

impl App {
    fn pause_for_dialog(&mut self) -> bool {
        let was_paused = self.speed.paused;
        self.speed.paused = true;
        was_paused
    }

    fn resume_after_dialog(&mut self, was_paused: bool) {
        self.speed.paused = was_paused;
        self.timing.last_frame_time = Instant::now();
    }
}
