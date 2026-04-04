use super::App;
use crate::emu_thread::EmuCommand;
use crate::settings::{InputBindingAction, ShortcutAction};
use winit::{
    event::{ElementState, KeyEvent},
    keyboard::{KeyCode, PhysicalKey},
};

impl App {
    pub(super) fn handle_keyboard_input(
        &mut self,
        key_event: &KeyEvent,
        event_consumed_by_egui: bool,
    ) {
        let PhysicalKey::Code(key_code) = key_event.physical_key else {
            return;
        };

        if matches!(key_code, KeyCode::ShiftLeft | KeyCode::ShiftRight) {
            self.modifiers.shift = key_event.state == ElementState::Pressed;
        }
        if matches!(key_code, KeyCode::ControlLeft | KeyCode::ControlRight) {
            self.modifiers.ctrl = key_event.state == ElementState::Pressed;
        }
        if matches!(key_code, KeyCode::AltLeft | KeyCode::AltRight) {
            self.modifiers.alt = key_event.state == ElementState::Pressed;
        }

        let egui_has_kb_focus = self.egui_wants_keyboard;

        if egui_has_kb_focus && event_consumed_by_egui {
            self.handle_consumed_keyboard_release(key_event, key_code);
            return;
        }

        if self.handle_rebinding_key(key_event, key_code) {
            return;
        }

        if self.handle_shortcut_key(key_event, key_code) {
            return;
        }

        if egui_has_kb_focus {
            return;
        }

        if !self.game_view_focused {
            return;
        }

        self.handle_joypad_key(key_event, key_code);
        self.handle_tilt_key(key_event, key_code);
    }

    fn handle_consumed_keyboard_release(&mut self, key_event: &KeyEvent, key_code: KeyCode) {
        if key_event.state != ElementState::Released {
            return;
        }

        let speedup_code = self.settings.speedup_key_code();
        if key_code == speedup_code || key_code == KeyCode::Backquote {
            self.speed.fast_forward_held = false;
        }

        if key_code == KeyCode::ShiftLeft {
            self.speed.turbo_held = false;
        }

        if key_code == self.settings.rewind.key_code() {
            self.rewind.held = false;
        }

        if let Some(gb_key) = self.map_key(key_code) {
            self.host_input.set_keyboard(gb_key, false);
        }

        if let Some(tilt_key) = self.map_tilt_key(key_code) {
            self.host_input.set_tilt_keyboard(tilt_key, false);
        }
    }

    fn handle_rebinding_key(&mut self, key_event: &KeyEvent, key_code: KeyCode) -> bool {
        if self.debug_windows.rebinding_speedup {
            if key_event.state == ElementState::Pressed && !key_event.repeat {
                self.settings.speedup_key = format!("{key_code:?}");
                self.debug_windows.rebinding_speedup = false;
            }
            return true;
        }

        if self.debug_windows.rebinding_rewind {
            if key_event.state == ElementState::Pressed && !key_event.repeat {
                self.settings.rewind.key = format!("{key_code:?}");
                self.debug_windows.rebinding_rewind = false;
            }
            return true;
        }

        if let Some(shortcut_action) = self.debug_windows.rebinding_shortcut {
            if key_event.state == ElementState::Pressed && !key_event.repeat {
                self.settings
                    .shortcut_bindings
                    .set(shortcut_action, key_code);
                self.debug_windows.rebinding_shortcut = None;
            }
            return true;
        }

        let Some(action) = self.debug_windows.rebinding_action else {
            return false;
        };

        if key_event.state == ElementState::Pressed && !key_event.repeat {
            match action {
                InputBindingAction::Joypad(a) => self.settings.key_bindings.set(a, key_code),
                InputBindingAction::Tilt(a) => self.settings.tilt.key_bindings.set(a, key_code),
            }
            self.debug_windows.rebinding_action = None;
        }

        true
    }

    fn handle_shortcut_key(&mut self, key_event: &KeyEvent, key_code: KeyCode) -> bool {
        let pressed = key_event.state == ElementState::Pressed && !key_event.repeat;

        if self.modifiers.ctrl && key_code == KeyCode::KeyR {
            if pressed {
                self.reset_game();
            }
            return true;
        }

        if self.modifiers.alt && key_code == KeyCode::Enter {
            if pressed {
                self.toggle_fullscreen();
            }
            return true;
        }

        let egui_kb = self.egui_wants_keyboard;

        if !egui_kb {
            let speedup_code = self.settings.speedup_key_code();
            if key_code == speedup_code || key_code == KeyCode::Backquote {
                match key_event.state {
                    ElementState::Pressed if !key_event.repeat => {
                        self.speed.fast_forward_held = true
                    }
                    ElementState::Released => self.speed.fast_forward_held = false,
                    _ => {}
                }
                return true;
            }
        }

        if !egui_kb && key_code == KeyCode::ShiftLeft {
            match key_event.state {
                ElementState::Pressed if !key_event.repeat => self.speed.turbo_held = true,
                ElementState::Released => self.speed.turbo_held = false,
                _ => {}
            }
            return true;
        }

        if !egui_kb && key_code == self.settings.rewind.key_code() {
            match key_event.state {
                ElementState::Pressed => self.rewind.held = true,
                ElementState::Released => self.rewind.held = false,
            }
            return true;
        }

        if egui_kb {
            return false;
        }

        let digit_slot = match key_code {
            KeyCode::Digit0 => Some(0u8),
            KeyCode::Digit1 => Some(1),
            KeyCode::Digit2 => Some(2),
            KeyCode::Digit3 => Some(3),
            KeyCode::Digit4 => Some(4),
            KeyCode::Digit5 => Some(5),
            KeyCode::Digit6 => Some(6),
            KeyCode::Digit7 => Some(7),
            KeyCode::Digit8 => Some(8),
            KeyCode::Digit9 => Some(9),
            _ => None,
        };
        if let Some(slot) = digit_slot {
            if pressed {
                self.active_save_slot = slot;
                self.toast_manager
                    .info(format!("Save slot {slot} selected"));
            }
            return pressed;
        }

        let bindings = &self.settings.shortcut_bindings;

        if key_code == bindings.get(ShortcutAction::Pause) || key_code == KeyCode::Pause {
            if pressed {
                self.speed.paused = !self.speed.paused;
                self.toast_manager.set_paused(self.speed.paused);
            }
            return true;
        }

        if key_code == bindings.get(ShortcutAction::Fullscreen) {
            if pressed {
                self.toggle_fullscreen();
            }
            return true;
        }

        if key_code == bindings.get(ShortcutAction::UncappedSpeed) {
            if pressed {
                self.timing.uncapped_speed = !self.timing.uncapped_speed;
                self.settings.emulation.uncapped_speed = self.timing.uncapped_speed;
                self.settings.save();
                if let Some(thread) = &self.emu_thread {
                    thread.send(EmuCommand::SetUncapped(self.timing.uncapped_speed));
                }
            }
            return true;
        }

        if key_code == bindings.get(ShortcutAction::MuteToggle) {
            if pressed {
                if self.settings.audio.volume > 0.0 {
                    self.settings.audio.pre_mute_volume = Some(self.settings.audio.volume);
                    self.settings.audio.volume = 0.0;
                    self.toast_manager.info("🔇 Muted");
                } else {
                    self.settings.audio.volume = self.settings.audio.pre_mute_volume.unwrap_or(1.0);
                    self.settings.audio.pre_mute_volume = None;
                    self.toast_manager.info("🔊 Unmuted");
                }
            }
            return true;
        }

        if key_code == bindings.get(ShortcutAction::Screenshot) {
            if pressed {
                self.take_screenshot();
            }
            return true;
        }

        if key_code == bindings.get(ShortcutAction::ResetGame) {
            if pressed {
                self.reset_game();
            }
            return true;
        }

        if key_code == bindings.get(ShortcutAction::FrameAdvance) {
            if pressed && self.speed.paused {
                self.debug_requests.frame_advance = true;
                self.toast_manager.info("▶ Frame +1");
            }
            return true;
        }

        if key_code == bindings.get(ShortcutAction::QuickSave) {
            if pressed {
                self.save_state_slot(self.active_save_slot);
            }
            return true;
        }

        if key_code == bindings.get(ShortcutAction::QuickLoad) {
            if pressed {
                self.load_state_slot(self.active_save_slot);
            }
            return true;
        }

        if key_code == bindings.get(ShortcutAction::SlotNext) {
            if pressed {
                self.active_save_slot = (self.active_save_slot + 1) % 10;
                self.toast_manager
                    .info(format!("Save slot {}", self.active_save_slot));
            }
            return true;
        }

        if key_code == bindings.get(ShortcutAction::SlotPrev) {
            if pressed {
                self.active_save_slot = (self.active_save_slot + 9) % 10;
                self.toast_manager
                    .info(format!("Save slot {}", self.active_save_slot));
            }
            return true;
        }

        if key_code == bindings.get(ShortcutAction::DebugContinue) {
            if pressed {
                self.debug_requests.continue_ = true;
            }
            return true;
        }

        if key_code == bindings.get(ShortcutAction::DebugStep) {
            if pressed {
                self.debug_requests.step = true;
            }
            return true;
        }

        false
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
