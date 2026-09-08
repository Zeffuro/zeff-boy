use serde::{Deserialize, Serialize};

use super::{
    GamepadBindings, KeyBindings, PceMultitapKeyBindings, Settings, ShortcutBindings, TiltSettings,
    WonderSwanKeyBindings, default_p2_key_bindings,
};

/// Host input mappings and transforms. Physical reservations and emulated hardware topology are
/// deliberately separate, so applying a profile cannot move a controller or
/// change the identity of a running machine.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub(crate) struct InputProfileBindings {
    pub(crate) keyboard: KeyBindings,
    pub(crate) keyboard_p2: KeyBindings,
    pub(crate) pce_multitap_keyboard: [PceMultitapKeyBindings; 3],
    pub(crate) wonderswan_keyboard: WonderSwanKeyBindings,
    pub(crate) gamepad: GamepadBindings,
    pub(crate) shortcuts: ShortcutBindings,
    pub(crate) speedup_key: String,
    pub(crate) rewind_key: String,
    pub(crate) tilt: TiltSettings,
}

impl Default for InputProfileBindings {
    fn default() -> Self {
        Self {
            keyboard: KeyBindings::default(),
            keyboard_p2: default_p2_key_bindings(),
            pce_multitap_keyboard: std::array::from_fn(|_| PceMultitapKeyBindings::default()),
            wonderswan_keyboard: WonderSwanKeyBindings::default(),
            gamepad: GamepadBindings::default(),
            shortcuts: ShortcutBindings::default(),
            speedup_key: "Space".to_owned(),
            rewind_key: super::RewindSettings::default().key,
            tilt: TiltSettings::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub(crate) struct NamedInputProfile {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) bindings: InputProfileBindings,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub(crate) struct InputProfileCatalog {
    pub(crate) profiles: Vec<NamedInputProfile>,
    pub(crate) next_id: u64,
}

impl Settings {
    pub(crate) fn capture_input_profile(&self) -> InputProfileBindings {
        InputProfileBindings {
            keyboard: self.key_bindings.clone(),
            keyboard_p2: self.key_bindings_p2.clone(),
            pce_multitap_keyboard: self.pce_multitap_key_bindings.clone(),
            wonderswan_keyboard: self.ws_key_bindings.clone(),
            gamepad: self.gamepad_bindings.clone(),
            shortcuts: self.shortcut_bindings.clone(),
            speedup_key: self.speedup_key.clone(),
            rewind_key: self.rewind.key.clone(),
            tilt: self.tilt.clone(),
        }
    }

    pub(crate) fn apply_input_profile(&mut self, profile: &InputProfileBindings) {
        self.key_bindings = profile.keyboard.clone();
        self.key_bindings_p2 = profile.keyboard_p2.clone();
        self.pce_multitap_key_bindings = profile.pce_multitap_keyboard.clone();
        self.ws_key_bindings = profile.wonderswan_keyboard.clone();
        self.gamepad_bindings = profile.gamepad.clone();
        self.shortcut_bindings = profile.shortcuts.clone();
        self.speedup_key = profile.speedup_key.clone();
        self.rewind.key = profile.rewind_key.clone();
        self.tilt = profile.tilt.clone();
    }

    pub(crate) fn save_input_profile(&mut self, name: &str) -> Result<String, String> {
        let name = name.trim();
        if name.is_empty() || name.chars().count() > 64 || name.chars().any(char::is_control) {
            return Err("Use a profile name of 1–64 characters without control characters.".into());
        }
        if self
            .input_profiles
            .profiles
            .iter()
            .any(|profile| profile.name.to_lowercase() == name.to_lowercase())
        {
            return Err("That profile name already exists. Choose a different name.".into());
        }
        if self.input_profiles.profiles.len() >= 64 {
            return Err("You can save up to 64 input profiles. Remove a profile first.".into());
        }
        let mut index = self.input_profiles.next_id.max(1);
        let id = loop {
            let id = format!("profile-{index}");
            index = index
                .checked_add(1)
                .ok_or("Input profile identifiers are exhausted.")?;
            if !self
                .input_profiles
                .profiles
                .iter()
                .any(|profile| profile.id == id)
            {
                break id;
            }
        };
        self.input_profiles.next_id = index;
        self.input_profiles.profiles.push(NamedInputProfile {
            id: id.clone(),
            name: name.to_owned(),
            bindings: self.capture_input_profile(),
        });
        Ok(id)
    }

    pub(crate) fn delete_input_profile(&mut self, id: &str) -> bool {
        let before = self.input_profiles.profiles.len();
        self.input_profiles
            .profiles
            .retain(|profile| profile.id != id);
        self.input_profiles.profiles.len() != before
    }

    pub(crate) fn update_input_profile(&mut self, id: &str) -> Result<(), String> {
        let bindings = self.capture_input_profile();
        let profile = self
            .input_profiles
            .profiles
            .iter_mut()
            .find(|profile| profile.id == id)
            .ok_or_else(|| "That input profile no longer exists.".to_owned())?;
        profile.bindings = bindings;
        Ok(())
    }

    pub(crate) fn reset_input_profile(&mut self, id: &str) -> Result<(), String> {
        let profile = self
            .input_profiles
            .profiles
            .iter_mut()
            .find(|profile| profile.id == id)
            .ok_or_else(|| "That input profile no longer exists.".to_owned())?;
        profile.bindings = InputProfileBindings::default();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::{BindingAction, GamepadAssignment, PceControllerPreference};
    use winit::keyboard::KeyCode;

    #[test]
    fn profiles_restore_all_mapping_families_without_changing_devices_or_machine() {
        let mut settings = Settings::default();
        settings.key_bindings.a = KeyCode::KeyQ;
        settings.key_bindings_p2.a = KeyCode::KeyW;
        settings
            .gamepad_bindings
            .set_for_player(BindingAction::A, 5, "North");
        settings.speedup_key = "KeyE".into();
        settings.rewind.key = "KeyR".into();
        settings.tilt.deadzone = 0.3;
        let profile = settings.capture_input_profile();
        let id = settings.save_input_profile("Arcade").unwrap();
        settings.apply_input_profile(&InputProfileBindings::default());
        settings.input_devices.players[0] = GamepadAssignment::Disabled;
        settings.emulation.pce_controller = PceControllerPreference::Multitap;
        settings.audio.volume = 0.25;
        settings.apply_input_profile(&profile);
        assert_eq!(settings.capture_input_profile(), profile);
        assert_eq!(
            settings.input_devices.players[0],
            GamepadAssignment::Disabled
        );
        assert_eq!(
            settings.emulation.pce_controller,
            PceControllerPreference::Multitap
        );
        assert_eq!(settings.audio.volume, 0.25);
        assert!(settings.delete_input_profile(&id));
        assert_eq!(settings.capture_input_profile(), profile);
    }

    #[test]
    fn profile_names_do_not_silently_replace_existing_mappings() {
        let mut settings = Settings::default();
        settings.save_input_profile("  Living room  ").unwrap();
        let catalog = settings.input_profiles.clone();
        assert!(settings.save_input_profile("living ROOM").is_err());
        assert!(settings.save_input_profile("\n").is_err());
        assert!(settings.save_input_profile(&"a".repeat(65)).is_err());
        assert_eq!(settings.input_profiles, catalog);
    }
}
