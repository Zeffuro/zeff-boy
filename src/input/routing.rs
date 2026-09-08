use std::collections::{BTreeMap, BTreeSet};

use crate::settings::{
    GamepadAssignment, GamepadBindings, GamepadFingerprint, InputDeviceSettings,
};

/// Monotonically allocated per connection; deliberately not serializable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct RuntimeGamepadId(pub(crate) u64);

#[derive(Debug, Clone)]
pub(crate) enum GamepadCommand {
    Identify {
        player: u8,
        device: RuntimeGamepadId,
    },
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum GamepadAssignmentStatus {
    Disabled,
    #[default]
    Waiting,
    Ambiguous,
    Connected,
}

#[derive(Debug, Clone)]
pub(crate) struct GamepadDeviceSnapshot {
    pub(crate) id: RuntimeGamepadId,
    pub(crate) fingerprint: GamepadFingerprint,
    pub(crate) buttons: Vec<String>,
    pub(crate) left_stick: (f32, f32),
    pub(crate) right_stick: (f32, f32),
    pub(crate) waiting_for_neutral: bool,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct GamepadPlayerSnapshot {
    pub(crate) status: GamepadAssignmentStatus,
    pub(crate) device: Option<RuntimeGamepadId>,
    pub(crate) buttons: u16,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct GamepadSnapshot {
    pub(crate) devices: Vec<GamepadDeviceSnapshot>,
    pub(crate) players: [GamepadPlayerSnapshot; 5],
    pub(crate) capture_active: bool,
    pub(crate) capture_ready: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct GamepadRawPress {
    pub(crate) device: RuntimeGamepadId,
    pub(crate) button: &'static str,
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(super) struct EffectivePlayer {
    pub(super) buttons: u16,
    pub(super) stick: (f32, f32),
}

#[derive(Debug, Clone, Default, PartialEq)]
pub(super) struct EffectiveGamepads {
    pub(super) players: [EffectivePlayer; 5],
    pub(super) ws: u16,
    pub(super) actions: u8,
}

struct DeviceState {
    fingerprint: GamepadFingerprint,
    buttons: BTreeSet<&'static str>,
    stick: (f32, f32),
    right_stick: (f32, f32),
    waiting_for_neutral: bool,
}

#[derive(Default)]
pub(super) struct GamepadRouter {
    devices: BTreeMap<RuntimeGamepadId, DeviceState>,
    assigned: [Option<RuntimeGamepadId>; 5],
    identified: [Option<RuntimeGamepadId>; 5],
    require_identification: [bool; 5],
    preferences: InputDeviceSettings,
    bindings: Option<GamepadBindings>,
    snapshot: GamepadSnapshot,
    capture_active: bool,
    capture_ready: bool,
    neutral_threshold: f32,
    pending_pause_presses: Vec<RuntimeGamepadId>,
}

impl GamepadRouter {
    pub(super) fn connect(&mut self, id: RuntimeGamepadId, fingerprint: GamepadFingerprint) {
        self.devices.entry(id).or_insert(DeviceState {
            fingerprint,
            buttons: BTreeSet::new(),
            stick: (0.0, 0.0),
            right_stick: (0.0, 0.0),
            waiting_for_neutral: self.capture_active,
        });
    }

    pub(super) fn disconnect(&mut self, id: RuntimeGamepadId) {
        self.devices.remove(&id);
        for selected in &mut self.identified {
            if *selected == Some(id) {
                *selected = None;
            }
        }
    }

    pub(super) fn button(
        &mut self,
        id: RuntimeGamepadId,
        name: &'static str,
        pressed: bool,
    ) -> bool {
        let Some(device) = self.devices.get_mut(&id) else {
            return false;
        };
        let new_press = if pressed {
            device.buttons.insert(name)
        } else {
            device.buttons.remove(name);
            false
        };
        if new_press
            && !self.capture_active
            && !device.waiting_for_neutral
            && self.assigned[0] == Some(id)
            && self.bindings.as_ref().is_some_and(|bindings| {
                bindings.map_action_button_name(name) == Some(crate::settings::GamepadAction::Pause)
            })
        {
            self.pending_pause_presses.push(id);
        }
        self.arm_capture_if_neutral();
        new_press
    }

    pub(super) fn stick(&mut self, id: RuntimeGamepadId, stick: (f32, f32)) {
        if let Some(device) = self.devices.get_mut(&id) {
            device.stick = (normalize_axis(stick.0), normalize_axis(stick.1));
        }
    }

    pub(super) fn apply_command(&mut self, command: GamepadCommand) {
        match command {
            GamepadCommand::Identify { player, device } => {
                if let Some(index) = player.checked_sub(1).map(usize::from).filter(|&i| i < 5)
                    && self.devices.contains_key(&device)
                {
                    self.identified[index] = Some(device);
                }
            }
        }
    }

    pub(super) fn right_stick(&mut self, id: RuntimeGamepadId, stick: (f32, f32)) {
        if let Some(device) = self.devices.get_mut(&id) {
            device.right_stick = (normalize_axis(stick.0), normalize_axis(stick.1));
        }
    }

    pub(super) fn snapshot(&self) -> &GamepadSnapshot {
        &self.snapshot
    }

    pub(super) fn assigned_p1(&self) -> Option<RuntimeGamepadId> {
        self.assigned[0]
    }

    pub(super) fn capture_accepts_press(&self) -> bool {
        !self.capture_active || self.capture_ready
    }

    fn arm_capture_if_neutral(&mut self) {
        if self.capture_active
            && !self.capture_ready
            && self.devices.values().all(|device| {
                device.buttons.is_empty()
                    && device.stick.0.abs() < self.neutral_threshold
                    && device.stick.1.abs() < self.neutral_threshold
            })
        {
            self.capture_ready = true;
        }
    }

    pub(super) fn take_pause_presses(&mut self) -> usize {
        let presses = std::mem::take(&mut self.pending_pause_presses);
        presses
            .into_iter()
            .filter(|&id| {
                !self.capture_active
                    && self.assigned[0] == Some(id)
                    && self
                        .devices
                        .get(&id)
                        .is_some_and(|device| !device.waiting_for_neutral)
            })
            .count()
    }

    pub(super) fn resolve(
        &mut self,
        preferences: &InputDeviceSettings,
        bindings: &GamepadBindings,
        capture_active: bool,
        deadzone: f32,
    ) -> EffectiveGamepads {
        self.neutral_threshold = if deadzone.is_finite() {
            deadzone.clamp(0.01, 1.0)
        } else {
            0.25
        };
        if capture_active != self.capture_active {
            self.capture_ready = false;
        }
        self.capture_active = capture_active;
        self.arm_capture_if_neutral();
        let old_assigned = self.assigned;
        let mut assigned = [None; 5];
        let mut status = [GamepadAssignmentStatus::Waiting; 5];
        let mut claimed = BTreeSet::new();
        // A reserved but ambiguous model must not silently become another player's Auto pad.
        let reserved: BTreeSet<_> = self
            .devices
            .iter()
            .filter_map(|(&id, device)| {
                preferences.players.iter().any(|choice| {
                matches!(choice, GamepadAssignment::Reserved(fp) if *fp == device.fingerprint)
            }).then_some(id)
            })
            .collect();
        for (index, choice) in preferences.players.iter().enumerate() {
            if self.preferences.players[index] != *choice {
                self.require_identification[index] = false;
            }
            match choice {
                GamepadAssignment::Disabled => status[index] = GamepadAssignmentStatus::Disabled,
                GamepadAssignment::Auto => {}
                GamepadAssignment::Reserved(fingerprint) => {
                    let candidates: Vec<_> = self
                        .devices
                        .iter()
                        .filter_map(|(&id, device)| {
                            (device.fingerprint == *fingerprint && !claimed.contains(&id))
                                .then_some(id)
                        })
                        .collect();
                    let shared_reservation = preferences.players.iter().filter(|choice|
                        matches!(choice, GamepadAssignment::Reserved(fp) if fp == fingerprint)).count() > 1;
                    if candidates.len() > 1 || shared_reservation {
                        self.require_identification[index] = true;
                    }
                    let identified = self.identified[index].filter(|id| candidates.contains(id));
                    let selected = identified.or_else(|| {
                        (candidates.len() == 1 && !self.require_identification[index])
                            .then(|| candidates[0])
                    });
                    if let Some(id) = selected {
                        assigned[index] = Some(id);
                        claimed.insert(id);
                        status[index] = GamepadAssignmentStatus::Connected;
                    } else if !candidates.is_empty() {
                        status[index] = GamepadAssignmentStatus::Ambiguous;
                    }
                }
            }
        }
        // Keep existing Auto seats before allocating new ones; connection order only fills vacancies.
        for (index, choice) in preferences.players.iter().enumerate() {
            if matches!(choice, GamepadAssignment::Auto)
                && let Some(id) = old_assigned[index]
                && self.devices.contains_key(&id)
                && !claimed.contains(&id)
                && !reserved.contains(&id)
            {
                assigned[index] = Some(id);
                claimed.insert(id);
            }
        }
        for (index, choice) in preferences.players.iter().enumerate() {
            if matches!(choice, GamepadAssignment::Auto) {
                if assigned[index].is_none() {
                    assigned[index] = self
                        .devices
                        .keys()
                        .copied()
                        .find(|id| !claimed.contains(id) && !reserved.contains(id));
                }
                if let Some(id) = assigned[index] {
                    claimed.insert(id);
                    status[index] = GamepadAssignmentStatus::Connected;
                }
            }
        }
        let bindings_changed = self.bindings.as_ref().is_some_and(|old| old != bindings);
        if bindings_changed {
            self.pending_pause_presses.clear();
        }
        for (index, (&old, &new)) in old_assigned.iter().zip(&assigned).enumerate() {
            let existing_source_moved = old != new
                && (old.is_some()
                    || new.is_some_and(|id| {
                        self.snapshot.devices.iter().any(|device| device.id == id)
                    }));
            if existing_source_moved
                || self.preferences.players[index] != preferences.players[index]
            {
                for id in old.into_iter().chain(new) {
                    if let Some(device) = self.devices.get_mut(&id) {
                        device.waiting_for_neutral = true;
                    }
                }
            }
        }
        let threshold = self.neutral_threshold;
        for device in self.devices.values_mut() {
            if capture_active || bindings_changed {
                device.waiting_for_neutral = true;
            } else if device.buttons.is_empty()
                && device.stick.0.abs() < threshold
                && device.stick.1.abs() < threshold
            {
                device.waiting_for_neutral = false;
            }
        }
        self.capture_active = capture_active;
        self.assigned = assigned;
        for (index, identified) in self.identified.iter_mut().enumerate() {
            if !matches!(&preferences.players[index], GamepadAssignment::Reserved(fp)
                if identified.and_then(|id| self.devices.get(&id)).is_some_and(|device| device.fingerprint == *fp))
            {
                *identified = None;
            }
        }
        if self.preferences != *preferences {
            self.preferences = preferences.clone();
        }
        if self.bindings.as_ref() != Some(bindings) {
            self.bindings = Some(bindings.clone());
        }
        let mut effective = EffectiveGamepads::default();
        for (index, id) in assigned.iter().enumerate() {
            let Some(device) = id.and_then(|id| self.devices.get(&id)) else {
                continue;
            };
            if device.waiting_for_neutral {
                continue;
            }
            let player = u8::try_from(index + 1).unwrap_or(1);
            effective.players[index].stick = device.stick;
            for name in &device.buttons {
                if let Some(button) = bindings.map_button_name_for_player(name, player) {
                    effective.players[index].buttons |= button.host_mask_bit();
                }
                if index == 0 {
                    if let Some(action) = bindings.map_action_button_name(name) {
                        effective.actions |= action_bit(action);
                    }
                    if let Some(button) = bindings.map_ws_button_name(name) {
                        effective.ws |= ws_bit(button);
                    }
                }
            }
        }
        self.snapshot = GamepadSnapshot {
            devices: self
                .devices
                .iter()
                .map(|(&id, device)| GamepadDeviceSnapshot {
                    id,
                    fingerprint: device.fingerprint.clone(),
                    buttons: device
                        .buttons
                        .iter()
                        .map(|name| (*name).to_owned())
                        .collect(),
                    left_stick: device.stick,
                    right_stick: device.right_stick,
                    waiting_for_neutral: device.waiting_for_neutral,
                })
                .collect(),
            players: std::array::from_fn(|index| GamepadPlayerSnapshot {
                status: status[index],
                device: assigned[index],
                buttons: effective.players[index].buttons,
            }),
            capture_active,
            capture_ready: self.capture_ready,
        };
        effective
    }
}

fn normalize_axis(value: f32) -> f32 {
    if value.is_finite() {
        value.clamp(-1.0, 1.0)
    } else {
        0.0
    }
}

pub(super) fn action_bit(action: crate::settings::GamepadAction) -> u8 {
    use crate::settings::GamepadAction;
    1 << match action {
        GamepadAction::SpeedUp => 0,
        GamepadAction::Rewind => 1,
        GamepadAction::Pause => 2,
        GamepadAction::Turbo => 3,
    }
}

pub(super) fn ws_bit(button: crate::settings::WonderSwanButton) -> u16 {
    crate::settings::WonderSwanButton::ALL
        .iter()
        .position(|&item| item == button)
        .map_or(0, |index| 1 << index)
}

#[cfg(test)]
mod tests;
