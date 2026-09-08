use std::collections::{BTreeMap, BTreeSet};

use crate::settings::{
    AxisDirection, BindingAction, BindingExpression, BindingExpressionKind, BindingSet,
    BindingTarget, GamepadAssignment, GamepadBindings, GamepadFingerprint, GameplayBindingSource,
    InputAxis, InputDeviceSettings, ResolvedGameplayInput,
};

/// Monotonically allocated per connection; deliberately not serializable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct RuntimeGamepadId(pub(crate) u64);

#[derive(Debug, Clone)]
pub(crate) enum GamepadCommand {
    Neutralize,
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
    /// Raw normalized backend values, retained for diagnostics and calibration.
    pub(crate) left_stick: (f32, f32),
    pub(crate) right_stick: (f32, f32),
    /// Model calibration is applied before the gameplay transform.
    pub(crate) calibrated_left_stick: (f32, f32),
    pub(crate) calibrated_right_stick: (f32, f32),
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
    pub(crate) wonderswan_buttons: u16,
    pub(crate) capture_active: bool,
    pub(crate) capture_ready: bool,
    pub(crate) sample_generation: u64,
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
    calibration_waiting_for_neutral: bool,
    actions_waiting_for_neutral: bool,
}

#[derive(Debug, Clone, PartialEq)]
struct TypedGamepadBinding {
    target: BindingTarget,
    bindings: BindingSet,
}

type AxisLatchKey = (RuntimeGamepadId, usize, u64, InputAxis, AxisDirection);

#[derive(Default)]
pub(super) struct GamepadRouter {
    devices: BTreeMap<RuntimeGamepadId, DeviceState>,
    assigned: [Option<RuntimeGamepadId>; 5],
    identified: [Option<RuntimeGamepadId>; 5],
    require_identification: [bool; 5],
    preferences: InputDeviceSettings,
    bindings: Option<GamepadBindings>,
    legacy_bindings: Option<GamepadBindings>,
    typed_bindings: Vec<TypedGamepadBinding>,
    typed_configuration_changed: bool,
    axis_latches: BTreeSet<AxisLatchKey>,
    snapshot: GamepadSnapshot,
    capture_active: bool,
    capture_ready: bool,
    neutral_threshold: f32,
    pending_pause_presses: Vec<RuntimeGamepadId>,
    sample_generation: u64,
}

impl GamepadRouter {
    pub(super) fn configure_typed(&mut self, resolved: &ResolvedGameplayInput) {
        let incoming = || {
            resolved
                .typed_binding_sets()
                .filter_map(|(target, source, bindings)| {
                    (source == GameplayBindingSource::Gamepad).then_some((target, bindings))
                })
        };
        if self
            .typed_bindings
            .iter()
            .map(|binding| (binding.target, &binding.bindings))
            .eq(incoming())
        {
            return;
        }
        self.typed_bindings = incoming()
            .map(|(target, bindings)| TypedGamepadBinding {
                target,
                bindings: bindings.clone(),
            })
            .collect();
        self.typed_configuration_changed = true;
        self.axis_latches.clear();
        self.capture_ready = false;
    }

    pub(super) fn connect(&mut self, id: RuntimeGamepadId, fingerprint: GamepadFingerprint) {
        self.devices.entry(id).or_insert(DeviceState {
            fingerprint,
            buttons: BTreeSet::new(),
            stick: (0.0, 0.0),
            right_stick: (0.0, 0.0),
            waiting_for_neutral: self.capture_active,
            calibration_waiting_for_neutral: false,
            actions_waiting_for_neutral: false,
        });
    }

    pub(super) fn disconnect(&mut self, id: RuntimeGamepadId) {
        self.devices.remove(&id);
        self.axis_latches.retain(|key| key.0 != id);
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
            && !device.actions_waiting_for_neutral
            && self.assigned[0] == Some(id)
            && self.bindings.as_ref().is_some_and(|bindings| {
                bindings.map_action_button_name(name) == Some(crate::settings::GamepadAction::Pause)
            })
        {
            self.pending_pause_presses.push(id);
        }
        let preferences = self.preferences.clone();
        self.arm_capture_if_neutral(&preferences);
        new_press
    }

    pub(super) fn stick(&mut self, id: RuntimeGamepadId, stick: (f32, f32)) {
        if let Some(device) = self.devices.get_mut(&id) {
            device.stick = (normalize_axis(stick.0), normalize_axis(stick.1));
        }
    }

    pub(super) fn apply_command(&mut self, command: GamepadCommand) {
        match command {
            GamepadCommand::Neutralize => {
                self.pending_pause_presses.clear();
                self.axis_latches.clear();
                self.capture_ready = false;
                for device in self.devices.values_mut() {
                    device.waiting_for_neutral = true;
                }
            }
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

    fn arm_capture_if_neutral(&mut self, preferences: &InputDeviceSettings) {
        if self.capture_active
            && !self.capture_ready
            && self.devices.iter().all(|(id, device)| {
                device_is_neutral(
                    preferences,
                    device,
                    self.neutral_threshold,
                    assigned_player(&self.assigned, *id),
                    &self.typed_bindings,
                )
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
                    && self.devices.get(&id).is_some_and(|device| {
                        !device.waiting_for_neutral && !device.actions_waiting_for_neutral
                    })
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
        self.sample_generation = self.sample_generation.saturating_add(1);
        self.neutral_threshold = if deadzone.is_finite() {
            deadzone.clamp(0.01, 1.0)
        } else {
            0.25
        };
        if capture_active != self.capture_active {
            self.capture_ready = false;
        }
        self.capture_active = capture_active;
        self.arm_capture_if_neutral(preferences);
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
        let bindings_changed = self
            .bindings
            .as_ref()
            .is_some_and(|old| !old.gameplay_eq(bindings));
        let typed_changed = std::mem::take(&mut self.typed_configuration_changed);
        let actions_changed = self
            .bindings
            .as_ref()
            .is_some_and(|old| !old.actions_eq(bindings));
        if bindings_changed || typed_changed || actions_changed {
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
                    self.axis_latches.retain(|key| key.0 != id);
                    if let Some(device) = self.devices.get_mut(&id) {
                        device.waiting_for_neutral = true;
                    }
                }
            }
        }
        let threshold = self.neutral_threshold;
        for (id, device) in &mut self.devices {
            let calibration_changed =
                calibration_for(&self.preferences, device) != calibration_for(preferences, device);
            if calibration_changed {
                device.waiting_for_neutral = true;
                device.calibration_waiting_for_neutral = true;
            }
            if actions_changed {
                device.actions_waiting_for_neutral = true;
            } else if device.buttons.is_empty() {
                device.actions_waiting_for_neutral = false;
            }
            let neutral = device_is_neutral(
                preferences,
                device,
                threshold,
                assigned_player(&assigned, *id),
                &self.typed_bindings,
            );
            if capture_active || bindings_changed || typed_changed {
                device.waiting_for_neutral = true;
            } else if device.calibration_waiting_for_neutral {
                if neutral {
                    device.calibration_waiting_for_neutral = false;
                }
            } else if neutral {
                device.waiting_for_neutral = false;
            }
            if device.waiting_for_neutral {
                self.axis_latches.retain(|key| key.0 != *id);
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
        if self.bindings.as_ref() != Some(bindings) || typed_changed {
            let mut legacy = bindings.clone();
            for binding in &self.typed_bindings {
                match binding.target {
                    BindingTarget::Joypad { player, action } => {
                        legacy.set_for_player(action, player, "")
                    }
                    BindingTarget::WonderSwan(action) => legacy.set_ws(action, ""),
                    BindingTarget::Tilt(_) => {}
                }
            }
            self.legacy_bindings = Some(legacy);
            self.bindings = Some(bindings.clone());
        }
        let legacy = self.legacy_bindings.as_ref().unwrap_or(bindings);
        let mut effective = EffectiveGamepads::default();
        for (index, id) in assigned.iter().enumerate() {
            let Some(device) = id.and_then(|id| self.devices.get(&id)) else {
                continue;
            };
            if device.waiting_for_neutral {
                continue;
            }
            let player = u8::try_from(index + 1).unwrap_or(1);
            effective.players[index].stick = calibrated_left_stick(preferences, device);
            for name in &device.buttons {
                if let Some(button) = legacy.map_button_name_for_player(name, player) {
                    effective.players[index].buttons |= button.host_mask_bit();
                }
                if index == 0 {
                    if !device.actions_waiting_for_neutral
                        && let Some(action) = bindings.map_action_button_name(name)
                    {
                        effective.actions |= action_bit(action);
                    }
                    if let Some(button) = legacy.map_ws_button_name(name) {
                        effective.ws |= ws_bit(button);
                    }
                }
            }
            let left = calibrated_left_stick(preferences, device);
            let right = calibrated_right_stick(preferences, device);
            for (binding_index, binding) in self.typed_bindings.iter().enumerate() {
                if !target_matches_player(binding.target, player) {
                    continue;
                }
                let active = binding.bindings.evaluate(|alternative, atom| match atom {
                    BindingExpressionKind::GamepadButton(button) => {
                        device.buttons.contains(button.as_str())
                    }
                    BindingExpressionKind::Axis(axis) => {
                        let key = (
                            id.expect("assigned device"),
                            binding_index,
                            alternative,
                            axis.axis,
                            axis.direction,
                        );
                        let active = axis.evaluate(
                            axis.axis.value(left, right),
                            self.axis_latches.contains(&key),
                        );
                        if active {
                            self.axis_latches.insert(key);
                        } else {
                            self.axis_latches.remove(&key);
                        }
                        active
                    }
                    _ => false,
                });
                if active {
                    match binding.target {
                        BindingTarget::Joypad { action, .. } => {
                            effective.players[index].buttons |= host_button(action).host_mask_bit()
                        }
                        BindingTarget::WonderSwan(action) => effective.ws |= ws_bit(action),
                        BindingTarget::Tilt(_) => {}
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
                    calibrated_left_stick: calibrated_left_stick(preferences, device),
                    calibrated_right_stick: calibrated_right_stick(preferences, device),
                    waiting_for_neutral: device.waiting_for_neutral,
                })
                .collect(),
            players: std::array::from_fn(|index| GamepadPlayerSnapshot {
                status: status[index],
                device: assigned[index],
                buttons: effective.players[index].buttons,
            }),
            wonderswan_buttons: effective.ws,
            capture_active,
            capture_ready: self.capture_ready,
            sample_generation: self.sample_generation,
        };
        effective
    }
}

fn assigned_player(assigned: &[Option<RuntimeGamepadId>; 5], id: RuntimeGamepadId) -> Option<u8> {
    assigned
        .iter()
        .position(|selected| *selected == Some(id))
        .map(|index| index as u8 + 1)
}

fn target_matches_player(target: BindingTarget, player: u8) -> bool {
    match target {
        BindingTarget::Joypad { player: target, .. } => target == player,
        BindingTarget::WonderSwan(_) => player == 1,
        BindingTarget::Tilt(_) => false,
    }
}

fn expression_uses_right_stick(expression: &BindingExpression) -> bool {
    if !expression.is_supported() {
        return false;
    }
    match &expression.kind {
        BindingExpressionKind::Axis(axis) => {
            matches!(axis.axis, InputAxis::RightX | InputAxis::RightY)
        }
        BindingExpressionKind::Chord(atoms) => atoms.iter().any(expression_uses_right_stick),
        _ => false,
    }
}

fn expression_axes_are_neutral(
    expression: &BindingExpression,
    left: (f32, f32),
    right: (f32, f32),
) -> bool {
    if !expression.is_supported() {
        return true;
    }
    match &expression.kind {
        BindingExpressionKind::Axis(axis) => !axis.evaluate(axis.axis.value(left, right), true),
        BindingExpressionKind::Chord(atoms) => atoms
            .iter()
            .all(|atom| expression_axes_are_neutral(atom, left, right)),
        _ => true,
    }
}

fn device_is_neutral(
    preferences: &InputDeviceSettings,
    device: &DeviceState,
    threshold: f32,
    player: Option<u8>,
    typed_bindings: &[TypedGamepadBinding],
) -> bool {
    if !device.buttons.is_empty() {
        return false;
    }
    let left = calibrated_left_stick(preferences, device);
    if !left_stick_is_neutral(left, threshold) {
        return false;
    }
    let right = calibrated_right_stick(preferences, device);
    for binding in typed_bindings.iter().filter(|binding| {
        player.is_some_and(|player| target_matches_player(binding.target, player))
    }) {
        for alternative in &binding.bindings.alternatives {
            if expression_uses_right_stick(&alternative.expression)
                && !left_stick_is_neutral(right, threshold)
            {
                return false;
            }
            if !expression_axes_are_neutral(&alternative.expression, left, right) {
                return false;
            }
        }
    }
    true
}

fn host_button(action: BindingAction) -> super::HostButton {
    use super::HostButton;
    match action {
        BindingAction::Right => HostButton::Right,
        BindingAction::Left => HostButton::Left,
        BindingAction::Up => HostButton::Up,
        BindingAction::Down => HostButton::Down,
        BindingAction::A => HostButton::A,
        BindingAction::B => HostButton::B,
        BindingAction::X => HostButton::X,
        BindingAction::Y => HostButton::Y,
        BindingAction::L => HostButton::L,
        BindingAction::R => HostButton::R,
        BindingAction::Start => HostButton::Start,
        BindingAction::Select => HostButton::Select,
    }
}

fn calibration_for(
    preferences: &InputDeviceSettings,
    device: &DeviceState,
) -> crate::settings::GamepadCalibration {
    preferences
        .calibration_for(&device.fingerprint)
        .filter(|calibration| calibration.is_valid())
        .unwrap_or_default()
}

fn calibrated_left_stick(preferences: &InputDeviceSettings, device: &DeviceState) -> (f32, f32) {
    calibration_for(preferences, device).apply_left(device.stick)
}

fn calibrated_right_stick(preferences: &InputDeviceSettings, device: &DeviceState) -> (f32, f32) {
    calibration_for(preferences, device).apply_right(device.right_stick)
}

fn left_stick_is_neutral(stick: (f32, f32), threshold: f32) -> bool {
    stick.0.abs() < threshold && stick.1.abs() < threshold
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
