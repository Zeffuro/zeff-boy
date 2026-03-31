use super::App;
mod archive_and_slots;
mod audio_recording;
mod replay;
mod rom_loading;
mod save_states;
mod screenshots;

pub(super) use archive_and_slots::build_slot_labels;
pub(crate) use archive_and_slots::extract_rom_from_zip;

use std::time::Instant;

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
}
