use super::super::App;
use super::super::PendingReplayBatch;
use crate::emu_thread::ReplayJoypadFrame;
use crate::settings::GamepadAction;

impl App {
    pub(super) fn poll_gamepad(&mut self) {
        if let Some(gamepad) = &mut self.gamepad {
            let poll = gamepad.poll(&self.settings.gamepad_bindings);

            if let Some(action) = self.debug_windows.rebinding_gamepad {
                if let Some(button_name) = poll.raw_pressed.first() {
                    self.settings.gamepad_bindings.set(action, button_name);
                    self.debug_windows.rebinding_gamepad = None;
                    self.debug_windows.rebinding_gamepad_p2 = None;
                }
            } else if let Some(action) = self.debug_windows.rebinding_gamepad_p2 {
                if let Some(button_name) = poll.raw_pressed.first() {
                    self.settings.gamepad_bindings.set_p2(action, button_name);
                    self.debug_windows.rebinding_gamepad = None;
                    self.debug_windows.rebinding_gamepad_p2 = None;
                }
            } else if let Some(button) = self.debug_windows.rebinding_ws_gamepad {
                if let Some(button_name) = poll.raw_pressed.first() {
                    self.settings.gamepad_bindings.set_ws(button, button_name);
                    self.debug_windows.rebinding_ws_gamepad = None;
                    self.debug_windows.rebinding_gamepad_p2 = None;
                }
            } else if let Some(action) = self.debug_windows.rebinding_gamepad_action {
                if let Some(button_name) = poll.raw_pressed.first() {
                    self.settings
                        .gamepad_bindings
                        .set_action(action, button_name);
                    self.debug_windows.rebinding_gamepad_action = None;
                    self.debug_windows.rebinding_gamepad_p2 = None;
                }
            } else {
                for (key, pressed) in poll.events {
                    self.host_input.set_gamepad(key, pressed);
                }
                for (key, pressed) in poll.events_p2 {
                    self.host_input.set_gamepad_p2(key, pressed);
                }
                for (button, pressed) in poll.ws_events {
                    self.host_input.set_ws_gamepad(button, pressed);
                }
                for (action, pressed) in poll.action_events {
                    match action {
                        GamepadAction::SpeedUp => {
                            self.speed.fast_forward_held = pressed;
                        }
                        GamepadAction::Rewind => {
                            self.rewind.held = pressed;
                        }
                        GamepadAction::Pause => {
                            if pressed {
                                self.speed.paused = !self.speed.paused;
                                self.toast_manager.set_paused(self.speed.paused);
                            }
                        }
                        GamepadAction::Turbo => {
                            self.speed.turbo_held = pressed;
                        }
                    }
                }
            }

            self.tilt.left_stick = poll.left_stick;
        }
    }

    pub(super) fn current_replay_recordable_joypad_input(&mut self) -> (u8, u8) {
        let (mut buttons, dpad) = self.current_host_joypad_input();
        if self.speed.turbo_held {
            self.speed.turbo_counter = self.speed.turbo_counter.wrapping_add(1);
            if self.speed.turbo_counter % 2 == 1 {
                buttons = 0;
            }
        } else {
            self.speed.turbo_counter = 0;
        }

        (buttons, dpad)
    }

    pub(super) fn prepare_replay_joypad_batch(
        &mut self,
        frames_to_step: usize,
        buttons: u8,
        dpad: u8,
    ) -> Option<Vec<ReplayJoypadFrame>> {
        if frames_to_step == 0 {
            return None;
        }

        if let Some(batch) = self.recording.pending_replay_batches.front() {
            return Some(batch.frames.iter().copied().take(frames_to_step).collect());
        }

        if let Some(player) = self.recording.replay_player.as_ref() {
            let frames = player
                .peek_frames(self.recording.queued_replay_playback_frames, frames_to_step)
                .into_iter()
                .map(|(buttons, dpad)| ReplayJoypadFrame { buttons, dpad })
                .collect::<Vec<_>>();

            if frames.is_empty() {
                self.toast_manager.info("Replay finished");
                self.recording.replay_player = None;
                self.recording.queued_replay_playback_frames = 0;
                return None;
            }

            self.recording.queued_replay_playback_frames += frames.len();
            self.recording
                .pending_replay_batches
                .push_back(PendingReplayBatch {
                    frames: frames.clone(),
                    record: false,
                    playback: true,
                });
            return Some(frames);
        }

        if self.recording.replay_recorder.is_some() {
            let frames = vec![ReplayJoypadFrame { buttons, dpad }; frames_to_step];
            self.recording
                .pending_replay_batches
                .push_back(PendingReplayBatch {
                    frames: frames.clone(),
                    record: true,
                    playback: false,
                });
            return Some(frames);
        }

        None
    }
}
