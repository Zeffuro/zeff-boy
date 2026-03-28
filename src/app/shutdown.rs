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

        if self.settings.auto_save_state
            && let Some(thread) = &self.emu_thread {
                thread.send(EmuCommand::AutoSaveState);
                match self.recv_cold_response_shutdown() {
                    Some(EmuResponse::SaveStateOk(path)) => {
                        log::info!("Auto-saved state to {}", path);
                    }
                    Some(EmuResponse::SaveStateFailed(err)) => {
                        log::warn!("Auto-save failed: {}", err);
                    }
                    _ => {}
                }
            }

        self.settings.open_debug_tabs = crate::debug::save_open_tabs(&self.debug_dock);
        self.settings.save();

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

        self.gfx = None;
        self.audio = None;
        self.window_id = None;
        self.latest_frame = None;
    }

    fn recv_cold_response_shutdown(&mut self) -> Option<EmuResponse> {
        loop {
            let result = self.emu_thread.as_ref()?.try_recv_frame();
            match result {
                Some(frame) => self.process_frame_result(frame),
                None => break,
            }
        }
        self.emu_thread.as_ref()?.recv()
    }
}
