use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use winit::keyboard::KeyCode;

use super::{
    AutofireOverride, AutofirePattern, AutofireTarget, BindingAction, BindingSet,
    BindingSetOverride, GamepadAction, GamepadBindings, InputProfileBindings, LeftStickMode,
    ResolvedAutofire, Settings, TiltBindingAction, TiltInputMode, TiltSettings, WonderSwanButton,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum InputSystem {
    GameBoy,
    GameBoyAdvance,
    Nes,
    MasterSystem,
    GameGear,
    Sg1000,
    Pce,
    WonderSwan,
    Coleco,
}

impl InputSystem {
    pub(crate) const ALL: [Self; 9] = [
        Self::GameBoy,
        Self::GameBoyAdvance,
        Self::Nes,
        Self::MasterSystem,
        Self::GameGear,
        Self::Sg1000,
        Self::Pce,
        Self::WonderSwan,
        Self::Coleco,
    ];

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::GameBoy => "Game Boy / Game Boy Color",
            Self::GameBoyAdvance => "Game Boy Advance",
            Self::Nes => "NES / Famicom Disk System",
            Self::MasterSystem => "Master System",
            Self::GameGear => "Game Gear",
            Self::Sg1000 => "SG-1000",
            Self::Pce => "PC Engine",
            Self::WonderSwan => "WonderSwan",
            Self::Coleco => "ColecoVision",
        }
    }

    fn code(self) -> &'static str {
        match self {
            Self::GameBoy => "gb",
            Self::GameBoyAdvance => "gba",
            Self::Nes => "nes",
            Self::MasterSystem => "sms",
            Self::GameGear => "gg",
            Self::Sg1000 => "sg1000",
            Self::Pce => "pce",
            Self::WonderSwan => "ws",
            Self::Coleco => "coleco",
        }
    }
}

impl From<crate::emu_backend::ActiveSystem> for InputSystem {
    fn from(system: crate::emu_backend::ActiveSystem) -> Self {
        use crate::emu_backend::ActiveSystem;
        match system {
            ActiveSystem::GameBoy => Self::GameBoy,
            ActiveSystem::GameBoyAdvance => Self::GameBoyAdvance,
            ActiveSystem::Nes => Self::Nes,
            ActiveSystem::MasterSystem => Self::MasterSystem,
            ActiveSystem::GameGear => Self::GameGear,
            ActiveSystem::Sg1000 => Self::Sg1000,
            ActiveSystem::Pce => Self::Pce,
            ActiveSystem::WonderSwan => Self::WonderSwan,
            ActiveSystem::Coleco => Self::Coleco,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct InputGameKey {
    pub(crate) system: InputSystem,
    pub(crate) digest: [u8; 32],
}

impl InputGameKey {
    pub(crate) fn new(system: InputSystem, digest: [u8; 32]) -> Self {
        Self { system, digest }
    }

    pub(crate) fn storage_key(&self) -> String {
        let digest: String = self
            .digest
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        format!(
            "effective-content-sha256-v1:{}:{digest}",
            self.system.code()
        )
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) enum InputScope {
    #[default]
    Global,
    System(InputSystem),
    Game(InputGameKey),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BindingTarget {
    Joypad { player: u8, action: BindingAction },
    WonderSwan(WonderSwanButton),
    Tilt(TiltBindingAction),
}

impl BindingTarget {
    pub(crate) fn key(self) -> String {
        match self {
            Self::Joypad { player, action } => format!("p{player}.{action:?}").to_ascii_lowercase(),
            Self::WonderSwan(action) => format!("ws.{action:?}").to_ascii_lowercase(),
            Self::Tilt(action) => format!("tilt.{action:?}").to_ascii_lowercase(),
        }
    }

    pub(crate) fn all() -> impl Iterator<Item = Self> {
        (1..=5)
            .flat_map(|player| {
                BindingAction::ALL
                    .iter()
                    .copied()
                    .map(move |action| Self::Joypad { player, action })
            })
            .chain(WonderSwanButton::ALL.iter().copied().map(Self::WonderSwan))
            .chain(
                [
                    TiltBindingAction::Up,
                    TiltBindingAction::Down,
                    TiltBindingAction::Left,
                    TiltBindingAction::Right,
                ]
                .into_iter()
                .map(Self::Tilt),
            )
    }

    pub(crate) fn valid(self, source: GameplayBindingSource) -> bool {
        match self {
            Self::Joypad { player, .. } => (1..=5).contains(&player),
            Self::WonderSwan(_) => true,
            Self::Tilt(_) => source == GameplayBindingSource::Keyboard,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GameplayBindingSource {
    Keyboard,
    Gamepad,
}

impl GameplayBindingSource {
    pub(crate) fn key(self, target: BindingTarget) -> String {
        format!(
            "{}/{}",
            match self {
                Self::Keyboard => "keyboard",
                Self::Gamepad => "gamepad",
            },
            target.key()
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PhysicalBinding {
    Keyboard(KeyCode),
    Gamepad(String),
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
enum StoredPhysicalBinding {
    Keyboard(String),
    Gamepad(String),
}

impl Serialize for PhysicalBinding {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let stored = match self {
            Self::Keyboard(key) => {
                StoredPhysicalBinding::Keyboard(super::keycode_serde::keycode_to_string(*key))
            }
            Self::Gamepad(button) => StoredPhysicalBinding::Gamepad(button.clone()),
        };
        stored.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for PhysicalBinding {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        match StoredPhysicalBinding::deserialize(deserializer)? {
            StoredPhysicalBinding::Keyboard(value) => super::keycode_from_string(&value)
                .map(Self::Keyboard)
                .ok_or_else(|| serde::de::Error::custom(format!("Unknown physical key: {value}"))),
            StoredPhysicalBinding::Gamepad(value) => Ok(Self::Gamepad(value)),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "state", content = "value", rename_all = "snake_case")]
enum BindingOverride {
    Set(PhysicalBinding),
    Unbound,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GameplayTransform {
    InputMode,
    LeftStickMode,
    InvertX,
    InvertY,
    StickBypassLerp,
    Sensitivity,
    Smoothing,
    Deadzone,
}

impl GameplayTransform {
    pub(crate) const ALL: [Self; 8] = [
        Self::InputMode,
        Self::LeftStickMode,
        Self::InvertX,
        Self::InvertY,
        Self::StickBypassLerp,
        Self::Sensitivity,
        Self::Smoothing,
        Self::Deadzone,
    ];

    fn key(self) -> &'static str {
        match self {
            Self::InputMode => "input_mode",
            Self::LeftStickMode => "left_stick_mode",
            Self::InvertX => "invert_x",
            Self::InvertY => "invert_y",
            Self::StickBypassLerp => "stick_bypass_lerp",
            Self::Sensitivity => "sensitivity",
            Self::Smoothing => "smoothing",
            Self::Deadzone => "deadzone",
        }
    }

    fn get(self, tilt: &TiltSettings) -> TransformValue {
        match self {
            Self::InputMode => TransformValue::InputMode(tilt.input_mode),
            Self::LeftStickMode => TransformValue::LeftStickMode(tilt.left_stick_mode),
            Self::InvertX => TransformValue::Bool(tilt.invert_x),
            Self::InvertY => TransformValue::Bool(tilt.invert_y),
            Self::StickBypassLerp => TransformValue::Bool(tilt.stick_bypass_lerp),
            Self::Sensitivity => TransformValue::Float(tilt.sensitivity),
            Self::Smoothing => TransformValue::Float(tilt.lerp),
            Self::Deadzone => TransformValue::Float(tilt.deadzone),
        }
    }

    fn set(self, tilt: &mut TiltSettings, value: &TransformValue) -> Result<(), String> {
        match (self, value) {
            (Self::InputMode, TransformValue::InputMode(value)) => tilt.input_mode = *value,
            (Self::LeftStickMode, TransformValue::LeftStickMode(value)) => {
                tilt.left_stick_mode = *value
            }
            (Self::InvertX, TransformValue::Bool(value)) => tilt.invert_x = *value,
            (Self::InvertY, TransformValue::Bool(value)) => tilt.invert_y = *value,
            (Self::StickBypassLerp, TransformValue::Bool(value)) => tilt.stick_bypass_lerp = *value,
            (Self::Sensitivity, TransformValue::Float(value))
                if value.is_finite() && (0.0..=16.0).contains(value) =>
            {
                tilt.sensitivity = *value
            }
            (Self::Smoothing, TransformValue::Float(value))
                if value.is_finite() && (0.0..=1.0).contains(value) =>
            {
                tilt.lerp = *value
            }
            (Self::Deadzone, TransformValue::Float(value))
                if value.is_finite() && (0.0..=1.0).contains(value) =>
            {
                tilt.deadzone = *value
            }
            _ => return Err(format!("Invalid value for {}", self.key())),
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub(crate) enum TransformValue {
    Bool(bool),
    Float(f32),
    InputMode(TiltInputMode),
    LeftStickMode(LeftStickMode),
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ResolvedBinding {
    pub(crate) value: Option<PhysicalBinding>,
    pub(crate) origin: InputScope,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ResolvedBindingSet {
    pub(crate) value: Option<BindingSet>,
    pub(crate) origin: InputScope,
    pub(crate) typed: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ResolvedTransform {
    pub(crate) value: TransformValue,
    pub(crate) origin: InputScope,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub(super) struct InputPatch {
    bindings: BTreeMap<String, BindingOverride>,
    binding_sets: BTreeMap<String, BindingSetOverride>,
    transforms: BTreeMap<String, TransformValue>,
    autofire: BTreeMap<String, AutofireOverride>,
    #[serde(flatten)]
    unknown: BTreeMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct InputOverrides {
    pub(super) global_keyboard_unbound: BTreeSet<String>,
    pub(super) global_binding_sets: BTreeMap<String, BindingSetOverride>,
    pub(super) global_autofire: BTreeMap<String, AutofireOverride>,
    systems: BTreeMap<String, InputPatch>,
    games: BTreeMap<String, InputPatch>,
    #[serde(flatten)]
    unknown: BTreeMap<String, serde_json::Value>,
}

impl InputOverrides {
    pub(crate) fn matches_runtime_scope(&self, other: &Self, scope: &InputScope) -> bool {
        self.global_keyboard_unbound == other.global_keyboard_unbound
            && self.global_binding_sets == other.global_binding_sets
            && self.global_autofire == other.global_autofire
            && self.patch(scope) == other.patch(scope)
            && match scope {
                InputScope::Game(game) => {
                    self.patch(&InputScope::System(game.system))
                        == other.patch(&InputScope::System(game.system))
                }
                _ => true,
            }
    }

    fn patch(&self, scope: &InputScope) -> Option<&InputPatch> {
        match scope {
            InputScope::Global => None,
            InputScope::System(system) => self.systems.get(system.code()),
            InputScope::Game(game) => self.games.get(&game.storage_key()),
        }
    }
    fn patch_mut(&mut self, scope: &InputScope) -> Option<&mut InputPatch> {
        match scope {
            InputScope::Global => None,
            InputScope::System(system) => {
                Some(self.systems.entry(system.code().into()).or_default())
            }
            InputScope::Game(game) => Some(self.games.entry(game.storage_key()).or_default()),
        }
    }

    fn binding_sets(&self, scope: &InputScope) -> Option<&BTreeMap<String, BindingSetOverride>> {
        match scope {
            InputScope::Global => Some(&self.global_binding_sets),
            _ => self.patch(scope).map(|patch| &patch.binding_sets),
        }
    }

    fn binding_sets_mut(
        &mut self,
        scope: &InputScope,
    ) -> &mut BTreeMap<String, BindingSetOverride> {
        match scope {
            InputScope::Global => &mut self.global_binding_sets,
            _ => {
                &mut self
                    .patch_mut(scope)
                    .expect("non-global input scope")
                    .binding_sets
            }
        }
    }

    fn autofire(&self, scope: &InputScope) -> Option<&BTreeMap<String, AutofireOverride>> {
        match scope {
            InputScope::Global => Some(&self.global_autofire),
            _ => self.patch(scope).map(|patch| &patch.autofire),
        }
    }

    fn autofire_mut(&mut self, scope: &InputScope) -> &mut BTreeMap<String, AutofireOverride> {
        match scope {
            InputScope::Global => &mut self.global_autofire,
            _ => {
                &mut self
                    .patch_mut(scope)
                    .expect("non-global input scope")
                    .autofire
            }
        }
    }

    pub(super) fn validate(&self) -> Result<(), String> {
        for key in &self.global_keyboard_unbound {
            if !BindingTarget::all().any(|target| target.key() == *key) {
                return Err(format!("Unknown global keyboard target: {key}"));
            }
        }
        for patch in self.systems.values().chain(self.games.values()) {
            for (key, value) in &patch.bindings {
                if let Some((target, source)) = BindingTarget::all().find_map(|target| {
                    [
                        GameplayBindingSource::Keyboard,
                        GameplayBindingSource::Gamepad,
                    ]
                    .into_iter()
                    .find(|source| source.key(target) == *key)
                    .map(|source| (target, source))
                }) {
                    if !target.valid(source) {
                        return Err(format!("Invalid binding target: {key}"));
                    }
                    if let BindingOverride::Set(value) = value {
                        validate_binding(source, value)?;
                    }
                }
            }
            for field in GameplayTransform::ALL {
                if let Some(value) = patch.transforms.get(field.key()) {
                    field.set(&mut TiltSettings::default(), value)?;
                }
            }
        }
        for bindings in std::iter::once(&self.global_binding_sets).chain(
            self.systems
                .values()
                .chain(self.games.values())
                .map(|patch| &patch.binding_sets),
        ) {
            for (key, value) in bindings {
                if let Some((target, source)) = binding_target_and_source(key) {
                    if !target.valid(source) {
                        return Err(format!("Invalid typed binding target: {key}"));
                    }
                    value.validate(source)?;
                }
            }
        }
        for autofire in std::iter::once(&self.global_autofire).chain(
            self.systems
                .values()
                .chain(self.games.values())
                .map(|patch| &patch.autofire),
        ) {
            for (key, value) in autofire {
                if AutofireTarget::from_key(key).is_none() {
                    continue;
                }
                if let Some(pattern) = value.value()
                    && !pattern.is_valid()
                {
                    return Err(format!("Invalid autofire pattern: {key}"));
                }
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ResolvedGameplayInput {
    keyboard: BTreeMap<String, KeyCode>,
    binding_sets: BTreeMap<String, BindingSet>,
    typed_bindings: BTreeSet<String>,
    pub(crate) gamepad: GamepadBindings,
    pub(crate) tilt: TiltSettings,
}

impl ResolvedGameplayInput {
    pub(crate) fn keyboard_binding(&self, target: BindingTarget) -> Option<KeyCode> {
        self.keyboard.get(&target.key()).copied()
    }

    #[cfg(test)]
    pub(crate) fn binding_set(
        &self,
        target: BindingTarget,
        source: GameplayBindingSource,
    ) -> Option<&BindingSet> {
        self.binding_sets.get(&source.key(target))
    }

    pub(crate) fn has_typed_binding(
        &self,
        target: BindingTarget,
        source: GameplayBindingSource,
    ) -> bool {
        self.typed_bindings.contains(&source.key(target))
    }

    pub(crate) fn typed_binding_sets(
        &self,
    ) -> impl Iterator<Item = (BindingTarget, GameplayBindingSource, &BindingSet)> {
        self.typed_bindings.iter().filter_map(|key| {
            let (target, source) = binding_target_and_source(key)?;
            Some((target, source, self.binding_sets.get(key)?))
        })
    }
}

fn binding_target_and_source(key: &str) -> Option<(BindingTarget, GameplayBindingSource)> {
    BindingTarget::all().find_map(|target| {
        [
            GameplayBindingSource::Keyboard,
            GameplayBindingSource::Gamepad,
        ]
        .into_iter()
        .find(|source| source.key(target) == key)
        .map(|source| (target, source))
    })
}

fn validate_binding(source: GameplayBindingSource, value: &PhysicalBinding) -> Result<(), String> {
    match (source, value) {
        (GameplayBindingSource::Keyboard, PhysicalBinding::Keyboard(key)) => {
            let name = super::keycode_serde::keycode_to_string(*key);
            if super::keycode_from_string(&name).is_none() {
                return Err("This physical key is not supported.".into());
            }
        }
        (GameplayBindingSource::Gamepad, PhysicalBinding::Gamepad(name))
            if !name.is_empty() && name.len() <= 64 && !name.chars().any(char::is_control) => {}
        _ => return Err("Binding source and value do not match.".into()),
    }
    Ok(())
}

impl Settings {
    fn global_binding(
        &self,
        target: BindingTarget,
        source: GameplayBindingSource,
    ) -> Option<PhysicalBinding> {
        if !target.valid(source) {
            return None;
        }
        match source {
            GameplayBindingSource::Keyboard => {
                if self
                    .input_overrides
                    .global_keyboard_unbound
                    .contains(&target.key())
                {
                    return None;
                }
                let key = match target {
                    BindingTarget::Joypad { player: 1, action } => {
                        Some(self.key_bindings.get(action))
                    }
                    BindingTarget::Joypad { player: 2, action } => {
                        Some(self.key_bindings_p2.get(action))
                    }
                    BindingTarget::Joypad { player, action } => self
                        .pce_multitap_key_bindings
                        .get(usize::from(player - 3))
                        .and_then(|keys| keys.get(action)),
                    BindingTarget::WonderSwan(action) => Some(self.ws_key_bindings.get(action)),
                    BindingTarget::Tilt(action) => Some(self.tilt.key_bindings.get(action)),
                };
                key.map(PhysicalBinding::Keyboard)
            }
            GameplayBindingSource::Gamepad => {
                let button = match target {
                    BindingTarget::Joypad { player, action } => {
                        self.gamepad_bindings.get_for_player(action, player)
                    }
                    BindingTarget::WonderSwan(action) => self.gamepad_bindings.get_ws(action),
                    BindingTarget::Tilt(_) => "",
                };
                (!button.is_empty()).then(|| PhysicalBinding::Gamepad(button.to_owned()))
            }
        }
    }

    pub(crate) fn binding(
        &self,
        scope: &InputScope,
        target: BindingTarget,
        source: GameplayBindingSource,
    ) -> ResolvedBinding {
        let resolved = self.binding_set(scope, target, source);
        ResolvedBinding {
            value: resolved
                .value
                .as_ref()
                .and_then(BindingSet::as_single_physical_binding),
            origin: resolved.origin,
        }
    }

    pub(crate) fn binding_set(
        &self,
        scope: &InputScope,
        target: BindingTarget,
        source: GameplayBindingSource,
    ) -> ResolvedBindingSet {
        let (record, origin, typed) = self.resolved_binding_record(scope, target, source);
        ResolvedBindingSet {
            value: record.value,
            origin,
            typed,
        }
    }

    fn resolved_binding_record(
        &self,
        scope: &InputScope,
        target: BindingTarget,
        source: GameplayBindingSource,
    ) -> (BindingSetOverride, InputScope, bool) {
        let key = source.key(target);
        if let Some(value) = self
            .input_overrides
            .binding_sets(scope)
            .and_then(|bindings| bindings.get(&key))
        {
            return (value.clone(), scope.clone(), true);
        }
        if let Some(value) = self
            .input_overrides
            .patch(scope)
            .and_then(|patch| patch.bindings.get(&key))
        {
            let value = match value {
                BindingOverride::Set(value) => {
                    Some(BindingSet::from_physical_binding(value.clone()))
                }
                BindingOverride::Unbound => None,
            };
            return (BindingSetOverride::new(value), scope.clone(), false);
        }
        if let InputScope::Game(game) = scope {
            return self.resolved_binding_record(&InputScope::System(game.system), target, source);
        }
        if scope != &InputScope::Global {
            return self.resolved_binding_record(&InputScope::Global, target, source);
        }
        (
            BindingSetOverride::new(
                self.global_binding(target, source)
                    .map(BindingSet::from_physical_binding),
            ),
            InputScope::Global,
            false,
        )
    }

    fn resolved_typed_binding_sets(
        &self,
        scope: &InputScope,
    ) -> BTreeMap<String, BindingSetOverride> {
        let mut bindings = match scope {
            InputScope::Global => return self.input_overrides.global_binding_sets.clone(),
            InputScope::System(_) => self.input_overrides.global_binding_sets.clone(),
            InputScope::Game(game) => {
                self.resolved_typed_binding_sets(&InputScope::System(game.system))
            }
        };
        if let Some(patch) = self.input_overrides.patch(scope) {
            for key in patch.bindings.keys() {
                bindings.remove(key);
            }
            bindings.extend(patch.binding_sets.clone());
        }
        bindings
    }

    pub(crate) fn set_binding_set(
        &mut self,
        scope: &InputScope,
        target: BindingTarget,
        source: GameplayBindingSource,
        value: Option<BindingSet>,
    ) -> Result<(), String> {
        if !target.valid(source) {
            return Err("This binding target is unavailable.".into());
        }
        if let Some(value) = &value {
            value.validate(source)?;
        }
        self.input_overrides
            .binding_sets_mut(scope)
            .entry(source.key(target))
            .and_modify(|record| record.replace(value.clone()))
            .or_insert_with(|| BindingSetOverride::new(value));
        Ok(())
    }

    pub(crate) fn set_binding(
        &mut self,
        scope: &InputScope,
        target: BindingTarget,
        source: GameplayBindingSource,
        value: Option<PhysicalBinding>,
    ) -> Result<(), String> {
        if !target.valid(source) {
            return Err("This binding target is unavailable.".into());
        }
        if let Some(value) = &value {
            validate_binding(source, value)?;
        }
        self.input_overrides
            .binding_sets_mut(scope)
            .remove(&source.key(target));
        if let Some(patch) = self.input_overrides.patch_mut(scope) {
            patch.bindings.insert(
                source.key(target),
                value.map_or(BindingOverride::Unbound, BindingOverride::Set),
            );
            return Ok(());
        }
        match source {
            GameplayBindingSource::Keyboard => {
                let Some(PhysicalBinding::Keyboard(key)) = value else {
                    self.input_overrides
                        .global_keyboard_unbound
                        .insert(target.key());
                    return Ok(());
                };
                self.input_overrides
                    .global_keyboard_unbound
                    .remove(&target.key());
                match target {
                    BindingTarget::Joypad { player: 1, action } => {
                        self.key_bindings.set(action, key)
                    }
                    BindingTarget::Joypad { player: 2, action } => {
                        self.key_bindings_p2.set(action, key)
                    }
                    BindingTarget::Joypad { player, action } => {
                        self.pce_multitap_key_bindings[usize::from(player - 3)].set(action, key)
                    }
                    BindingTarget::WonderSwan(action) => self.ws_key_bindings.set(action, key),
                    BindingTarget::Tilt(action) => self.tilt.key_bindings.set(action, key),
                }
            }
            GameplayBindingSource::Gamepad => {
                let button = match value {
                    Some(PhysicalBinding::Gamepad(value)) => value,
                    _ => String::new(),
                };
                match target {
                    BindingTarget::Joypad { player, action } => self
                        .gamepad_bindings
                        .set_for_player(action, player, &button),
                    BindingTarget::WonderSwan(action) => {
                        self.gamepad_bindings.set_ws(action, &button)
                    }
                    BindingTarget::Tilt(_) => {}
                }
            }
        }
        Ok(())
    }

    pub(crate) fn inherit_binding(
        &mut self,
        scope: &InputScope,
        target: BindingTarget,
        source: GameplayBindingSource,
    ) {
        if scope != &InputScope::Global {
            self.input_overrides
                .binding_sets_mut(scope)
                .remove(&source.key(target));
        }
        if let Some(patch) = self.input_overrides.patch_mut(scope) {
            patch.bindings.remove(&source.key(target));
        }
    }

    pub(crate) fn transform(
        &self,
        scope: &InputScope,
        field: GameplayTransform,
    ) -> ResolvedTransform {
        if let Some(value) = self
            .input_overrides
            .patch(scope)
            .and_then(|patch| patch.transforms.get(field.key()))
        {
            return ResolvedTransform {
                value: value.clone(),
                origin: scope.clone(),
            };
        }
        if let InputScope::Game(game) = scope {
            return self.transform(&InputScope::System(game.system), field);
        }
        ResolvedTransform {
            value: field.get(&self.tilt),
            origin: InputScope::Global,
        }
    }

    pub(crate) fn set_transform(
        &mut self,
        scope: &InputScope,
        field: GameplayTransform,
        value: TransformValue,
    ) -> Result<(), String> {
        field.set(&mut TiltSettings::default(), &value)?;
        if let Some(patch) = self.input_overrides.patch_mut(scope) {
            patch.transforms.insert(field.key().into(), value);
        } else {
            field.set(&mut self.tilt, &value)?;
        }
        Ok(())
    }

    pub(crate) fn inherit_transform(&mut self, scope: &InputScope, field: GameplayTransform) {
        if let Some(patch) = self.input_overrides.patch_mut(scope) {
            patch.transforms.remove(field.key());
        }
    }

    pub(crate) fn autofire(&self, scope: &InputScope, target: AutofireTarget) -> ResolvedAutofire {
        if let Some(value) = self
            .input_overrides
            .autofire(scope)
            .and_then(|autofire| autofire.get(&target.key()))
        {
            return ResolvedAutofire {
                value: value.value(),
                origin: scope.clone(),
            };
        }
        if let InputScope::Game(game) = scope {
            return self.autofire(&InputScope::System(game.system), target);
        }
        if scope != &InputScope::Global {
            return self.autofire(&InputScope::Global, target);
        }
        ResolvedAutofire {
            value: None,
            origin: InputScope::Global,
        }
    }

    pub(crate) fn set_autofire(
        &mut self,
        scope: &InputScope,
        target: AutofireTarget,
        value: AutofireOverride,
    ) -> Result<(), String> {
        if target.matrix_index().is_none() {
            return Err("This autofire target is unavailable.".into());
        }
        if let Some(pattern) = value.value()
            && !pattern.is_valid()
        {
            return Err("Autofire period must be 1–60 frames and on-time must fit it.".into());
        }
        let value = self
            .input_overrides
            .autofire(scope)
            .and_then(|autofire| autofire.get(&target.key()))
            .map(|existing| existing.replace_value_preserving_extensions(value.value()))
            .unwrap_or(value);
        self.input_overrides
            .autofire_mut(scope)
            .insert(target.key(), value);
        Ok(())
    }

    pub(crate) fn inherit_autofire(&mut self, scope: &InputScope, target: AutofireTarget) {
        self.input_overrides
            .autofire_mut(scope)
            .remove(&target.key());
    }

    pub(crate) fn reset_input_scope(&mut self, scope: &InputScope) {
        match scope {
            InputScope::Global => {
                let defaults = Self::default().capture_input_profile();
                let _ = self.apply_controller_profile(scope, &defaults);
                self.input_overrides.global_binding_sets.clear();
                self.input_overrides.global_autofire.clear();
            }
            InputScope::System(system) => {
                self.input_overrides.systems.remove(system.code());
            }
            InputScope::Game(game) => {
                self.input_overrides.games.remove(&game.storage_key());
            }
        }
    }

    pub(crate) fn resolve_gameplay_input(&self, scope: &InputScope) -> ResolvedGameplayInput {
        let mut resolved = ResolvedGameplayInput {
            keyboard: BTreeMap::new(),
            binding_sets: BTreeMap::new(),
            typed_bindings: BTreeSet::new(),
            gamepad: self.gamepad_bindings.clone(),
            tilt: self.tilt.clone(),
        };
        for target in BindingTarget::all() {
            for source in [
                GameplayBindingSource::Keyboard,
                GameplayBindingSource::Gamepad,
            ] {
                if target.valid(source) {
                    let binding = self.binding_set(scope, target, source);
                    let key = source.key(target);
                    if binding.typed {
                        resolved.typed_bindings.insert(key.clone());
                    }
                    if let Some(value) = binding.value {
                        resolved.binding_sets.insert(key, value);
                    }
                }
            }
            if let Some(PhysicalBinding::Keyboard(key)) = self
                .binding(scope, target, GameplayBindingSource::Keyboard)
                .value
            {
                resolved.keyboard.insert(target.key(), key);
            }
            if target.valid(GameplayBindingSource::Gamepad) {
                let value = self
                    .binding(scope, target, GameplayBindingSource::Gamepad)
                    .value;
                let button = match value {
                    Some(PhysicalBinding::Gamepad(value)) => value,
                    _ => String::new(),
                };
                match target {
                    BindingTarget::Joypad { player, action } => {
                        resolved.gamepad.set_for_player(action, player, &button)
                    }
                    BindingTarget::WonderSwan(action) => resolved.gamepad.set_ws(action, &button),
                    BindingTarget::Tilt(_) => {}
                }
            }
        }
        for field in GameplayTransform::ALL {
            let _ = field.set(&mut resolved.tilt, &self.transform(scope, field).value);
        }
        resolved
    }

    pub(crate) fn resolve_autofire(&self, scope: &InputScope) -> [[Option<AutofirePattern>; 8]; 5] {
        let mut resolved = [[None; 8]; 5];
        for target in AutofireTarget::all() {
            let Some((player, action)) = target.matrix_index() else {
                continue;
            };
            resolved[player][action] = self.autofire(scope, target).value;
        }
        resolved
    }

    pub(crate) fn capture_controller_profile(&self, scope: &InputScope) -> InputProfileBindings {
        let mut adapter = Self::default();
        adapter.apply_input_profile(&self.capture_input_profile());
        for target in BindingTarget::all() {
            for source in [
                GameplayBindingSource::Keyboard,
                GameplayBindingSource::Gamepad,
            ] {
                if target.valid(source) {
                    let _ = adapter.set_binding(
                        &InputScope::Global,
                        target,
                        source,
                        self.binding(scope, target, source).value,
                    );
                }
            }
        }
        for field in GameplayTransform::ALL {
            let _ = adapter.set_transform(
                &InputScope::Global,
                field,
                self.transform(scope, field).value,
            );
        }
        adapter.input_overrides.global_binding_sets = self.resolved_typed_binding_sets(scope);
        for target in AutofireTarget::all() {
            let value = self.autofire(scope, target).value;
            adapter.input_overrides.global_autofire.insert(
                target.key(),
                value.map_or_else(AutofireOverride::disabled, AutofireOverride::enabled),
            );
        }
        adapter.capture_input_profile()
    }

    pub(crate) fn apply_controller_profile(
        &mut self,
        scope: &InputScope,
        profile: &InputProfileBindings,
    ) -> Result<(), String> {
        let mut adapter = Self::default();
        adapter.apply_input_profile(profile);
        adapter.input_overrides.validate()?;
        for field in GameplayTransform::ALL {
            field.set(&mut TiltSettings::default(), &field.get(&adapter.tilt))?;
        }
        let mut candidate = self.clone();
        for target in BindingTarget::all() {
            for source in [
                GameplayBindingSource::Keyboard,
                GameplayBindingSource::Gamepad,
            ] {
                if target.valid(source) {
                    candidate.set_binding(
                        scope,
                        target,
                        source,
                        adapter.global_binding(target, source),
                    )?;
                }
            }
        }
        for field in GameplayTransform::ALL {
            candidate.set_transform(scope, field, field.get(&adapter.tilt))?;
        }
        *candidate.input_overrides.binding_sets_mut(scope) = profile.binding_sets.clone();
        for target in AutofireTarget::all() {
            let value = profile
                .autofire
                .get(&target.key())
                .cloned()
                .unwrap_or_else(AutofireOverride::disabled);
            candidate.set_autofire(scope, target, value)?;
        }
        *self = candidate;
        Ok(())
    }

    pub(crate) fn apply_profile_shortcuts(&mut self, profile: &InputProfileBindings) {
        self.shortcut_bindings = profile.shortcuts.clone();
        self.speedup_key = profile.speedup_key.clone();
        self.rewind.key = profile.rewind_key.clone();
        for action in [
            GamepadAction::SpeedUp,
            GamepadAction::Rewind,
            GamepadAction::Pause,
            GamepadAction::Turbo,
        ] {
            self.gamepad_bindings
                .set_action(action, profile.gamepad.get_action(action));
        }
    }
}

#[cfg(test)]
mod tests;
