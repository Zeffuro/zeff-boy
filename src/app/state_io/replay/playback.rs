use super::App;
use crate::emu_thread::{EmuCommand, EmuResponse, TasControlCommandKind};
use zeff_emu_common::replay::ReplayPlayer;

impl App {
    pub(in crate::app) fn play_replay(&mut self, player: ReplayPlayer) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.core_supports_replay(),
            "the active core does not support replay"
        );
        anyhow::ensure!(
            !self.recording.is_replay_finalizing(),
            "replay save is still finishing"
        );
        anyhow::ensure!(
            self.recording.replay_recorder.is_none() && !self.recording.is_replay_start_pending(),
            "stop replay recording before playback"
        );
        self.preflight_emu_command_kind(TasControlCommandKind::Replay)?;
        self.validate_replay_playback(&player)?;
        self.suspend_uncapped_worker()?;
        let command = EmuCommand::LoadStateBytes {
            state_bytes: player.save_state().to_vec(),
            buttons_pressed: 0,
            dpad_pressed: 0,
            replay_events: Some(player.metadata().events.clone()),
            game_boy_link_start_state: player.metadata().game_boy_link_start_state,
            game_boy_link_coordinator_start_state: player
                .metadata()
                .game_boy_link_coordinator_start_state,
            game_boy_link_start_tick: player.metadata().game_boy_link_start_tick,
            wonder_swan_link_start_tick: player.metadata().wonder_swan_link_start_tick,
        };
        if let Err(error) = self.send_emu_command_checked(command) {
            self.resume_uncapped_worker_after_replay();
            return Err(error.into());
        }
        match self.recv_cold_response() {
            Some(EmuResponse::LoadStateOk {
                media_slot_snapshot,
                game_boy_serial_device,
                ..
            }) => {
                self.media_slot_snapshot = media_slot_snapshot;
                if let Some(device) = game_boy_serial_device {
                    self.game_boy_serial_device = device;
                }
                if let Some(thread) = &self.emu_thread {
                    self.latest_frame = thread.shared_framebuffer().load_full();
                }
                self.clear_replay_progress();
                let total = player.total_frames();
                self.recording.replay_player = Some(player);
                self.toast_manager
                    .info(format!("Playing replay ({total} frames)"));
                Ok(())
            }
            response => {
                self.resume_uncapped_worker_after_replay();
                match response {
                    Some(EmuResponse::LoadStateFailed(error)) => anyhow::bail!("{error}"),
                    _ => anyhow::bail!("emulator worker did not acknowledge replay restore"),
                }
            }
        }
    }
}
