use serde::de::Deserializer;
use serde::ser::SerializeStruct;
use serde::{Deserialize, Serialize, Serializer};
use winit::keyboard::KeyCode;

use super::binding_actions::{BindingAction, WonderSwanButton};
use super::keycode_serde::{keycode_to_string, parse_key_or_default};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct KeyBindings {
    pub(crate) up: KeyCode,
    pub(crate) down: KeyCode,
    pub(crate) left: KeyCode,
    pub(crate) right: KeyCode,
    pub(crate) a: KeyCode,
    pub(crate) b: KeyCode,
    pub(crate) x: KeyCode,
    pub(crate) y: KeyCode,
    pub(crate) l: KeyCode,
    pub(crate) r: KeyCode,
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
            x: KeyCode::KeyC,
            y: KeyCode::KeyV,
            l: KeyCode::KeyA,
            r: KeyCode::KeyS,
            start: KeyCode::Enter,
            select: KeyCode::ShiftRight,
        }
    }
}

impl KeyBindings {
    pub(crate) fn player_two_defaults() -> Self {
        Self {
            up: KeyCode::Numpad8,
            down: KeyCode::Numpad5,
            left: KeyCode::Numpad4,
            right: KeyCode::Numpad6,
            a: KeyCode::Numpad1,
            b: KeyCode::Numpad2,
            x: KeyCode::Numpad3,
            y: KeyCode::NumpadDecimal,
            l: KeyCode::Numpad7,
            r: KeyCode::Numpad9,
            start: KeyCode::NumpadEnter,
            select: KeyCode::Numpad0,
        }
    }

    pub(crate) fn get(&self, action: BindingAction) -> KeyCode {
        match action {
            BindingAction::Up => self.up,
            BindingAction::Down => self.down,
            BindingAction::Left => self.left,
            BindingAction::Right => self.right,
            BindingAction::A => self.a,
            BindingAction::B => self.b,
            BindingAction::X => self.x,
            BindingAction::Y => self.y,
            BindingAction::L => self.l,
            BindingAction::R => self.r,
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
            BindingAction::X => self.x = key,
            BindingAction::Y => self.y = key,
            BindingAction::L => self.l = key,
            BindingAction::R => self.r = key,
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
        let mut state = serializer.serialize_struct("KeyBindings", 12)?;
        state.serialize_field("up", &keycode_to_string(self.up))?;
        state.serialize_field("down", &keycode_to_string(self.down))?;
        state.serialize_field("left", &keycode_to_string(self.left))?;
        state.serialize_field("right", &keycode_to_string(self.right))?;
        state.serialize_field("a", &keycode_to_string(self.a))?;
        state.serialize_field("b", &keycode_to_string(self.b))?;
        state.serialize_field("x", &keycode_to_string(self.x))?;
        state.serialize_field("y", &keycode_to_string(self.y))?;
        state.serialize_field("l", &keycode_to_string(self.l))?;
        state.serialize_field("r", &keycode_to_string(self.r))?;
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
            x: Option<String>,
            y: Option<String>,
            l: Option<String>,
            r: Option<String>,
            start: Option<String>,
            select: Option<String>,
        }

        let raw = RawKeyBindings::deserialize(deserializer)?;
        let d = Self::default();
        Ok(Self {
            up: parse_key_or_default(raw.up.as_deref(), d.up),
            down: parse_key_or_default(raw.down.as_deref(), d.down),
            left: parse_key_or_default(raw.left.as_deref(), d.left),
            right: parse_key_or_default(raw.right.as_deref(), d.right),
            a: parse_key_or_default(raw.a.as_deref(), d.a),
            b: parse_key_or_default(raw.b.as_deref(), d.b),
            x: parse_key_or_default(raw.x.as_deref(), d.x),
            y: parse_key_or_default(raw.y.as_deref(), d.y),
            l: parse_key_or_default(raw.l.as_deref(), d.l),
            r: parse_key_or_default(raw.r.as_deref(), d.r),
            start: parse_key_or_default(raw.start.as_deref(), d.start),
            select: parse_key_or_default(raw.select.as_deref(), d.select),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WonderSwanKeyBindings {
    pub(crate) x1: KeyCode,
    pub(crate) x2: KeyCode,
    pub(crate) x3: KeyCode,
    pub(crate) x4: KeyCode,
    pub(crate) y1: KeyCode,
    pub(crate) y2: KeyCode,
    pub(crate) y3: KeyCode,
    pub(crate) y4: KeyCode,
    pub(crate) a: KeyCode,
    pub(crate) b: KeyCode,
    pub(crate) start: KeyCode,
}

impl Default for WonderSwanKeyBindings {
    fn default() -> Self {
        Self {
            x1: KeyCode::KeyW,
            x2: KeyCode::KeyD,
            x3: KeyCode::KeyS,
            x4: KeyCode::KeyA,
            y1: KeyCode::ArrowUp,
            y2: KeyCode::ArrowRight,
            y3: KeyCode::ArrowDown,
            y4: KeyCode::ArrowLeft,
            a: KeyCode::KeyX,
            b: KeyCode::KeyZ,
            start: KeyCode::Enter,
        }
    }
}

impl WonderSwanKeyBindings {
    pub(crate) fn get(&self, action: WonderSwanButton) -> KeyCode {
        match action {
            WonderSwanButton::X1 => self.x1,
            WonderSwanButton::X2 => self.x2,
            WonderSwanButton::X3 => self.x3,
            WonderSwanButton::X4 => self.x4,
            WonderSwanButton::Y1 => self.y1,
            WonderSwanButton::Y2 => self.y2,
            WonderSwanButton::Y3 => self.y3,
            WonderSwanButton::Y4 => self.y4,
            WonderSwanButton::A => self.a,
            WonderSwanButton::B => self.b,
            WonderSwanButton::Start => self.start,
        }
    }

    pub(crate) fn set(&mut self, action: WonderSwanButton, key: KeyCode) {
        match action {
            WonderSwanButton::X1 => self.x1 = key,
            WonderSwanButton::X2 => self.x2 = key,
            WonderSwanButton::X3 => self.x3 = key,
            WonderSwanButton::X4 => self.x4 = key,
            WonderSwanButton::Y1 => self.y1 = key,
            WonderSwanButton::Y2 => self.y2 = key,
            WonderSwanButton::Y3 => self.y3 = key,
            WonderSwanButton::Y4 => self.y4 = key,
            WonderSwanButton::A => self.a = key,
            WonderSwanButton::B => self.b = key,
            WonderSwanButton::Start => self.start = key,
        }
    }
}

impl Serialize for WonderSwanKeyBindings {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut state = serializer.serialize_struct("WonderSwanKeyBindings", 11)?;
        state.serialize_field("x1", &keycode_to_string(self.x1))?;
        state.serialize_field("x2", &keycode_to_string(self.x2))?;
        state.serialize_field("x3", &keycode_to_string(self.x3))?;
        state.serialize_field("x4", &keycode_to_string(self.x4))?;
        state.serialize_field("y1", &keycode_to_string(self.y1))?;
        state.serialize_field("y2", &keycode_to_string(self.y2))?;
        state.serialize_field("y3", &keycode_to_string(self.y3))?;
        state.serialize_field("y4", &keycode_to_string(self.y4))?;
        state.serialize_field("a", &keycode_to_string(self.a))?;
        state.serialize_field("b", &keycode_to_string(self.b))?;
        state.serialize_field("start", &keycode_to_string(self.start))?;
        state.end()
    }
}

impl<'de> Deserialize<'de> for WonderSwanKeyBindings {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct RawWonderSwanKeyBindings {
            x1: Option<String>,
            x2: Option<String>,
            x3: Option<String>,
            x4: Option<String>,
            y1: Option<String>,
            y2: Option<String>,
            y3: Option<String>,
            y4: Option<String>,
            a: Option<String>,
            b: Option<String>,
            start: Option<String>,
        }

        let raw = RawWonderSwanKeyBindings::deserialize(deserializer)?;
        let d = Self::default();
        Ok(Self {
            x1: parse_key_or_default(raw.x1.as_deref(), d.x1),
            x2: parse_key_or_default(raw.x2.as_deref(), d.x2),
            x3: parse_key_or_default(raw.x3.as_deref(), d.x3),
            x4: parse_key_or_default(raw.x4.as_deref(), d.x4),
            y1: parse_key_or_default(raw.y1.as_deref(), d.y1),
            y2: parse_key_or_default(raw.y2.as_deref(), d.y2),
            y3: parse_key_or_default(raw.y3.as_deref(), d.y3),
            y4: parse_key_or_default(raw.y4.as_deref(), d.y4),
            a: parse_key_or_default(raw.a.as_deref(), d.a),
            b: parse_key_or_default(raw.b.as_deref(), d.b),
            start: parse_key_or_default(raw.start.as_deref(), d.start),
        })
    }
}
