#[cfg(not(target_arch = "wasm32"))]
use gilrs::ff;
use gilrs::{Axis, Button, Event, EventType, GamepadId, Gilrs};

use crate::settings::GamepadBindings;

use super::GamepadPoll;

#[cfg(not(target_arch = "wasm32"))]
const RUMBLE_MAGNITUDE: u16 = 40_000;

pub(crate) struct GamepadHandler {
    gilrs: Gilrs,
    active_gamepad: Option<GamepadId>,
    player_gamepads: [Option<GamepadId>; 5],
    #[cfg(not(target_arch = "wasm32"))]
    rumble_effect: Option<ff::Effect>,
    #[cfg(not(target_arch = "wasm32"))]
    rumble_playing: bool,
}

impl GamepadHandler {
    pub(crate) fn new() -> anyhow::Result<Self> {
        let gilrs = Gilrs::new()
            .map_err(|e| anyhow::anyhow!("failed to initialize gamepad subsystem: {e}"))?;
        Ok(Self {
            gilrs,
            active_gamepad: None,
            player_gamepads: [None; 5],
            #[cfg(not(target_arch = "wasm32"))]
            rumble_effect: None,
            #[cfg(not(target_arch = "wasm32"))]
            rumble_playing: false,
        })
    }

    pub(crate) fn poll(&mut self, bindings: &GamepadBindings) -> GamepadPoll {
        let mut events = Vec::with_capacity(4);
        let mut events_p2 = Vec::with_capacity(4);
        let mut events_p3 = Vec::with_capacity(4);
        let mut events_p4 = Vec::with_capacity(4);
        let mut events_p5 = Vec::with_capacity(4);
        let mut ws_events = Vec::with_capacity(4);
        let mut action_events = Vec::with_capacity(4);
        let mut raw_pressed = Vec::with_capacity(4);
        while let Some(Event { id, event, .. }) = self.gilrs.next_event() {
            let player = self.player_for_gamepad(id);
            match event {
                EventType::ButtonPressed(button, _) => {
                    let name = button_name(button);
                    raw_pressed.push(name);
                    if let Some(key) = bindings.map_button_name_for_player(name, player) {
                        match player {
                            1 => events.push((key, true)),
                            2 => events_p2.push((key, true)),
                            3 => events_p3.push((key, true)),
                            4 => events_p4.push((key, true)),
                            5 => events_p5.push((key, true)),
                            _ => {}
                        }
                    }
                    if player == 1 {
                        if let Some(action) = bindings.map_action_button_name(name) {
                            action_events.push((action, true));
                        }
                        if let Some(button) = bindings.map_ws_button_name(name) {
                            ws_events.push((button, true));
                        }
                    }
                }
                EventType::ButtonReleased(button, _) => {
                    let name = button_name(button);
                    if let Some(key) = bindings.map_button_name_for_player(name, player) {
                        match player {
                            1 => events.push((key, false)),
                            2 => events_p2.push((key, false)),
                            3 => events_p3.push((key, false)),
                            4 => events_p4.push((key, false)),
                            5 => events_p5.push((key, false)),
                            _ => {}
                        }
                    }
                    if player == 1 {
                        if let Some(action) = bindings.map_action_button_name(name) {
                            action_events.push((action, false));
                        }
                        if let Some(button) = bindings.map_ws_button_name(name) {
                            ws_events.push((button, false));
                        }
                    }
                }
                EventType::Disconnected => {
                    let was_active_gamepad = self.active_gamepad == Some(id);
                    self.release_gamepad(id);
                    #[cfg(not(target_arch = "wasm32"))]
                    if was_active_gamepad {
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
            events_p2,
            events_p3,
            events_p4,
            events_p5,
            ws_events,
            action_events,
            left_stick,
            raw_pressed,
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
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

    #[cfg(target_arch = "wasm32")]
    pub(crate) fn set_rumble(&mut self, _active: bool) {}

    fn player_for_gamepad(&mut self, id: GamepadId) -> u8 {
        if self.player_gamepads[0] == Some(id) {
            self.active_gamepad = Some(id);
            return 1;
        }
        for (index, slot) in self.player_gamepads.iter().enumerate().skip(1) {
            if *slot == Some(id) {
                return u8::try_from(index + 1).unwrap_or(1);
            }
        }
        for (index, slot) in self.player_gamepads.iter_mut().enumerate() {
            if slot.is_none() {
                *slot = Some(id);
                if index == 0 {
                    self.active_gamepad = Some(id);
                }
                return u8::try_from(index + 1).unwrap_or(1);
            }
        }
        1
    }

    fn release_gamepad(&mut self, id: GamepadId) {
        for slot in &mut self.player_gamepads {
            if *slot == Some(id) {
                *slot = None;
            }
        }
        if self.active_gamepad == Some(id) {
            self.active_gamepad = self.player_gamepads[0];
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
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
