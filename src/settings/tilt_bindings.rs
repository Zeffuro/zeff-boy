use serde::de::Deserializer;
use serde::ser::SerializeStruct;
use serde::{Deserialize, Serialize, Serializer};
use winit::keyboard::KeyCode;

use super::keycode_serde::{keycode_to_string, parse_key_or_default};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TiltBindingAction {
    Up,
    Down,
    Left,
    Right,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TiltKeyBindings {
    pub(crate) up: KeyCode,
    pub(crate) down: KeyCode,
    pub(crate) left: KeyCode,
    pub(crate) right: KeyCode,
}

impl Default for TiltKeyBindings {
    fn default() -> Self {
        Self {
            up: KeyCode::KeyW,
            down: KeyCode::KeyS,
            left: KeyCode::KeyA,
            right: KeyCode::KeyD,
        }
    }
}

impl TiltKeyBindings {
    pub(crate) fn get(&self, action: TiltBindingAction) -> KeyCode {
        match action {
            TiltBindingAction::Up => self.up,
            TiltBindingAction::Down => self.down,
            TiltBindingAction::Left => self.left,
            TiltBindingAction::Right => self.right,
        }
    }

    pub(crate) fn set(&mut self, action: TiltBindingAction, key: KeyCode) {
        match action {
            TiltBindingAction::Up => self.up = key,
            TiltBindingAction::Down => self.down = key,
            TiltBindingAction::Left => self.left = key,
            TiltBindingAction::Right => self.right = key,
        }
    }

    pub(crate) fn set_wasd_defaults(&mut self) {
        *self = Self::default();
    }
}

impl Serialize for TiltKeyBindings {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut state = serializer.serialize_struct("TiltKeyBindings", 4)?;
        state.serialize_field("up", &keycode_to_string(self.up))?;
        state.serialize_field("down", &keycode_to_string(self.down))?;
        state.serialize_field("left", &keycode_to_string(self.left))?;
        state.serialize_field("right", &keycode_to_string(self.right))?;
        state.end()
    }
}

impl<'de> Deserialize<'de> for TiltKeyBindings {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct RawTiltKeyBindings {
            up: Option<String>,
            down: Option<String>,
            left: Option<String>,
            right: Option<String>,
        }

        let raw = RawTiltKeyBindings::deserialize(deserializer)?;
        let d = Self::default();
        Ok(Self {
            up: parse_key_or_default(raw.up.as_deref(), d.up),
            down: parse_key_or_default(raw.down.as_deref(), d.down),
            left: parse_key_or_default(raw.left.as_deref(), d.left),
            right: parse_key_or_default(raw.right.as_deref(), d.right),
        })
    }
}
