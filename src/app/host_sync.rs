use super::App;

impl App {
    pub(super) fn sync_host_input_with_stick_mode(&mut self, is_mbc7: bool) {
        if self.left_stick_controls_dpad(is_mbc7) {
            self.host_input
                .set_gamepad_stick_dpad(self.left_stick, self.settings.tilt_deadzone);
        } else {
            self.host_input.clear_gamepad_stick_dpad();
        }
        // Joypad input is now sent via StepFrames command — no mutex lock needed.
    }
}
