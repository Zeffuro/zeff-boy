use super::super::App;
use crate::settings::GamepadAction;

impl App {
    pub(super) fn poll_gamepad(&mut self) {
        if let Some(gamepad) = &mut self.gamepad {
            let poll = gamepad.poll(&self.settings.gamepad_bindings);

            if let Some(action) = self.debug_windows.rebinding_gamepad {
                if let Some(button_name) = poll.raw_pressed.first() {
                    self.settings.gamepad_bindings.set(action, button_name);
                    self.debug_windows.rebinding_gamepad = None;
                }
            } else if let Some(button) = self.debug_windows.rebinding_ws_gamepad {
                if let Some(button_name) = poll.raw_pressed.first() {
                    self.settings.gamepad_bindings.set_ws(button, button_name);
                    self.debug_windows.rebinding_ws_gamepad = None;
                }
            } else if let Some(action) = self.debug_windows.rebinding_gamepad_action {
                if let Some(button_name) = poll.raw_pressed.first() {
                    self.settings
                        .gamepad_bindings
                        .set_action(action, button_name);
                    self.debug_windows.rebinding_gamepad_action = None;
                }
            } else {
                for (key, pressed) in poll.events {
                    self.host_input.set_gamepad(key, pressed);
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

    pub(super) fn gather_joypad_input(&mut self) -> (u8, u8) {
        let (mut buttons, dpad) = if let Some(player) = &mut self.recording.replay_player {
            if let Some((buttons, dpad)) = player.next_frame() {
                (buttons, dpad)
            } else {
                self.toast_manager.info("Replay finished");
                self.recording.replay_player = None;
                self.current_host_joypad_input()
            }
        } else {
            self.current_host_joypad_input()
        };

        if self.speed.turbo_held {
            self.speed.turbo_counter = self.speed.turbo_counter.wrapping_add(1);
            if self.speed.turbo_counter % 2 == 1 {
                buttons = 0;
            }
        } else {
            self.speed.turbo_counter = 0;
        }

        if let Some(recorder) = &mut self.recording.replay_recorder {
            recorder.record_frame(buttons, dpad);
        }

        (buttons, dpad)
    }
}
