use super::App;
use crate::emulator::Emulator;

impl App {
    pub(super) fn apply_host_input_to_joypad(&self, emu: &mut Emulator) {
        let buttons_pressed = self.host_input.buttons_pressed();
        let dpad_pressed = self.host_input.dpad_pressed();
        if emu
            .bus
            .io
            .joypad
            .apply_pressed_masks(buttons_pressed, dpad_pressed)
        {
            emu.bus.if_reg |= 0x10;
        }
    }

    pub(super) fn sync_host_input_to_joypad(&self) {
        let Some(emu) = self.emulator.as_ref() else {
            return;
        };
        let mut emu = emu.lock().expect("emulator mutex poisoned");
        self.apply_host_input_to_joypad(&mut emu);
    }

    pub(super) fn current_rom_is_mbc7(&self) -> bool {
        let Some(emu) = self.emulator.as_ref() else {
            return false;
        };
        let emu = emu.lock().expect("emulator mutex poisoned");
        emu.is_mbc7_cartridge()
    }

    pub(super) fn sync_host_input_with_stick_mode(&mut self, is_mbc7: bool) {
        if self.left_stick_controls_dpad(is_mbc7) {
            self.host_input
                .set_gamepad_stick_dpad(self.left_stick, self.settings.tilt_deadzone);
        } else {
            self.host_input.clear_gamepad_stick_dpad();
        }

        self.sync_host_input_to_joypad();
    }

    pub(super) fn update_emulator_tilt(&mut self, smoothed_tilt: (f32, f32)) {
        let Some(emu) = self.emulator.as_ref() else {
            return;
        };
        let mut emu = emu.lock().expect("emulator mutex poisoned");
        emu.set_mbc7_host_tilt(smoothed_tilt.0, smoothed_tilt.1);
    }
}
