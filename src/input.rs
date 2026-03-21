use gilrs::{Button, Event, EventType, Gilrs};

use crate::hardware::joypad::JoypadKey;

pub(crate) struct GamepadHandler {
    gilrs: Gilrs,
}

impl GamepadHandler {
    pub(crate) fn new() -> Option<Self> {
        Gilrs::new().ok().map(|gilrs| Self { gilrs })
    }

    pub(crate) fn poll(&mut self) -> Vec<(JoypadKey, bool)> {
        let mut events = Vec::new();
        while let Some(Event { event, .. }) = self.gilrs.next_event() {
            match event {
                EventType::ButtonPressed(button, _) => {
                    if let Some(key) = Self::map_button(button) {
                        events.push((key, true));
                    }
                }
                EventType::ButtonReleased(button, _) => {
                    if let Some(key) = Self::map_button(button) {
                        events.push((key, false));
                    }
                }
                _ => {}
            }
        }
        events
    }

    fn map_button(button: Button) -> Option<JoypadKey> {
        match button {
            Button::South => Some(JoypadKey::A),
            Button::East => Some(JoypadKey::B),
            Button::Start => Some(JoypadKey::Start),
            Button::Select => Some(JoypadKey::Select),
            Button::DPadUp => Some(JoypadKey::Up),
            Button::DPadDown => Some(JoypadKey::Down),
            Button::DPadLeft => Some(JoypadKey::Left),
            Button::DPadRight => Some(JoypadKey::Right),
            _ => None,
        }
    }
}
