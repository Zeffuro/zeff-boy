use crate::settings::{
    AutofirePattern, InputGameKey, InputOverrides, InputProfileBindings, InputScope,
    ResolvedGameplayInput, Settings,
};

pub(super) struct ScopedInputRuntime {
    pub(super) resolved: ResolvedGameplayInput,
    pub(super) autofire: [[Option<AutofirePattern>; 8]; 5],
    pub(super) game: Option<InputGameKey>,
    scope: InputScope,
    global: InputProfileBindings,
    overrides: InputOverrides,
}

impl ScopedInputRuntime {
    pub(super) fn new(settings: &Settings) -> Self {
        Self {
            resolved: settings.resolve_gameplay_input(&InputScope::Global),
            autofire: settings.resolve_autofire(&InputScope::Global),
            game: None,
            scope: InputScope::Global,
            global: settings.capture_input_profile(),
            overrides: settings.input_overrides.clone(),
        }
    }

    fn refresh(&mut self, settings: &Settings) -> bool {
        for action in [
            crate::settings::GamepadAction::SpeedUp,
            crate::settings::GamepadAction::Rewind,
            crate::settings::GamepadAction::Pause,
            crate::settings::GamepadAction::Turbo,
        ] {
            self.resolved
                .gamepad
                .set_action(action, settings.gamepad_bindings.get_action(action));
        }
        let scope = self
            .game
            .clone()
            .map_or(InputScope::Global, InputScope::Game);
        if scope == self.scope
            && self
                .overrides
                .matches_runtime_scope(&settings.input_overrides, &scope)
            && settings.gameplay_input_matches(&self.global)
        {
            return false;
        }
        let resolved = settings.resolve_gameplay_input(&scope);
        let autofire = settings.resolve_autofire(&scope);
        let changed = scope != self.scope || resolved != self.resolved || autofire != self.autofire;
        self.scope = scope;
        self.resolved = resolved;
        self.autofire = autofire;
        self.global = settings.capture_input_profile();
        self.overrides = settings.input_overrides.clone();
        changed
    }
}

impl super::App {
    pub(super) fn sync_effective_input(&mut self) {
        if self.input_configuration.refresh(&self.settings) {
            self.autofire_rearm_pending = true;
            self.release_gameplay_keyboard_state();
            self.host_input.clear_gamepad();
            self.tilt.left_stick = (0.0, 0.0);
            self.tilt.smoothed = (0.0, 0.0);
            self.tilt.auto_source = super::AutoTiltSource::Keyboard;
            if let Some(gamepad) = &mut self.gamepad {
                gamepad.apply_command(crate::input::GamepadCommand::Neutralize);
            }
            for action in [
                super::keyboard::HeldFrontendAction::FastForward,
                super::keyboard::HeldFrontendAction::Rewind,
                super::keyboard::HeldFrontendAction::Turbo,
            ] {
                self.set_gamepad_frontend_hold(action, false);
            }
        }
    }

    pub(super) fn set_loaded_input_game(
        &mut self,
        game: Option<InputGameKey>,
        name: Option<String>,
    ) {
        if self.input_configuration.game != game {
            self.clear_rebinding_state();
            self.debug_windows.settings_ui.input_capture_scope = None;
        }
        self.input_configuration.game = game.clone();
        self.debug_windows.settings_ui.current_input_game = game;
        self.debug_windows.settings_ui.current_input_game_name = name;
        self.sync_effective_input();
    }

    pub(super) fn commit_gameplay_binding(
        &mut self,
        target: crate::settings::BindingTarget,
        source: crate::settings::GameplayBindingSource,
        value: crate::settings::PhysicalBinding,
    ) {
        let scope = self
            .debug_windows
            .settings_ui
            .input_capture_scope
            .take()
            .unwrap_or_default();
        if let InputScope::Game(game) = &scope
            && self.input_configuration.game.as_ref() != Some(game)
        {
            self.clear_rebinding_state();
            return;
        }
        match self
            .settings
            .set_binding(&scope, target, source, Some(value))
        {
            Ok(()) => {
                self.settings.save();
                self.sync_effective_input();
            }
            Err(error) => self.toast_manager.error(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::{
        BindingAction, BindingTarget, GameplayBindingSource, InputSystem, PhysicalBinding,
    };
    use winit::keyboard::KeyCode;

    #[test]
    fn global_gamepad_action_edits_refresh_without_a_gameplay_barrier() {
        let mut settings = Settings::default();
        let mut runtime = ScopedInputRuntime::new(&settings);
        runtime.game = Some(InputGameKey::new(InputSystem::GameBoyAdvance, [1; 32]));
        assert!(runtime.refresh(&settings));
        let target = BindingTarget::Joypad {
            player: 1,
            action: BindingAction::A,
        };
        let before = runtime.resolved.keyboard_binding(target);
        settings
            .gamepad_bindings
            .set_action(crate::settings::GamepadAction::Pause, "North");
        assert!(!runtime.refresh(&settings));
        assert_eq!(
            runtime
                .resolved
                .gamepad
                .get_action(crate::settings::GamepadAction::Pause),
            "North"
        );
        assert_eq!(runtime.resolved.keyboard_binding(target), before);
        settings
            .gamepad_bindings
            .set_action(crate::settings::GamepadAction::Pause, "");
        assert!(!runtime.refresh(&settings));
        assert_eq!(
            runtime
                .resolved
                .gamepad
                .get_action(crate::settings::GamepadAction::Pause),
            ""
        );
    }

    #[test]
    fn editing_an_inactive_system_does_not_change_live_input() {
        let mut settings = Settings::default();
        let mut runtime = ScopedInputRuntime::new(&settings);
        let target = BindingTarget::Joypad {
            player: 1,
            action: BindingAction::A,
        };
        settings
            .set_binding(
                &InputScope::System(InputSystem::GameBoyAdvance),
                target,
                GameplayBindingSource::Keyboard,
                Some(PhysicalBinding::Keyboard(KeyCode::KeyQ)),
            )
            .unwrap();
        assert!(!runtime.refresh(&settings));
        assert_eq!(
            runtime.resolved.keyboard_binding(target),
            Some(KeyCode::KeyX)
        );
        runtime.game = Some(InputGameKey::new(InputSystem::GameBoyAdvance, [1; 32]));
        assert!(runtime.refresh(&settings));
        assert_eq!(
            runtime.resolved.keyboard_binding(target),
            Some(KeyCode::KeyQ)
        );
        runtime.game = Some(InputGameKey::new(InputSystem::GameBoyAdvance, [2; 32]));
        assert!(runtime.refresh(&settings));
        assert_eq!(
            runtime.resolved.keyboard_binding(target),
            Some(KeyCode::KeyQ)
        );
    }
}
