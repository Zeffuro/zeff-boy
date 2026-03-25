use gilrs::{Axis, Event, EventType, GamepadId, Gilrs, ff};

use crate::hardware::joypad::JoypadKey;
use crate::settings::{GamepadAction, GamepadBindings};

pub(crate) struct GamepadHandler {
    gilrs: Gilrs,
    active_gamepad: Option<GamepadId>,
    rumble_effect: Option<ff::Effect>,
    rumble_playing: bool,
}

pub(crate) struct GamepadPoll {
    pub(crate) events: Vec<(JoypadKey, bool)>,
    pub(crate) action_events: Vec<(GamepadAction, bool)>,
    pub(crate) left_stick: (f32, f32),
    pub(crate) raw_pressed: Vec<String>,
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

    pub(crate) fn poll(&mut self, bindings: &GamepadBindings) -> GamepadPoll {
        let mut events = Vec::new();
        let mut action_events = Vec::new();
        let mut raw_pressed = Vec::new();
        while let Some(Event { id, event, .. }) = self.gilrs.next_event() {
            self.active_gamepad = Some(id);
            match event {
                EventType::ButtonPressed(button, _) => {
                    let name = format!("{button:?}");
                    raw_pressed.push(name.clone());
                    if let Some(key) = bindings.map_button_name(&name) {
                        events.push((key, true));
                    }
                    if let Some(action) = bindings.map_action_button_name(&name) {
                        action_events.push((action, true));
                    }
                }
                EventType::ButtonReleased(button, _) => {
                    let name = format!("{button:?}");
                    if let Some(key) = bindings.map_button_name(&name) {
                        events.push((key, false));
                    }
                    if let Some(action) = bindings.map_action_button_name(&name) {
                        action_events.push((action, false));
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

        GamepadPoll {
            events,
            action_events,
            left_stick,
            raw_pressed,
        }
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
}

impl Drop for GamepadHandler {
    fn drop(&mut self) {
        if let Some(effect) = &mut self.rumble_effect {
            let _ = effect.stop();
        }
    }
}
