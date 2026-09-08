use serde::{Deserialize, Serialize};

use super::GamepadFingerprint;

pub(crate) const MIN_AXIS_SPAN: f32 = 0.05;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct AxisCalibration {
    pub(crate) min: f32,
    pub(crate) center: f32,
    pub(crate) max: f32,
}

impl Default for AxisCalibration {
    fn default() -> Self {
        Self {
            min: -1.0,
            center: 0.0,
            max: 1.0,
        }
    }
}

impl AxisCalibration {
    pub(crate) fn is_valid(self) -> bool {
        self.min.is_finite()
            && self.center.is_finite()
            && self.max.is_finite()
            && (-1.0..=1.0).contains(&self.min)
            && (-1.0..=1.0).contains(&self.center)
            && (-1.0..=1.0).contains(&self.max)
            && self.center - self.min >= MIN_AXIS_SPAN
            && self.max - self.center >= MIN_AXIS_SPAN
    }

    pub(crate) fn apply(self, raw: f32) -> f32 {
        let raw = normalize_axis(raw);
        if !self.is_valid() {
            return raw;
        }
        let denominator = if raw >= self.center {
            self.max - self.center
        } else {
            self.center - self.min
        };
        ((raw - self.center) / denominator).clamp(-1.0, 1.0)
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct StickCalibration {
    pub(crate) x: AxisCalibration,
    pub(crate) y: AxisCalibration,
}

impl StickCalibration {
    pub(crate) fn is_valid(self) -> bool {
        self.x.is_valid() && self.y.is_valid()
    }

    pub(crate) fn apply(self, raw: (f32, f32)) -> (f32, f32) {
        (self.x.apply(raw.0), self.y.apply(raw.1))
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct GamepadCalibration {
    pub(crate) left: StickCalibration,
    pub(crate) right: StickCalibration,
}

impl GamepadCalibration {
    pub(crate) fn is_valid(self) -> bool {
        self.left.is_valid() && self.right.is_valid()
    }

    pub(crate) fn apply_left(self, raw: (f32, f32)) -> (f32, f32) {
        self.left.apply(raw)
    }

    pub(crate) fn apply_right(self, raw: (f32, f32)) -> (f32, f32) {
        self.right.apply(raw)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct ModelCalibration {
    pub(crate) fingerprint: GamepadFingerprint,
    pub(crate) calibration: GamepadCalibration,
}

fn normalize_axis(value: f32) -> f32 {
    if value.is_finite() {
        value.clamp(-1.0, 1.0)
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn applies_asymmetric_center_and_range() {
        let calibration = AxisCalibration {
            min: -0.75,
            center: 0.2,
            max: 0.8,
        };
        assert_eq!(calibration.apply(0.2), 0.0);
        assert_eq!(calibration.apply(0.5), 0.5);
        assert!((calibration.apply(-0.275) + 0.5).abs() < 1e-6);
        assert_eq!(calibration.apply(-1.0), -1.0);
        assert_eq!(calibration.apply(1.0), 1.0);
    }

    #[test]
    fn invalid_calibration_keeps_normalized_raw_input() {
        let calibration = AxisCalibration {
            min: -1.0,
            center: -0.99,
            max: 1.0,
        };
        assert!(!calibration.is_valid());
        assert_eq!(calibration.apply(0.4), 0.4);
        assert_eq!(calibration.apply(f32::NAN), 0.0);
    }
}
