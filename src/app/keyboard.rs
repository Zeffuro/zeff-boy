use super::App;
use crate::emu_backend::ActiveSystem;
use crate::settings::{InputBindingAction, ShortcutAction};
use winit::{
    event::{ElementState, KeyEvent},
    keyboard::{KeyCode, PhysicalKey},
};

mod expressions;
mod pressed;
#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests;

use pressed::PressTarget;
pub(in crate::app) use pressed::{HeldFrontendAction, HeldFrontendSources, PressedKeyboardTargets};

impl App {
    pub(super) fn handle_settings_capture_key(&mut self, key_event: &KeyEvent) -> bool {
        self.sync_effective_input();
        let PhysicalKey::Code(key_code) = key_event.physical_key else {
            return false;
        };
        self.observe_physical_keyboard_key(key_code, key_event.state == ElementState::Pressed);
        let released = key_event.state == ElementState::Released
            && self.release_pressed_keyboard_key(key_code);
        if self.handle_binding_editor_key(key_event, key_code) || released {
            return true;
        }
        if self.cancel_binding_capture_with_escape(key_event, key_code) {
            return true;
        }
        self.handle_rebinding_key(key_event, key_code)
    }

    pub(super) fn handle_keyboard_input(
        &mut self,
        key_event: &KeyEvent,
        event_consumed_by_egui: bool,
    ) {
        self.sync_effective_input();
        let PhysicalKey::Code(key_code) = key_event.physical_key else {
            return;
        };

        self.observe_physical_keyboard_key(key_code, key_event.state == ElementState::Pressed);
        let released = key_event.state == ElementState::Released
            && self.release_pressed_keyboard_key(key_code);
        if self.handle_binding_editor_key(key_event, key_code) || released {
            return;
        }

        if self.cancel_binding_capture_with_escape(key_event, key_code) {
            return;
        }

        #[cfg(not(target_arch = "wasm32"))]
        if key_event.state == ElementState::Pressed
            && matches!(
                key_code,
                KeyCode::Escape | KeyCode::AltLeft | KeyCode::AltRight
            )
            && self.pce_mouse_captured
        {
            self.release_pce_mouse(true);
            return;
        }

        if self.handle_rebinding_key(key_event, key_code) {
            return;
        }

        let egui_has_kb_focus = self.egui_wants_keyboard;

        if egui_has_kb_focus && event_consumed_by_egui {
            self.handle_consumed_keyboard_release(key_event, key_code);
            return;
        }

        if !egui_has_kb_focus
            && !self.pressed_keyboard_targets.gameplay_blocked(key_code)
            && self.game_view_focused
            && !self.modifiers.ctrl
            && !self.modifiers.alt
            && self.handle_coleco_keypad_key(key_event, key_code)
        {
            return;
        }

        if self.handle_shortcut_key(key_event, key_code) {
            return;
        }

        if egui_has_kb_focus {
            return;
        }

        if !self.game_view_focused || self.pressed_keyboard_targets.gameplay_blocked(key_code) {
            return;
        }

        if self.handle_ws_key(key_event, key_code) {
            return;
        }

        if key_event.state == ElementState::Pressed
            && !key_event.repeat
            && self
                .input_configuration
                .resolved
                .typed_binding_sets()
                .any(|(_, source, _)| source == crate::settings::GameplayBindingSource::Keyboard)
        {
            self.pressed_keyboard_targets.press_expression_key(key_code);
            self.refresh_keyboard_expressions();
        }

        self.handle_joypad_key(key_event, key_code);
        self.handle_tilt_key(key_event, key_code);
        self.observe_autofire_host_transitions();
        let _ = self.sync_uncapped_worker();
    }

    fn handle_coleco_keypad_key(&mut self, key_event: &KeyEvent, key_code: KeyCode) -> bool {
        let Some(key) = self.coleco_keypad_key(key_code) else {
            return false;
        };
        match key_event.state {
            ElementState::Pressed if !key_event.repeat => {
                let player = if self.modifiers.shift { 2 } else { 1 };
                self.host_input
                    .set_coleco_keyboard_keypad(player, key, true);
                self.pressed_keyboard_targets
                    .press(key_code, PressTarget::ColecoKeypad { player, key });
                true
            }
            ElementState::Released => {
                self.host_input.set_coleco_keyboard_keypad(1, key, false);
                self.host_input.set_coleco_keyboard_keypad(2, key, false);
                true
            }
            ElementState::Pressed => true,
        }
    }

    fn handle_consumed_keyboard_release(&mut self, key_event: &KeyEvent, key_code: KeyCode) {
        if key_event.state != ElementState::Released {
            return;
        }

        let speedup_code = self.settings.speedup_key_code();
        if key_code == speedup_code || key_code == KeyCode::Backquote {
            self.set_keyboard_frontend_hold(HeldFrontendAction::FastForward, false);
        }

        if key_code == KeyCode::ShiftLeft {
            self.set_keyboard_frontend_hold(HeldFrontendAction::Turbo, false);
        }

        if key_code == self.settings.rewind.key_code() {
            self.set_keyboard_frontend_hold(HeldFrontendAction::Rewind, false);
        }

        if let Some(gb_key) = self.map_key(key_code) {
            self.host_input.set_keyboard(gb_key, false);
        }
        if let Some(gb_key) = self.map_key_p2(key_code) {
            self.host_input.set_keyboard_p2(gb_key, false);
        }
        self.set_pce_multitap_keyboard_key(key_code, false);

        if let Some(key) = self.coleco_keypad_key(key_code) {
            self.host_input.set_coleco_keyboard_keypad(1, key, false);
            self.host_input.set_coleco_keyboard_keypad(2, key, false);
        }

        if let Some(ws_key) = self.map_ws_key(key_code) {
            self.host_input.set_ws_keyboard(ws_key, false);
        }

        if let Some(tilt_key) = self.map_tilt_key(key_code) {
            self.host_input.set_tilt_keyboard(tilt_key, false);
        }
        self.observe_autofire_host_transitions();
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
            use crate::settings::{BindingTarget, GameplayBindingSource, PhysicalBinding};
            let target = match action {
                InputBindingAction::Joypad(action) => BindingTarget::Joypad { player: 1, action },
                InputBindingAction::JoypadP2(action) => BindingTarget::Joypad { player: 2, action },
                InputBindingAction::PceMultitap { player, action } => {
                    BindingTarget::Joypad { player, action }
                }
                InputBindingAction::WonderSwan(action) => BindingTarget::WonderSwan(action),
            };
            self.commit_gameplay_binding(
                target,
                GameplayBindingSource::Keyboard,
                PhysicalBinding::Keyboard(key_code),
            );
            self.pressed_keyboard_targets
                .suppress_gameplay_until_release(key_code);
            self.debug_windows.rebinding_action = None;
        }

        true
    }

    fn cancel_binding_capture_with_escape(
        &mut self,
        key_event: &KeyEvent,
        key_code: KeyCode,
    ) -> bool {
        if key_code != KeyCode::Escape
            || key_event.state != ElementState::Pressed
            || key_event.repeat
            || !self.any_binding_capture_active()
        {
            return false;
        }
        self.clear_rebinding_state();
        self.sync_keyboard_capture(false);
        true
    }

    fn any_binding_capture_active(&self) -> bool {
        self.debug_windows.rebinding_speedup
            || self.debug_windows.rebinding_rewind
            || self.debug_windows.rebinding_shortcut.is_some()
            || self.debug_windows.rebinding_action.is_some()
            || self.debug_windows.rebinding_gamepad.is_some()
            || self.debug_windows.rebinding_gamepad_p2.is_some()
            || self.debug_windows.rebinding_gamepad_pce_multitap.is_some()
            || self.debug_windows.rebinding_ws_gamepad.is_some()
            || self.debug_windows.rebinding_gamepad_action.is_some()
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
                        self.set_keyboard_frontend_hold(HeldFrontendAction::FastForward, true);
                        self.pressed_keyboard_targets.press(
                            key_code,
                            PressTarget::Frontend(HeldFrontendAction::FastForward),
                        );
                    }
                    ElementState::Released => {
                        self.set_keyboard_frontend_hold(HeldFrontendAction::FastForward, false)
                    }
                    _ => {}
                }
                return true;
            }
        }

        if !egui_kb && key_code == KeyCode::ShiftLeft {
            match key_event.state {
                ElementState::Pressed if !key_event.repeat => {
                    self.set_keyboard_frontend_hold(HeldFrontendAction::Turbo, true);
                    self.pressed_keyboard_targets
                        .press(key_code, PressTarget::Frontend(HeldFrontendAction::Turbo));
                }
                ElementState::Released => {
                    self.set_keyboard_frontend_hold(HeldFrontendAction::Turbo, false)
                }
                _ => {}
            }
            return true;
        }

        if !egui_kb && key_code == self.settings.rewind.key_code() {
            if self.core_supports_rewind() {
                match key_event.state {
                    ElementState::Pressed if !key_event.repeat => {
                        if let Err(error) = self.preflight_emu_command_kind(
                            crate::emu_thread::TasControlCommandKind::Rewind,
                        ) {
                            self.toast_manager.error(error.to_string());
                        } else {
                            self.set_keyboard_frontend_hold(HeldFrontendAction::Rewind, true);
                            self.pressed_keyboard_targets
                                .press(key_code, PressTarget::Frontend(HeldFrontendAction::Rewind));
                        }
                    }
                    ElementState::Released => {
                        self.set_keyboard_frontend_hold(HeldFrontendAction::Rewind, false)
                    }
                    _ => {}
                }
            } else {
                self.force_clear_frontend_hold(HeldFrontendAction::Rewind);
            }
            return true;
        }

        if egui_kb {
            return false;
        }

        if self.modifiers.ctrl && self.modifiers.alt && self.media_slot_snapshot.is_some() {
            match key_code {
                KeyCode::KeyA => {
                    if pressed
                        && self
                            .media_slot_snapshot
                            .as_ref()
                            .is_some_and(|snapshot| snapshot.side_count > 0)
                    {
                        self.set_fds_disk_side(0);
                    }
                    return true;
                }
                KeyCode::KeyB => {
                    if pressed
                        && self
                            .media_slot_snapshot
                            .as_ref()
                            .is_some_and(|snapshot| snapshot.side_count > 1)
                    {
                        self.set_fds_disk_side(1);
                    }
                    return true;
                }
                _ => {}
            }
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
            #[cfg(not(target_arch = "wasm32"))]
            if pressed && self.realtime_tas_recording_active() {
                self.stop_realtime_tas_recording();
                self.toast_manager.info("Stopped real-time TAS recording");
                return true;
            }
            if pressed {
                self.toggle_user_paused();
            }
            return true;
        }

        if key_code == bindings.get(ShortcutAction::Fullscreen) {
            if pressed {
                self.toggle_fullscreen();
            }
            return true;
        }

        if key_code == bindings.get(ShortcutAction::SlowMotion) {
            if pressed && !key_event.repeat {
                self.settings.emulation.slow_motion_enabled =
                    !self.settings.emulation.slow_motion_enabled;
                self.settings.save();
                self.toast_manager
                    .info(if self.settings.emulation.slow_motion_enabled {
                        "Slow motion enabled"
                    } else {
                        "Slow motion disabled"
                    });
            }
            return true;
        }

        if key_code == bindings.get(ShortcutAction::UncappedSpeed) {
            if pressed {
                let previous = self.timing.uncapped_speed;
                let enable_uncapped = !previous;
                self.timing.uncapped_speed = enable_uncapped;
                if self.sync_uncapped_worker().is_ok() {
                    self.settings.emulation.uncapped_speed = enable_uncapped;
                    self.settings.save();
                } else {
                    self.timing.uncapped_speed = previous;
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
            #[cfg(not(target_arch = "wasm32"))]
            if self.realtime_tas_recording_active() {
                return true;
            }
            #[cfg(not(target_arch = "wasm32"))]
            if pressed && self.tas_control.can_record_live_input() {
                if let Err(error) = self.record_current_tas_input_and_advance() {
                    self.toast_manager
                        .error(format!("Could not record live TAS input: {error:#}"));
                }
                return true;
            }
            if pressed && self.speed.paused {
                match self.preflight_emu_command_kind(
                    crate::emu_thread::TasControlCommandKind::FrameExecution,
                ) {
                    Ok(()) => {
                        self.debug_requests.frame_advance = true;
                        self.toast_manager.info("▶ Frame +1");
                    }
                    Err(error) => self.toast_manager.error(error.to_string()),
                }
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

        if key_code == bindings.get(ShortcutAction::RotateWs)
            && self.active_system == ActiveSystem::WonderSwan
        {
            if pressed {
                self.toggle_ws_rotation();
            }
            return true;
        }

        if key_code == bindings.get(ShortcutAction::DebugContinue) {
            if pressed {
                if let Err(error) = self.preflight_emu_command_kind(
                    crate::emu_thread::TasControlCommandKind::DebuggerMutation,
                ) {
                    self.toast_manager.error(error.to_string());
                } else {
                    self.debug_requests.continue_ = true;
                }
            }
            return true;
        }

        if key_code == bindings.get(ShortcutAction::DebugStep) {
            if pressed {
                if let Err(error) = self.preflight_emu_command_kind(
                    crate::emu_thread::TasControlCommandKind::DebuggerMutation,
                ) {
                    self.toast_manager.error(error.to_string());
                } else {
                    self.debug_requests.step = true;
                }
            }
            return true;
        }

        false
    }

    fn handle_joypad_key(&mut self, key_event: &KeyEvent, key_code: KeyCode) -> bool {
        let gb_key = self.map_key(key_code);
        let gb_key_p2 = if self.active_system == ActiveSystem::WonderSwan {
            None
        } else {
            self.map_key_p2(key_code)
        };
        let pce_multitap_bound =
            (3..=5).any(|player| self.map_key_pce_multitap(player, key_code).is_some());
        if gb_key.is_none() && gb_key_p2.is_none() && !pce_multitap_bound {
            return false;
        }
        match key_event.state {
            ElementState::Pressed => {
                if !key_event.repeat {
                    if let Some(gb_key) = gb_key {
                        self.host_input.set_keyboard(gb_key, true);
                        self.pressed_keyboard_targets.press(
                            key_code,
                            PressTarget::Joypad {
                                player: 1,
                                button: gb_key,
                            },
                        );
                    }
                    if let Some(gb_key) = gb_key_p2 {
                        self.host_input.set_keyboard_p2(gb_key, true);
                        self.pressed_keyboard_targets.press(
                            key_code,
                            PressTarget::Joypad {
                                player: 2,
                                button: gb_key,
                            },
                        );
                    }
                    self.press_pce_multitap_keyboard_key(key_code);
                    return true;
                }
            }
            ElementState::Released => {
                if let Some(gb_key) = gb_key {
                    self.host_input.set_keyboard(gb_key, false);
                }
                if let Some(gb_key) = gb_key_p2 {
                    self.host_input.set_keyboard_p2(gb_key, false);
                }
                self.set_pce_multitap_keyboard_key(key_code, false);
                return true;
            }
        }

        false
    }

    fn set_pce_multitap_keyboard_key(&mut self, key_code: KeyCode, pressed: bool) {
        for player in 3..=5 {
            let Some(button) = self.map_key_pce_multitap(player, key_code) else {
                continue;
            };
            match player {
                3 => self.host_input.set_keyboard_p3(button, pressed),
                4 => self.host_input.set_keyboard_p4(button, pressed),
                5 => self.host_input.set_keyboard_p5(button, pressed),
                _ => {}
            }
        }
    }

    fn press_pce_multitap_keyboard_key(&mut self, key_code: KeyCode) {
        for player in 3..=5 {
            let Some(button) = self.map_key_pce_multitap(player, key_code) else {
                continue;
            };
            self.set_keyboard_joypad(player, button, true);
            self.pressed_keyboard_targets
                .press(key_code, PressTarget::Joypad { player, button });
        }
    }

    fn handle_ws_key(&mut self, key_event: &KeyEvent, key_code: KeyCode) -> bool {
        if self.active_system != ActiveSystem::WonderSwan {
            return false;
        }

        let Some(ws_key) = self.map_ws_key(key_code) else {
            return false;
        };

        match key_event.state {
            ElementState::Pressed => {
                if !key_event.repeat {
                    self.host_input.set_ws_keyboard(ws_key, true);
                    self.pressed_keyboard_targets
                        .press(key_code, PressTarget::WonderSwan(ws_key));
                    self.observe_autofire_host_transitions();
                    return true;
                }
            }
            ElementState::Released => {
                self.host_input.set_ws_keyboard(ws_key, false);
                self.observe_autofire_host_transitions();
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
                    self.pressed_keyboard_targets
                        .press(key_code, PressTarget::Tilt(tilt_key));
                }
            }
            ElementState::Released => self.host_input.set_tilt_keyboard(tilt_key, false),
        }
    }

    fn set_keyboard_joypad(&mut self, player: u8, button: crate::input::HostButton, pressed: bool) {
        match player {
            1 => self.host_input.set_keyboard(button, pressed),
            2 => self.host_input.set_keyboard_p2(button, pressed),
            3 => self.host_input.set_keyboard_p3(button, pressed),
            4 => self.host_input.set_keyboard_p4(button, pressed),
            5 => self.host_input.set_keyboard_p5(button, pressed),
            _ => {}
        }
    }

    fn release_pressed_keyboard_key(&mut self, key_code: KeyCode) -> bool {
        let handled = self.pressed_keyboard_targets.contains_key(key_code);
        let targets = self.pressed_keyboard_targets.release(key_code);
        for target in targets {
            self.apply_keyboard_target(target, false);
        }
        self.refresh_keyboard_expressions();
        self.observe_autofire_host_transitions();
        handled
    }

    fn refresh_keyboard_expressions(&mut self) {
        if self.egui_wants_keyboard
            || self.keyboard_capture_active
            || !self.game_view_focused
            || !self.game_window_focused
        {
            self.release_gameplay_keyboard_state();
            return;
        }
        let targets = expressions::active_targets(
            &self.input_configuration.resolved,
            self.active_system,
            self.pressed_keyboard_targets.expression_keys(),
            self.media_slot_snapshot.is_some(),
        );
        for (target, pressed) in self
            .pressed_keyboard_targets
            .replace_expression_targets(targets)
        {
            self.apply_keyboard_target(target, pressed);
        }
    }

    fn observe_physical_keyboard_key(&mut self, key: KeyCode, pressed: bool) {
        let keys = &mut self.debug_windows.settings_ui.binding_keyboard_down;
        if pressed {
            if !keys.contains(&key) {
                keys.push(key);
            }
        } else {
            keys.retain(|held| *held != key);
        }
        self.modifiers.shift = keys
            .iter()
            .any(|key| matches!(key, KeyCode::ShiftLeft | KeyCode::ShiftRight));
        self.modifiers.ctrl = keys
            .iter()
            .any(|key| matches!(key, KeyCode::ControlLeft | KeyCode::ControlRight));
        self.modifiers.alt = keys
            .iter()
            .any(|key| matches!(key, KeyCode::AltLeft | KeyCode::AltRight));
    }

    fn handle_binding_editor_key(&mut self, event: &KeyEvent, key: KeyCode) -> bool {
        if self
            .debug_windows
            .settings_ui
            .binding_editor
            .as_ref()
            .is_some_and(|editor| editor.is_capturing())
        {
            self.sync_keyboard_capture(true);
        }
        self.debug_windows
            .settings_ui
            .binding_editor
            .as_mut()
            .is_some_and(|editor| {
                editor.keyboard_event(key, event.state == ElementState::Pressed, event.repeat)
            })
    }

    fn apply_keyboard_target(&mut self, target: PressTarget, pressed: bool) {
        match target {
            PressTarget::Joypad { player, button } => {
                self.set_keyboard_joypad(player, button, pressed)
            }
            PressTarget::WonderSwan(button) => self.host_input.set_ws_keyboard(button, pressed),
            PressTarget::Tilt(action) => self.host_input.set_tilt_keyboard(action, pressed),
            PressTarget::ColecoKeypad { player, key } => self
                .host_input
                .set_coleco_keyboard_keypad(player, key, pressed),
            PressTarget::Frontend(action) => self.set_keyboard_frontend_hold(action, pressed),
        }
    }

    fn set_keyboard_frontend_hold(&mut self, action: HeldFrontendAction, held: bool) {
        let effective = self.held_frontend_sources.set_keyboard(action, held);
        self.set_effective_frontend_hold(action, effective);
    }

    pub(super) fn set_gamepad_frontend_hold(&mut self, action: HeldFrontendAction, held: bool) {
        let effective = self.held_frontend_sources.set_gamepad(action, held);
        self.set_effective_frontend_hold(action, effective);
    }

    pub(super) fn set_remote_frontend_hold(&mut self, action: HeldFrontendAction, held: bool) {
        let effective = self.held_frontend_sources.set_remote(action, held);
        self.set_effective_frontend_hold(action, effective);
    }

    pub(super) fn force_clear_frontend_hold(&mut self, action: HeldFrontendAction) {
        self.held_frontend_sources.clear_action(action);
        self.set_effective_frontend_hold(action, false);
    }

    fn set_effective_frontend_hold(&mut self, action: HeldFrontendAction, held: bool) {
        match action {
            HeldFrontendAction::FastForward => self.speed.fast_forward_held = held,
            HeldFrontendAction::Rewind => self.rewind.held = held,
            HeldFrontendAction::Turbo => {
                self.speed.turbo_held = held;
                self.observe_autofire_host_transitions();
            }
        }
    }

    pub(super) fn clear_keyboard_state(&mut self) {
        let targets: Vec<_> = self.pressed_keyboard_targets.drain().collect();
        for target in targets {
            self.apply_keyboard_target(target, false);
        }
        self.host_input.clear_keyboard();
        self.observe_autofire_host_transitions();
        for action in [
            HeldFrontendAction::FastForward,
            HeldFrontendAction::Rewind,
            HeldFrontendAction::Turbo,
        ] {
            let effective = self.held_frontend_sources.clear_keyboard(action);
            self.set_effective_frontend_hold(action, effective);
        }
        self.modifiers = Default::default();
    }

    pub(super) fn release_gameplay_keyboard_state(&mut self) {
        for target in self.pressed_keyboard_targets.drain_gameplay() {
            self.apply_keyboard_target(target, false);
        }
        self.host_input.clear_keyboard();
        self.observe_autofire_host_transitions();
    }

    pub(super) fn sync_keyboard_capture(&mut self, capture_active: bool) {
        let capture_started = capture_active && !self.keyboard_capture_active;
        self.keyboard_capture_active = capture_active;
        if capture_started {
            self.clear_keyboard_state();
        }
    }

    pub(super) fn clear_all_frontend_holds(&mut self) {
        self.held_frontend_sources.clear_all();
        self.speed.fast_forward_held = false;
        self.rewind.held = false;
        self.speed.turbo_held = false;
        self.observe_autofire_host_transitions();
    }
}
