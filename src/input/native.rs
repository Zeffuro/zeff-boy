#[cfg(not(target_arch = "wasm32"))]
use gilrs::ff;
use gilrs::{Axis, Button, Event, EventType, GamepadId, Gilrs};

use std::collections::BTreeMap;

use super::routing::{EffectiveGamepads, GamepadRawPress, GamepadRouter, action_bit, ws_bit};
use super::{GamepadCommand, GamepadSnapshot, HostButton, RuntimeGamepadId};
use crate::settings::{GamepadFingerprint, InputDeviceSettings, ResolvedGameplayInput};

use super::GamepadPoll;

#[cfg(not(target_arch = "wasm32"))]
const RUMBLE_MAGNITUDE: u16 = 40_000;

pub(crate) struct GamepadHandler {
    gilrs: Gilrs,
    active_gamepad: Option<GamepadId>,
    connected: BTreeMap<usize, (GamepadId, RuntimeGamepadId)>,
    next_connection: u64,
    router: GamepadRouter,
    effective: EffectiveGamepads,
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
            connected: BTreeMap::new(),
            next_connection: 1,
            router: GamepadRouter::default(),
            effective: EffectiveGamepads::default(),
            #[cfg(not(target_arch = "wasm32"))]
            rumble_effect: None,
            #[cfg(not(target_arch = "wasm32"))]
            rumble_playing: false,
        })
    }

    pub(crate) fn snapshot(&self) -> GamepadSnapshot {
        self.router.snapshot().clone()
    }

    pub(crate) fn apply_command(&mut self, command: GamepadCommand) {
        self.router.apply_command(command);
    }

    fn ensure_connected(&mut self, id: GamepadId) -> RuntimeGamepadId {
        let key = usize::from(id);
        if let Some((_, runtime)) = self.connected.get(&key) {
            return *runtime;
        }
        let runtime = RuntimeGamepadId(self.next_connection);
        self.next_connection = self.next_connection.saturating_add(1);
        let gamepad = self.gilrs.gamepad(id);
        self.router.connect(
            runtime,
            GamepadFingerprint {
                name: gamepad.name().to_owned(),
                uuid: gamepad
                    .uuid()
                    .iter()
                    .map(|byte| format!("{byte:02x}"))
                    .collect(),
            },
        );
        self.connected.insert(key, (id, runtime));
        runtime
    }

    pub(crate) fn poll(
        &mut self,
        resolved: &ResolvedGameplayInput,
        preferences: &InputDeviceSettings,
        capture_active: bool,
        deadzone: f32,
        measure_timing: bool,
    ) -> GamepadPoll {
        let mut raw_pressed = Vec::new();
        let mut latest_event = None;
        let mut observed_events = 0u64;
        let bindings = &resolved.gamepad;
        self.router.configure_typed(resolved);
        // Apply capture/config changes before consuming new physical events.
        self.router
            .resolve(preferences, bindings, capture_active, deadzone);
        while let Some(Event { id, event, .. }) = self.gilrs.next_event() {
            if measure_timing {
                latest_event = Some(crate::platform::Instant::now());
                observed_events = observed_events.saturating_add(1);
            }
            if matches!(event, EventType::Disconnected) {
                if let Some((_, runtime)) = self.connected.remove(&usize::from(id)) {
                    self.router.disconnect(runtime);
                }
                continue;
            }
            if !self.gilrs.gamepad(id).is_connected() {
                continue;
            }
            let newly_connected = !self.connected.contains_key(&usize::from(id));
            let runtime = self.ensure_connected(id);
            if newly_connected {
                self.router
                    .resolve(preferences, bindings, capture_active, deadzone);
            }
            match event {
                EventType::ButtonPressed(button, _) => {
                    let name = button_name(button);
                    let capture_ready = self.router.capture_accepts_press();
                    if self.router.button(runtime, name, true) && capture_ready {
                        raw_pressed.push(GamepadRawPress {
                            device: runtime,
                            button: name,
                        });
                    }
                }
                EventType::ButtonReleased(button, _) => {
                    self.router.button(runtime, button_name(button), false);
                }
                _ => {}
            }
        }
        // Enumerate connected devices even before their first button press, including on web.
        let connected: Vec<_> = self.gilrs.gamepads().map(|(id, _)| id).collect();
        for id in connected {
            let runtime = self.ensure_connected(id);
            let gamepad = self.gilrs.gamepad(id);
            self.router.stick(
                runtime,
                (
                    gamepad.value(Axis::LeftStickX),
                    gamepad.value(Axis::LeftStickY),
                ),
            );
            self.router.right_stick(
                runtime,
                (
                    gamepad.value(Axis::RightStickX),
                    gamepad.value(Axis::RightStickY),
                ),
            );
        }
        let effective = self
            .router
            .resolve(preferences, bindings, capture_active, deadzone);
        let timing = measure_timing.then(|| super::timing::InputPollTiming {
            latest_event,
            observed_events,
            snapshot_complete: crate::platform::Instant::now(),
        });
        let mut player_events: [Vec<(HostButton, bool)>; 5] = std::array::from_fn(|_| Vec::new());
        for (index, events) in player_events.iter_mut().enumerate() {
            for &button in HostButton::WITH_SIX_BUTTONS {
                let bit = button.host_mask_bit();
                let before = self.effective.players[index].buttons & bit != 0;
                let after = effective.players[index].buttons & bit != 0;
                if before != after {
                    events.push((button, after));
                }
            }
        }
        let ws_events = crate::settings::WonderSwanButton::ALL
            .iter()
            .copied()
            .filter_map(|button| {
                let bit = ws_bit(button);
                let before = self.effective.ws & bit != 0;
                let after = effective.ws & bit != 0;
                (before != after).then_some((button, after))
            })
            .collect();
        use crate::settings::GamepadAction;
        let mut action_events: Vec<_> = [
            GamepadAction::SpeedUp,
            GamepadAction::Rewind,
            GamepadAction::Turbo,
        ]
        .into_iter()
        .filter_map(|action| {
            let bit = action_bit(action);
            let before = self.effective.actions & bit != 0;
            let after = effective.actions & bit != 0;
            (before != after).then_some((action, after))
        })
        .collect();
        action_events
            .extend((0..self.router.take_pause_presses()).map(|_| (GamepadAction::Pause, true)));
        let assigned = self.router.assigned_p1().and_then(|runtime| {
            self.connected
                .values()
                .find_map(|&(id, candidate)| (candidate == runtime).then_some(id))
        });
        if self.active_gamepad != assigned {
            self.set_rumble(false);
            #[cfg(not(target_arch = "wasm32"))]
            {
                self.rumble_effect = None;
                self.rumble_playing = false;
            }
            self.active_gamepad = assigned;
        }
        let player_sticks = effective.players.map(|player| player.stick);
        self.effective = effective;
        let [events, events_p2, events_p3, events_p4, events_p5] = player_events;
        GamepadPoll {
            timing,
            events,
            events_p2,
            events_p3,
            events_p4,
            events_p5,
            ws_events,
            action_events,
            left_stick: player_sticks[0],
            player_sticks,
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
