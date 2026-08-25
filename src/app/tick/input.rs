use super::super::App;
use super::super::PendingReplayBatch;
use crate::emu_thread::PceMouseInput;
use crate::emu_thread::ReplayJoypadFrame;
use crate::settings::GamepadAction;

fn encode_pce_mouse_axis(delta: f64, sensitivity: f32) -> i16 {
    (-delta * f64::from(sensitivity.clamp(0.25, 4.0)))
        .round()
        .clamp(f64::from(i16::MIN), f64::from(i16::MAX)) as i16
}

impl App {
    pub(super) fn pce_mouse_input(&mut self, consume_motion: bool) -> PceMouseInput {
        let mode = self.settings.emulation.pce_controller.core_mode();
        let memory_base_mode = self.settings.emulation.pce_memory_base.core_mode();
        if self.active_system != crate::emu_backend::ActiveSystem::Pce {
            self.pce_mouse_motion = (0.0, 0.0);
            return PceMouseInput {
                mode,
                memory_base_mode,
                ..Default::default()
            };
        }

        let motion = if consume_motion {
            std::mem::replace(&mut self.pce_mouse_motion, (0.0, 0.0))
        } else {
            (0.0, 0.0)
        };
        let sensitivity = self.settings.emulation.pce_mouse_sensitivity;
        let buttons = u8::from(self.mouse_left_pressed) | (u8::from(self.mouse_right_pressed) << 1);
        PceMouseInput {
            mode,
            memory_base_mode,
            delta_x: encode_pce_mouse_axis(motion.0, sensitivity),
            delta_y: encode_pce_mouse_axis(motion.1, sensitivity),
            buttons,
        }
    }

    pub(super) fn poll_gamepad(&mut self) {
        let supports_rewind = self.core_supports_rewind();
        if let Some(gamepad) = &mut self.gamepad {
            let poll = gamepad.poll(&self.settings.gamepad_bindings);

            if let Some(action) = self.debug_windows.rebinding_gamepad {
                if let Some(button_name) = poll.raw_pressed.first() {
                    self.settings.gamepad_bindings.set(action, button_name);
                    self.debug_windows.rebinding_gamepad = None;
                    self.debug_windows.rebinding_gamepad_p2 = None;
                    self.debug_windows.rebinding_gamepad_pce_multitap = None;
                }
            } else if let Some(action) = self.debug_windows.rebinding_gamepad_p2 {
                if let Some(button_name) = poll.raw_pressed.first() {
                    self.settings.gamepad_bindings.set_p2(action, button_name);
                    self.debug_windows.rebinding_gamepad = None;
                    self.debug_windows.rebinding_gamepad_p2 = None;
                    self.debug_windows.rebinding_gamepad_pce_multitap = None;
                }
            } else if let Some((player, action)) = self.debug_windows.rebinding_gamepad_pce_multitap
            {
                if let Some(button_name) = poll.raw_pressed.first() {
                    self.settings
                        .gamepad_bindings
                        .set_for_player(action, player, button_name);
                    self.debug_windows.rebinding_gamepad_pce_multitap = None;
                }
            } else if let Some(button) = self.debug_windows.rebinding_ws_gamepad {
                if let Some(button_name) = poll.raw_pressed.first() {
                    self.settings.gamepad_bindings.set_ws(button, button_name);
                    self.debug_windows.rebinding_ws_gamepad = None;
                    self.debug_windows.rebinding_gamepad_p2 = None;
                    self.debug_windows.rebinding_gamepad_pce_multitap = None;
                }
            } else if let Some(action) = self.debug_windows.rebinding_gamepad_action {
                if let Some(button_name) = poll.raw_pressed.first() {
                    self.settings
                        .gamepad_bindings
                        .set_action(action, button_name);
                    self.debug_windows.rebinding_gamepad_action = None;
                    self.debug_windows.rebinding_gamepad_p2 = None;
                    self.debug_windows.rebinding_gamepad_pce_multitap = None;
                }
            } else {
                for (key, pressed) in poll.events {
                    self.host_input.set_gamepad(key, pressed);
                }
                for (key, pressed) in poll.events_p2 {
                    self.host_input.set_gamepad_p2(key, pressed);
                }
                for (key, pressed) in poll.events_p3 {
                    self.host_input.set_gamepad_p3(key, pressed);
                }
                for (key, pressed) in poll.events_p4 {
                    self.host_input.set_gamepad_p4(key, pressed);
                }
                for (key, pressed) in poll.events_p5 {
                    self.host_input.set_gamepad_p5(key, pressed);
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
                            self.rewind.held = supports_rewind && pressed;
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
        host_tilt: (f32, f32),
        host_camera_frame: Option<&[u8]>,
    ) -> Option<Vec<ReplayJoypadFrame>> {
        if frames_to_step == 0 {
            return None;
        }

        if let Some(batch) = self.recording.pending_replay_batches.front() {
            return Some(batch.frames.iter().take(frames_to_step).cloned().collect());
        }

        if let Some(player) = self.recording.replay_player.as_ref() {
            let frames_to_step = player.frames_until_next_event(frames_to_step);
            let frames = player
                .peek_joypad_frames(self.recording.queued_replay_playback_frames, frames_to_step);

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

        if self.recording.should_stage_replay_recording_input() {
            let (buttons_p2, dpad_p2) = self.current_host_joypad_p2_input();
            let (buttons_p3, dpad_p3) = self.current_host_joypad_p3_input();
            let (buttons_p4, dpad_p4) = self.current_host_joypad_p4_input();
            let (buttons_p5, dpad_p5) = self.current_host_joypad_p5_input();
            let zapper = if self.active_system == crate::emu_backend::ActiveSystem::Nes {
                self.nes_zapper_input().into()
            } else {
                Default::default()
            };
            let host_tilt = if self.rom_info.is_mbc7 {
                host_tilt
            } else {
                (0.0, 0.0)
            };
            let camera_frame = self
                .rom_info
                .is_pocket_camera
                .then(|| host_camera_frame.map(<[u8]>::to_vec))
                .flatten();
            let frames = vec![
                ReplayJoypadFrame {
                    buttons,
                    dpad,
                    buttons_p2,
                    dpad_p2,
                    buttons_p3,
                    dpad_p3,
                    buttons_p4,
                    dpad_p4,
                    buttons_p5,
                    dpad_p5,
                    zapper,
                    host_tilt,
                    camera_frame,
                };
                frames_to_step
            ];
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

#[cfg(test)]
mod tests {
    use super::encode_pce_mouse_axis;

    #[test]
    fn pce_mouse_axis_uses_host_relative_sign_sensitivity_and_saturation() {
        assert_eq!(encode_pce_mouse_axis(8.0, 1.25), -10);
        assert_eq!(encode_pce_mouse_axis(-8.0, 1.25), 10);
        assert_eq!(encode_pce_mouse_axis(8.0, 0.0), -2);
        assert_eq!(encode_pce_mouse_axis(8.0, 99.0), -32);
        assert_eq!(encode_pce_mouse_axis(-1_000_000.0, 1.0), i16::MAX);
    }
}
