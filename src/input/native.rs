use gilrs::{Axis, Button, Event, EventType, GamepadId, Gilrs, ff};

use crate::settings::GamepadBindings;

use super::GamepadPoll;

const RUMBLE_MAGNITUDE: u16 = 40_000;

pub(crate) struct GamepadHandler {
    gilrs: Gilrs,
    active_gamepad: Option<GamepadId>,
    rumble_effect: Option<ff::Effect>,
    rumble_playing: bool,
}

impl GamepadHandler {
    pub(crate) fn new() -> anyhow::Result<Self> {
        let gilrs = Gilrs::new()
            .map_err(|e| anyhow::anyhow!("failed to initialize gamepad subsystem: {e}"))?;
        Ok(Self {
            gilrs,
            active_gamepad: None,
            rumble_effect: None,
            rumble_playing: false,
        })
    }

    pub(crate) fn poll(&mut self, bindings: &GamepadBindings) -> GamepadPoll {
        let mut events = Vec::with_capacity(4);
        let mut action_events = Vec::with_capacity(4);
        let mut raw_pressed = Vec::with_capacity(4);
        while let Some(Event { id, event, .. }) = self.gilrs.next_event() {
            self.active_gamepad = Some(id);
            match event {
                EventType::ButtonPressed(button, _) => {
                    let name = button_name(button);
                    raw_pressed.push(name);
                    if let Some(key) = bindings.map_button_name(name) {
                        events.push((key, true));
                    }
                    if let Some(action) = bindings.map_action_button_name(name) {
                        action_events.push((action, true));
                    }
                }
                EventType::ButtonReleased(button, _) => {
                    let name = button_name(button);
                    if let Some(key) = bindings.map_button_name(name) {
                        events.push((key, false));
                    }
                    if let Some(action) = bindings.map_action_button_name(name) {
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
                let x = gp.value(Axis::LeftStickX).clamp(-1.0, 1.0);
                let y = gp.value(Axis::LeftStickY).clamp(-1.0, 1.0);
                (x, y)
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
                        kind: ff::BaseEffectType::Strong {
                            magnitude: RUMBLE_MAGNITUDE,
                        },
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
                if let Err(e) = effect.play() {
                    log::warn!("Failed to start rumble effect: {e}");
                }
                self.rumble_playing = true;
            }
        } else {
            if let Some(effect) = &mut self.rumble_effect
                && let Err(e) = effect.stop()
            {
                log::warn!("Failed to stop rumble effect: {e}");
            }
            self.rumble_playing = false;
        }
    }
}

impl Drop for GamepadHandler {
    fn drop(&mut self) {
        if let Some(effect) = &mut self.rumble_effect
            && let Err(e) = effect.stop()
        {
            log::warn!("Failed to stop rumble effect on drop: {e}");
        }
    }
}

fn button_name(button: Button) -> &'static str {
    match button {
        Button::South => "South",
        Button::East => "East",
        Button::North => "North",
        Button::West => "West",
        Button::C => "C",
        Button::Z => "Z",
        Button::LeftTrigger => "LeftTrigger",
        Button::LeftTrigger2 => "LeftTrigger2",
        Button::RightTrigger => "RightTrigger",
        Button::RightTrigger2 => "RightTrigger2",
        Button::Select => "Select",
        Button::Start => "Start",
        Button::Mode => "Mode",
        Button::LeftThumb => "LeftThumb",
        Button::RightThumb => "RightThumb",
        Button::DPadUp => "DPadUp",
        Button::DPadDown => "DPadDown",
        Button::DPadLeft => "DPadLeft",
        Button::DPadRight => "DPadRight",
        Button::Unknown => "Unknown",
    }
}
