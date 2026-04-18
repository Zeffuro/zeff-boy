use super::App;
use crate::emu_thread::{EmuCommand, EmuResponse};

impl App {
    pub(super) fn stop_emu_thread(&mut self) {
        if let Some(mut thread) = self.emu_thread.take() {
            thread.shutdown();
        }
    }

    pub(super) fn perform_shutdown(&mut self) {
        if self.shutdown_performed {
            return;
        }
        self.shutdown_performed = true;

        self.stop_audio_recording();
        self.stop_replay_recording();

        if self.settings.emulation.auto_save_state
            && let Some(thread) = &self.emu_thread
        {
            thread.send(EmuCommand::AutoSaveState);
            match self.recv_cold_response() {
                Some(EmuResponse::SaveStateOk(path)) => {
                    log::info!("Auto-saved state to {}", path);
                }
                Some(EmuResponse::SaveStateFailed(err)) => {
                    log::warn!("Auto-save failed: {}", err);
                }
                _ => {}
            }
        }

        self.settings.ui.open_debug_tabs = crate::debug::save_open_tabs(&self.debug_dock);
        self.settings.save();

        self.save_current_cheats();

        self.stop_emu_thread();
        self.stop_camera_capture();

        self.gfx = None;
        self.audio = None;
        self.window_id = None;
        self.latest_frame = None;
    }
}
