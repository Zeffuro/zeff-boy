use gilrs::{Axis, Button, Event, EventType, GamepadId, Gilrs, ff};

use crate::hardware::joypad::JoypadKey;

pub(crate) struct GamepadHandler {
    gilrs: Gilrs,
    active_gamepad: Option<GamepadId>,
    rumble_effect: Option<ff::Effect>,
    rumble_playing: bool,
}

pub(crate) struct GamepadPoll {
    pub(crate) events: Vec<(JoypadKey, bool)>,
    pub(crate) left_stick: (f32, f32),
}

impl GamepadHandler {
    pub(crate) fn new() -> Option<Self> {
        Gilrs::new().ok().map(|gilrs| Self {
            gilrs,
            active_gamepad: None,
            rumble_effect: None,
            rumble_playing: false,
        })
    }

    pub(crate) fn poll(&mut self) -> GamepadPoll {
        let mut events = Vec::new();
        while let Some(Event { id, event, .. }) = self.gilrs.next_event() {
            self.active_gamepad = Some(id);
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
                EventType::Disconnected => {
                    if self.active_gamepad == Some(id) {
                        self.rumble_effect = None;
                        self.rumble_playing = false;
                    }
                }
                _ => {}
            }
        }

        let left_stick = self
            .active_gamepad
            .map(|id| {
                let gp = self.gilrs.gamepad(id);
                (gp.value(Axis::LeftStickX), gp.value(Axis::LeftStickY))
            })
            .unwrap_or((0.0, 0.0));

        GamepadPoll { events, left_stick }
    }

    pub(crate) fn set_rumble(&mut self, active: bool) {
        if active == self.rumble_playing {
            return;
        }

        let Some(gp_id) = self.active_gamepad else {
            return;
        };

        if active {
            if self.rumble_effect.is_none() {
                self.rumble_effect = ff::EffectBuilder::new()
                    .add_effect(ff::BaseEffect {
                        kind: ff::BaseEffectType::Strong { magnitude: 40_000 },
                        scheduling: ff::Replay {
                            play_for: ff::Ticks::from_ms(u32::MAX),
                            with_delay: ff::Ticks::from_ms(0),
                            after: ff::Ticks::from_ms(0),
                        },
                        envelope: Default::default(),
                    })
                    .gamepads(&[gp_id])
                    .finish(&mut self.gilrs)
                    .ok();
            }

            if let Some(effect) = &mut self.rumble_effect {
                let _ = effect.play();
                self.rumble_playing = true;
            }
        } else {
            if let Some(effect) = &mut self.rumble_effect {
                let _ = effect.stop();
            }
            self.rumble_playing = false;
        }
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

impl Drop for GamepadHandler {
    fn drop(&mut self) {
        if let Some(effect) = &mut self.rumble_effect {
            let _ = effect.stop();
        }
    }
}
