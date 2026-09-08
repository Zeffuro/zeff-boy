use serde::{Deserialize, Serialize};

use super::{GamepadCalibration, ModelCalibration};

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

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct InputDeviceSettings {
    pub(crate) players: [GamepadAssignment; 5],
    pub(crate) calibrations: Vec<ModelCalibration>,
}

impl InputDeviceSettings {
    pub(crate) fn calibration_for(
        &self,
        fingerprint: &GamepadFingerprint,
    ) -> Option<GamepadCalibration> {
        self.calibrations
            .iter()
            .find(|entry| entry.fingerprint == *fingerprint)
            .map(|entry| entry.calibration)
    }

    pub(crate) fn set_calibration(
        &mut self,
        fingerprint: GamepadFingerprint,
        calibration: GamepadCalibration,
    ) {
        if let Some(existing) = self
            .calibrations
            .iter_mut()
            .find(|entry| entry.fingerprint == fingerprint)
        {
            existing.calibration = calibration;
        } else {
            self.calibrations.push(ModelCalibration {
                fingerprint,
                calibration,
            });
        }
    }

    pub(crate) fn remove_calibration(&mut self, fingerprint: &GamepadFingerprint) -> bool {
        let before = self.calibrations.len();
        self.calibrations
            .retain(|entry| entry.fingerprint != *fingerprint);
        self.calibrations.len() != before
    }

    pub(crate) fn discard_invalid_calibrations(&mut self) -> usize {
        let before = self.calibrations.len();
        self.calibrations
            .retain(|entry| entry.calibration.is_valid());
        before - self.calibrations.len()
    }
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
        assert!(settings.calibrations.is_empty());
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

    #[test]
    fn calibration_is_replaced_by_model_hint_and_can_be_removed() {
        let fingerprint = GamepadFingerprint {
            name: "Controller".into(),
            uuid: "model-id".into(),
        };
        let mut settings = InputDeviceSettings::default();
        settings.set_calibration(fingerprint.clone(), GamepadCalibration::default());
        let replacement = GamepadCalibration {
            left: super::super::StickCalibration {
                x: super::super::AxisCalibration {
                    min: -0.8,
                    center: 0.1,
                    max: 0.9,
                },
                ..Default::default()
            },
            ..Default::default()
        };
        settings.set_calibration(fingerprint.clone(), replacement);
        assert_eq!(settings.calibrations.len(), 1);
        assert_eq!(settings.calibration_for(&fingerprint), Some(replacement));
        assert!(settings.remove_calibration(&fingerprint));
        assert!(settings.calibrations.is_empty());
    }

    #[test]
    fn invalid_calibrations_are_discarded_before_runtime_routing() {
        let mut settings = InputDeviceSettings::default();
        settings.set_calibration(
            GamepadFingerprint {
                name: "Controller".into(),
                uuid: "model-id".into(),
            },
            GamepadCalibration {
                left: super::super::StickCalibration {
                    x: super::super::AxisCalibration {
                        min: -1.0,
                        center: -0.99,
                        max: 1.0,
                    },
                    ..Default::default()
                },
                ..Default::default()
            },
        );
        assert_eq!(settings.discard_invalid_calibrations(), 1);
        assert!(settings.calibrations.is_empty());
    }
}
