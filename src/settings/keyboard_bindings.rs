use serde::de::Deserializer;
use serde::ser::SerializeStruct;
use serde::{Deserialize, Serialize, Serializer};
use winit::keyboard::KeyCode;

use super::binding_actions::BindingAction;
use super::keycode_serde::{keycode_from_string, keycode_to_string};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct KeyBindings {
    pub(crate) up: KeyCode,
    pub(crate) down: KeyCode,
    pub(crate) left: KeyCode,
    pub(crate) right: KeyCode,
    pub(crate) a: KeyCode,
    pub(crate) b: KeyCode,
    pub(crate) start: KeyCode,
    pub(crate) select: KeyCode,
}

impl Default for KeyBindings {
    fn default() -> Self {
        Self {
            up: KeyCode::ArrowUp,
            down: KeyCode::ArrowDown,
            left: KeyCode::ArrowLeft,
            right: KeyCode::ArrowRight,
            a: KeyCode::KeyX,
            b: KeyCode::KeyZ,
            start: KeyCode::Enter,
            select: KeyCode::ShiftRight,
        }
    }
}

impl KeyBindings {
    pub(crate) fn get(&self, action: BindingAction) -> KeyCode {
        match action {
            BindingAction::Up => self.up,
            BindingAction::Down => self.down,
            BindingAction::Left => self.left,
            BindingAction::Right => self.right,
            BindingAction::A => self.a,
            BindingAction::B => self.b,
            BindingAction::Start => self.start,
            BindingAction::Select => self.select,
        }
    }

    pub(crate) fn set(&mut self, action: BindingAction, key: KeyCode) {
        match action {
            BindingAction::Up => self.up = key,
            BindingAction::Down => self.down = key,
            BindingAction::Left => self.left = key,
            BindingAction::Right => self.right = key,
            BindingAction::A => self.a = key,
            BindingAction::B => self.b = key,
            BindingAction::Start => self.start = key,
            BindingAction::Select => self.select = key,
        }
    }
}

impl Serialize for KeyBindings {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut state = serializer.serialize_struct("KeyBindings", 8)?;
        state.serialize_field("up", &keycode_to_string(self.up))?;
        state.serialize_field("down", &keycode_to_string(self.down))?;
        state.serialize_field("left", &keycode_to_string(self.left))?;
        state.serialize_field("right", &keycode_to_string(self.right))?;
        state.serialize_field("a", &keycode_to_string(self.a))?;
        state.serialize_field("b", &keycode_to_string(self.b))?;
        state.serialize_field("start", &keycode_to_string(self.start))?;
        state.serialize_field("select", &keycode_to_string(self.select))?;
        state.end()
    }
}

impl<'de> Deserialize<'de> for KeyBindings {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct RawKeyBindings {
            up: Option<String>,
            down: Option<String>,
            left: Option<String>,
            right: Option<String>,
            a: Option<String>,
            b: Option<String>,
            start: Option<String>,
            select: Option<String>,
        }

        let raw = RawKeyBindings::deserialize(deserializer)?;
        let defaults = Self::default();
        Ok(Self {
            up: raw
                .up
                .as_deref()
                .and_then(keycode_from_string)
                .unwrap_or(defaults.up),
            down: raw
                .down
                .as_deref()
                .and_then(keycode_from_string)
                .unwrap_or(defaults.down),
            left: raw
                .left
                .as_deref()
                .and_then(keycode_from_string)
                .unwrap_or(defaults.left),
            right: raw
                .right
                .as_deref()
                .and_then(keycode_from_string)
                .unwrap_or(defaults.right),
            a: raw
                .a
                .as_deref()
                .and_then(keycode_from_string)
                .unwrap_or(defaults.a),
            b: raw
                .b
                .as_deref()
                .and_then(keycode_from_string)
                .unwrap_or(defaults.b),
            start: raw
                .start
                .as_deref()
                .and_then(keycode_from_string)
                .unwrap_or(defaults.start),
            select: raw
                .select
                .as_deref()
                .and_then(keycode_from_string)
                .unwrap_or(defaults.select),
        })
    }
}

