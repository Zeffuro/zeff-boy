use serde::{Deserialize, Serialize};

/// A model-level hint, not a physical-unit serial or a portable emulation identity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct GamepadFingerprint {
    pub(crate) name: String,
    pub(crate) uuid: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum GamepadAssignment {
    #[default]
    Auto,
    Disabled,
    Reserved(GamepadFingerprint),
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct InputDeviceSettings {
    pub(crate) players: [GamepadAssignment; 5],
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_device_preferences_keep_automatic_assignment() {
        let settings: InputDeviceSettings = serde_json::from_str("{}").unwrap();
        assert_eq!(
            settings.players,
            std::array::from_fn(|_| GamepadAssignment::Auto)
        );
    }

    #[test]
    fn reservations_roundtrip_without_session_identifiers() {
        let mut settings = InputDeviceSettings::default();
        settings.players[1] = GamepadAssignment::Reserved(GamepadFingerprint {
            name: "Controller".into(),
            uuid: "model-id".into(),
        });
        settings.players[4] = GamepadAssignment::Disabled;
        let json = serde_json::to_string(&settings).unwrap();
        assert!(!json.contains("runtime"));
        assert_eq!(
            serde_json::from_str::<InputDeviceSettings>(&json).unwrap(),
            settings
        );
    }
}
