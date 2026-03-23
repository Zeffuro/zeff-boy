use super::App;
use crate::settings::InputBindingAction;
use winit::{
    event::{ElementState, KeyEvent},
    keyboard::{KeyCode, PhysicalKey},
};

impl App {
    pub(super) fn handle_keyboard_input(&mut self, key_event: &KeyEvent) {
        let PhysicalKey::Code(key_code) = key_event.physical_key else {
            return;
        };

        if matches!(key_code, KeyCode::ShiftLeft | KeyCode::ShiftRight) {
            self.shift_held = key_event.state == ElementState::Pressed;
        }

        if self.handle_rebinding_key(key_event, key_code) {
            return;
        }

        if self.handle_shortcut_key(key_event, key_code) {
            return;
        }

        self.handle_joypad_key(key_event, key_code);
        self.handle_tilt_key(key_event, key_code);
    }

    fn handle_rebinding_key(&mut self, key_event: &KeyEvent, key_code: KeyCode) -> bool {
        let Some(action) = self.debug_windows.rebinding_action else {
            return false;
        };

        if key_event.state == ElementState::Pressed && !key_event.repeat {
            match action {
                InputBindingAction::Joypad(a) => self.settings.key_bindings.set(a, key_code),
                InputBindingAction::Tilt(a) => self.settings.tilt_key_bindings.set(a, key_code),
            }
            self.debug_windows.rebinding_action = None;
        }

        true
    }

    fn handle_shortcut_key(&mut self, key_event: &KeyEvent, key_code: KeyCode) -> bool {
        match key_code {
            KeyCode::Tab => {
                match key_event.state {
                    ElementState::Pressed if !key_event.repeat => self.fast_forward_held = true,
                    ElementState::Released => self.fast_forward_held = false,
                    _ => {}
                }
                true
            }
            KeyCode::F11 => {
                if key_event.state == ElementState::Pressed && !key_event.repeat {
                    self.uncapped_speed = !self.uncapped_speed;
                    self.settings.uncapped_speed = self.uncapped_speed;
                    self.settings.save();
                    // Present mode update is handled by sync_speed_setting() next tick.
                }
                true
            }
            KeyCode::F1 | KeyCode::F2 | KeyCode::F3 | KeyCode::F4 => {
                if key_event.state == ElementState::Pressed && !key_event.repeat {
                    if let Some(slot) = Self::keycode_to_state_slot(key_code) {
                        if self.shift_held {
                            self.load_state_slot(slot);
                        } else {
                            self.save_state_slot(slot);
                        }
                    }
                }
                true
            }
            _ => false,
        }
    }

    fn handle_joypad_key(&mut self, key_event: &KeyEvent, key_code: KeyCode) -> bool {
        let Some(gb_key) = self.map_key(key_code) else {
            return false;
        };

        match key_event.state {
            ElementState::Pressed => {
                if !key_event.repeat {
                    self.host_input.set_keyboard(gb_key, true);
                    return true;
                }
            }
            ElementState::Released => {
                self.host_input.set_keyboard(gb_key, false);
                return true;
            }
        }

        false
    }

    fn handle_tilt_key(&mut self, key_event: &KeyEvent, key_code: KeyCode) {
        let Some(tilt_key) = self.map_tilt_key(key_code) else {
            return;
        };

        match key_event.state {
            ElementState::Pressed => {
                if !key_event.repeat {
                    self.host_input.set_tilt_keyboard(tilt_key, true);
                }
            }
            ElementState::Released => self.host_input.set_tilt_keyboard(tilt_key, false),
        }
    }
}
