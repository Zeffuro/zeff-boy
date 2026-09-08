use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use winit::keyboard::KeyCode;

use super::{GameplayBindingSource, PhysicalBinding};

pub(crate) const MAX_BINDING_ALTERNATIVES: usize = 8;
pub(crate) const MAX_CHORD_ATOMS: usize = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum InputAxis {
    LeftX,
    LeftY,
    RightX,
    RightY,
}

impl InputAxis {
    pub(crate) const ALL: [Self; 4] = [Self::LeftX, Self::LeftY, Self::RightX, Self::RightY];

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::LeftX => "Left stick X",
            Self::LeftY => "Left stick Y",
            Self::RightX => "Right stick X",
            Self::RightY => "Right stick Y",
        }
    }

    pub(crate) fn value(self, left: (f32, f32), right: (f32, f32)) -> f32 {
        match self {
            Self::LeftX => left.0,
            Self::LeftY => left.1,
            Self::RightX => right.0,
            Self::RightY => right.1,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum AxisDirection {
    Positive,
    Negative,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct AxisTransform {
    pub(crate) deadzone: f32,
    pub(crate) curve: f32,
    pub(crate) invert: bool,
    #[serde(flatten)]
    extensions: Map<String, Value>,
}

impl Default for AxisTransform {
    fn default() -> Self {
        Self {
            deadzone: 0.0,
            curve: 1.0,
            invert: false,
            extensions: Map::new(),
        }
    }
}

impl AxisTransform {
    pub(crate) fn validate(&self) -> Result<(), String> {
        if !self.deadzone.is_finite() || !(0.0..=1.0).contains(&self.deadzone) {
            return Err("Axis deadzone must be between 0 and 1.".into());
        }
        if !self.curve.is_finite() || !(0.1..=8.0).contains(&self.curve) {
            return Err("Axis curve must be between 0.1 and 8.".into());
        }
        Ok(())
    }

    pub(crate) fn apply(&self, value: f32) -> f32 {
        if !value.is_finite() || self.validate().is_err() {
            return 0.0;
        }
        let value = value.clamp(-1.0, 1.0) * if self.invert { -1.0 } else { 1.0 };
        if value.abs() <= self.deadzone || self.deadzone >= 1.0 {
            return 0.0;
        }
        let magnitude = ((value.abs() - self.deadzone) / (1.0 - self.deadzone)).powf(self.curve);
        value.signum() * magnitude
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct AxisBinding {
    pub(crate) axis: InputAxis,
    pub(crate) direction: AxisDirection,
    pub(crate) press_threshold: f32,
    pub(crate) release_threshold: f32,
    #[serde(default)]
    pub(crate) transform: AxisTransform,
    #[serde(flatten)]
    extensions: Map<String, Value>,
}

impl AxisBinding {
    pub(crate) fn new(axis: InputAxis, direction: AxisDirection) -> Self {
        Self {
            axis,
            direction,
            press_threshold: 0.55,
            release_threshold: 0.4,
            transform: AxisTransform::default(),
            extensions: Map::new(),
        }
    }

    pub(crate) fn validate(&self) -> Result<(), String> {
        self.transform.validate()?;
        if !self.press_threshold.is_finite()
            || !self.release_threshold.is_finite()
            || !(0.0..=1.0).contains(&self.release_threshold)
            || !(0.0..=1.0).contains(&self.press_threshold)
            || self.release_threshold >= self.press_threshold
        {
            return Err("Axis thresholds must satisfy 0 ≤ release < press ≤ 1.".into());
        }
        Ok(())
    }

    pub(crate) fn transformed_value(&self, value: f32) -> f32 {
        let value = self.transform.apply(value);
        match self.direction {
            AxisDirection::Positive => value,
            AxisDirection::Negative => -value,
        }
    }

    pub(crate) fn evaluate(&self, value: f32, was_active: bool) -> bool {
        if self.validate().is_err() {
            return false;
        }
        let value = self.transformed_value(value);
        if was_active {
            value > self.release_threshold
        } else {
            value >= self.press_threshold
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum BindingExpressionKind {
    Keyboard(KeyCode),
    GamepadButton(String),
    Axis(AxisBinding),
    Chord(Vec<BindingExpression>),
    Unknown(Value),
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct BindingExpression {
    pub(crate) kind: BindingExpressionKind,
    extensions: Map<String, Value>,
}

impl BindingExpression {
    pub(crate) fn new(kind: BindingExpressionKind) -> Self {
        Self {
            kind,
            extensions: Map::new(),
        }
    }

    pub(crate) fn keyboard(key: KeyCode) -> Self {
        Self::new(BindingExpressionKind::Keyboard(key))
    }

    pub(crate) fn gamepad_button(button: impl Into<String>) -> Self {
        Self::new(BindingExpressionKind::GamepadButton(button.into()))
    }

    pub(crate) fn axis(binding: AxisBinding) -> Self {
        Self::new(BindingExpressionKind::Axis(binding))
    }

    pub(crate) fn chord(atoms: Vec<Self>) -> Self {
        Self::new(BindingExpressionKind::Chord(atoms))
    }

    pub(crate) fn is_supported(&self) -> bool {
        match &self.kind {
            BindingExpressionKind::Unknown(_) => false,
            BindingExpressionKind::Chord(atoms) => atoms.iter().all(Self::is_supported),
            _ => true,
        }
    }

    pub(crate) fn keyboard_keys(&self) -> Vec<KeyCode> {
        let mut keys = Vec::new();
        self.collect_keyboard_keys(&mut keys);
        keys
    }

    fn collect_keyboard_keys(&self, keys: &mut Vec<KeyCode>) {
        match &self.kind {
            BindingExpressionKind::Keyboard(key) if !keys.contains(key) => keys.push(*key),
            BindingExpressionKind::Chord(atoms) => {
                for atom in atoms {
                    atom.collect_keyboard_keys(keys);
                }
            }
            _ => {}
        }
    }

    pub(crate) fn references_key(&self, key: KeyCode) -> bool {
        match &self.kind {
            BindingExpressionKind::Keyboard(bound) => *bound == key,
            BindingExpressionKind::Chord(atoms) => {
                atoms.iter().any(|atom| atom.references_key(key))
            }
            _ => false,
        }
    }

    pub(crate) fn evaluate(&self, mut atom: impl FnMut(&BindingExpressionKind) -> bool) -> bool {
        self.evaluate_atom(&mut atom)
    }

    fn evaluate_atom(&self, atom: &mut dyn FnMut(&BindingExpressionKind) -> bool) -> bool {
        match &self.kind {
            BindingExpressionKind::Unknown(_) => false,
            BindingExpressionKind::Chord(atoms) => atoms
                .iter()
                .map(|expression| expression.evaluate_atom(atom))
                .fold(!atoms.is_empty(), |active, value| active & value),
            kind => atom(kind),
        }
    }

    pub(crate) fn label(&self) -> String {
        match &self.kind {
            BindingExpressionKind::Keyboard(key) => format!("{key:?} (physical)"),
            BindingExpressionKind::GamepadButton(button) => button.clone(),
            BindingExpressionKind::Axis(binding) => format!(
                "{} {}",
                binding.axis.label(),
                match binding.direction {
                    AxisDirection::Positive => "+",
                    AxisDirection::Negative => "−",
                }
            ),
            BindingExpressionKind::Chord(atoms) => atoms
                .iter()
                .map(Self::label)
                .collect::<Vec<_>>()
                .join(" + "),
            BindingExpressionKind::Unknown(_) => "Unsupported binding (retained)".into(),
        }
    }

    pub(crate) fn as_physical_binding(&self) -> Option<PhysicalBinding> {
        match &self.kind {
            BindingExpressionKind::Keyboard(key) => Some(PhysicalBinding::Keyboard(*key)),
            BindingExpressionKind::GamepadButton(button) => {
                Some(PhysicalBinding::Gamepad(button.clone()))
            }
            _ => None,
        }
    }

    pub(crate) fn validate(&self, source: GameplayBindingSource) -> Result<(), String> {
        match &self.kind {
            BindingExpressionKind::Keyboard(key) => {
                if source != GameplayBindingSource::Keyboard {
                    return Err("Controller bindings cannot contain keyboard keys.".into());
                }
                if super::keycode_from_string(&format!("{key:?}")).is_none() {
                    return Err("This physical keyboard key is unsupported.".into());
                }
            }
            BindingExpressionKind::GamepadButton(button) => {
                if source != GameplayBindingSource::Gamepad {
                    return Err("Keyboard bindings cannot contain controller buttons.".into());
                }
                if button.is_empty() || button.len() > 64 || button.chars().any(char::is_control) {
                    return Err("Use a controller button name of 1–64 characters.".into());
                }
            }
            BindingExpressionKind::Axis(binding) => {
                if source != GameplayBindingSource::Gamepad {
                    return Err("Keyboard bindings cannot contain controller axes.".into());
                }
                binding.validate()?;
            }
            BindingExpressionKind::Chord(atoms) => {
                if !(2..=MAX_CHORD_ATOMS).contains(&atoms.len()) {
                    return Err(format!(
                        "A chord must contain 2–{MAX_CHORD_ATOMS} controls."
                    ));
                }
                let mut controls = BTreeSet::new();
                for atom in atoms {
                    if matches!(atom.kind, BindingExpressionKind::Chord(_)) {
                        return Err("Chords cannot contain other chords.".into());
                    }
                    atom.validate(source)?;
                    if let Some(control) = atom.control_key()
                        && !controls.insert(control)
                    {
                        return Err("A chord cannot contain the same control twice.".into());
                    }
                }
            }
            BindingExpressionKind::Unknown(_) => {}
        }
        Ok(())
    }

    fn control_key(&self) -> Option<String> {
        match &self.kind {
            BindingExpressionKind::Keyboard(key) => Some(format!("key:{key:?}")),
            BindingExpressionKind::GamepadButton(button) => Some(format!("button:{button}")),
            BindingExpressionKind::Axis(binding) => {
                Some(format!("axis:{:?}:{:?}", binding.axis, binding.direction))
            }
            _ => None,
        }
    }

    fn to_value(&self) -> Value {
        let mut object = self.extensions.clone();
        match &self.kind {
            BindingExpressionKind::Keyboard(key) => {
                object.insert("kind".into(), "keyboard".into());
                object.insert("key".into(), format!("{key:?}").into());
            }
            BindingExpressionKind::GamepadButton(button) => {
                object.insert("kind".into(), "gamepad_button".into());
                object.insert("button".into(), button.clone().into());
            }
            BindingExpressionKind::Axis(binding) => {
                if let Ok(Value::Object(axis)) = serde_json::to_value(binding) {
                    object.extend(axis);
                }
                object.insert("kind".into(), "axis".into());
            }
            BindingExpressionKind::Chord(atoms) => {
                object.insert("kind".into(), "chord".into());
                object.insert(
                    "atoms".into(),
                    Value::Array(atoms.iter().map(Self::to_value).collect()),
                );
            }
            BindingExpressionKind::Unknown(value) => return value.clone(),
        }
        Value::Object(object)
    }

    fn from_value(value: Value) -> Result<Self, String> {
        let mut object = value
            .as_object()
            .cloned()
            .ok_or("A binding expression must be an object.")?;
        let kind = object
            .remove("kind")
            .and_then(|kind| kind.as_str().map(str::to_owned))
            .ok_or("A binding expression needs a kind.")?;
        let kind = match kind.as_str() {
            "keyboard" => {
                let key = take_string(&mut object, "key")?;
                BindingExpressionKind::Keyboard(
                    super::keycode_from_string(&key)
                        .ok_or_else(|| format!("Unknown physical key: {key}"))?,
                )
            }
            "gamepad_button" => {
                BindingExpressionKind::GamepadButton(take_string(&mut object, "button")?)
            }
            "axis" => {
                let binding = serde_json::from_value(Value::Object(std::mem::take(&mut object)))
                    .map_err(|error| error.to_string())?;
                BindingExpressionKind::Axis(binding)
            }
            "chord" => {
                let atoms = object
                    .remove("atoms")
                    .ok_or("A chord needs its controls.")?;
                let atoms: Vec<Self> =
                    serde_json::from_value(atoms).map_err(|error| error.to_string())?;
                BindingExpressionKind::Chord(atoms)
            }
            _ => return Ok(Self::new(BindingExpressionKind::Unknown(value))),
        };
        Ok(Self {
            kind,
            extensions: object,
        })
    }
}

fn take_string(object: &mut Map<String, Value>, field: &str) -> Result<String, String> {
    object
        .remove(field)
        .and_then(|value| value.as_str().map(str::to_owned))
        .ok_or_else(|| format!("Binding field {field} must be text."))
}

impl Serialize for BindingExpression {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.to_value().serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for BindingExpression {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::from_value(Value::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct BindingAlternative {
    pub(crate) id: u64,
    pub(crate) expression: BindingExpression,
    #[serde(flatten)]
    extensions: Map<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct BindingSet {
    pub(crate) alternatives: Vec<BindingAlternative>,
    next_id: u64,
    #[serde(flatten)]
    extensions: Map<String, Value>,
}

impl Default for BindingSet {
    fn default() -> Self {
        Self {
            alternatives: Vec::new(),
            next_id: 1,
            extensions: Map::new(),
        }
    }
}

impl BindingSet {
    pub(crate) fn new(expression: BindingExpression) -> Self {
        Self {
            alternatives: vec![BindingAlternative {
                id: 1,
                expression,
                extensions: Map::new(),
            }],
            next_id: 2,
            extensions: Map::new(),
        }
    }

    pub(crate) fn from_physical_binding(binding: PhysicalBinding) -> Self {
        Self::new(match binding {
            PhysicalBinding::Keyboard(key) => BindingExpression::keyboard(key),
            PhysicalBinding::Gamepad(button) => BindingExpression::gamepad_button(button),
        })
    }

    pub(crate) fn as_single_physical_binding(&self) -> Option<PhysicalBinding> {
        let [alternative] = self.alternatives.as_slice() else {
            return None;
        };
        alternative.expression.as_physical_binding()
    }

    pub(crate) fn evaluate(
        &self,
        mut atom: impl FnMut(u64, &BindingExpressionKind) -> bool,
    ) -> bool {
        self.alternatives
            .iter()
            .map(|alternative| {
                alternative
                    .expression
                    .evaluate(|kind| atom(alternative.id, kind))
            })
            .fold(false, |active, value| active | value)
    }

    pub(crate) fn add_expression(&mut self, expression: BindingExpression) -> Result<u64, String> {
        if self.alternatives.len() >= MAX_BINDING_ALTERNATIVES {
            return Err(format!(
                "An action supports at most {MAX_BINDING_ALTERNATIVES} alternatives."
            ));
        }
        let minimum = self
            .alternatives
            .iter()
            .map(|alternative| alternative.id)
            .max()
            .unwrap_or(0)
            .checked_add(1)
            .ok_or("Binding identifiers are exhausted.")?;
        let id = self.next_id.max(minimum).max(1);
        self.next_id = id
            .checked_add(1)
            .ok_or("Binding identifiers are exhausted.")?;
        self.alternatives.push(BindingAlternative {
            id,
            expression,
            extensions: Map::new(),
        });
        Ok(id)
    }

    pub(crate) fn replace_expression(
        &mut self,
        id: u64,
        expression: BindingExpression,
    ) -> Result<(), String> {
        let alternative = self
            .alternatives
            .iter_mut()
            .find(|alternative| alternative.id == id)
            .ok_or("That binding alternative no longer exists.")?;
        alternative.expression = expression;
        Ok(())
    }

    pub(crate) fn remove_alternative(&mut self, id: u64) -> bool {
        if let Some(maximum) = self
            .alternatives
            .iter()
            .map(|alternative| alternative.id)
            .max()
        {
            self.next_id = self.next_id.max(maximum.saturating_add(1));
        }
        let before = self.alternatives.len();
        self.alternatives.retain(|alternative| alternative.id != id);
        self.alternatives.len() != before
    }

    pub(crate) fn validate(&self, source: GameplayBindingSource) -> Result<(), String> {
        if !(1..=MAX_BINDING_ALTERNATIVES).contains(&self.alternatives.len()) {
            return Err(format!(
                "Use 1–{MAX_BINDING_ALTERNATIVES} alternatives, or choose Unbind."
            ));
        }
        let mut ids = BTreeSet::new();
        for alternative in &self.alternatives {
            if alternative.id == 0 || !ids.insert(alternative.id) {
                return Err("Binding alternatives need distinct nonzero identifiers.".into());
            }
            alternative.expression.validate(source)?;
        }
        Ok(())
    }

    pub(crate) fn label(&self) -> String {
        self.alternatives
            .iter()
            .map(|alternative| alternative.expression.label())
            .collect::<Vec<_>>()
            .join(" OR ")
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct BindingSetOverride {
    pub(crate) value: Option<BindingSet>,
    unsupported: Option<Value>,
    extensions: Map<String, Value>,
}

impl BindingSetOverride {
    pub(crate) fn new(value: Option<BindingSet>) -> Self {
        Self {
            value,
            unsupported: None,
            extensions: Map::new(),
        }
    }

    pub(crate) fn replace(&mut self, value: Option<BindingSet>) {
        self.value = value;
        self.unsupported = None;
    }

    pub(crate) fn validate(&self, source: GameplayBindingSource) -> Result<(), String> {
        if let Some(value) = &self.value {
            value.validate(source)?;
        }
        Ok(())
    }
}

impl Serialize for BindingSetOverride {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        if let Some(value) = &self.unsupported {
            return value.serialize(serializer);
        }
        let mut object = self.extensions.clone();
        if let Some(value) = &self.value {
            object.insert("state".into(), "set".into());
            object.insert(
                "value".into(),
                serde_json::to_value(value).map_err(serde::ser::Error::custom)?,
            );
        } else {
            object.insert("state".into(), "unbound".into());
            object.remove("value");
        }
        object.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for BindingSetOverride {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = Value::deserialize(deserializer)?;
        let mut object = raw
            .as_object()
            .cloned()
            .ok_or_else(|| serde::de::Error::custom("A binding override must be an object."))?;
        let state = take_string(&mut object, "state").map_err(serde::de::Error::custom)?;
        let value = match state.as_str() {
            "set" => Some(
                serde_json::from_value(
                    object
                        .remove("value")
                        .ok_or_else(|| serde::de::Error::custom("A Set binding needs a value."))?,
                )
                .map_err(serde::de::Error::custom)?,
            ),
            "unbound" => {
                object.remove("value");
                None
            }
            _ => {
                return Ok(Self {
                    value: None,
                    unsupported: Some(raw),
                    extensions: Map::new(),
                });
            }
        };
        Ok(Self {
            value,
            unsupported: None,
            extensions: object,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn chord_visits_every_atom_and_preserves_physical_modifier_identity() {
        let chord = BindingExpression::chord(vec![
            BindingExpression::keyboard(KeyCode::ControlLeft),
            BindingExpression::keyboard(KeyCode::KeyR),
        ]);
        assert_eq!(
            chord.keyboard_keys(),
            vec![KeyCode::ControlLeft, KeyCode::KeyR]
        );
        assert!(!chord.references_key(KeyCode::ControlRight));
        let mut visits = 0;
        assert!(!chord.evaluate(|kind| {
            visits += 1;
            matches!(kind, BindingExpressionKind::Keyboard(KeyCode::KeyR))
        }));
        assert_eq!(visits, 2);
        assert!(chord.evaluate(|_| true));
        let mut alternatives = BindingSet::new(BindingExpression::keyboard(KeyCode::KeyX));
        alternatives.add_expression(chord).unwrap();
        let mut visits = 0;
        assert!(alternatives.evaluate(|id, _| {
            visits += 1;
            id == 1
        }));
        assert_eq!(visits, 3);
    }

    #[test]
    fn limits_and_source_constraints_reject_ambiguous_chords() {
        let key = BindingExpression::keyboard(KeyCode::KeyX);
        assert!(
            BindingExpression::chord(vec![key.clone()])
                .validate(GameplayBindingSource::Keyboard)
                .is_err()
        );
        assert!(
            BindingExpression::chord(vec![key.clone(), key.clone()])
                .validate(GameplayBindingSource::Keyboard)
                .is_err()
        );
        assert!(
            BindingExpression::chord(vec![
                key.clone(),
                BindingExpression::gamepad_button("South")
            ])
            .validate(GameplayBindingSource::Keyboard)
            .is_err()
        );
        let mut set = BindingSet::new(key.clone());
        for _ in 1..MAX_BINDING_ALTERNATIVES {
            set.add_expression(key.clone()).unwrap();
        }
        assert!(set.add_expression(key).is_err());
        assert!(set.validate(GameplayBindingSource::Keyboard).is_ok());
        set.alternatives[1].id = set.alternatives[0].id;
        assert!(set.validate(GameplayBindingSource::Keyboard).is_err());
    }

    #[test]
    fn stable_alternative_identity_owns_extensions_through_reorder_and_removal() {
        let mut set: BindingSet = serde_json::from_value(json!({
            "future_set": {"keep": true},
            "alternatives": [
                {"id": 10, "future_alternative": "first", "expression": {"kind": "keyboard", "key": "KeyX", "future_atom": 7}},
                {"id": 20, "future_alternative": "second", "expression": {"kind": "future_touch", "future_payload": {"a": [1, 2]}}}
            ]
        })).unwrap();
        set.validate(GameplayBindingSource::Keyboard).unwrap();
        set.alternatives.reverse();
        assert!(!set.alternatives[0].expression.evaluate(|_| true));
        let raw = serde_json::to_value(&set).unwrap();
        assert_eq!(raw["alternatives"][0]["future_alternative"], "second");
        assert_eq!(
            raw["alternatives"][0]["expression"],
            json!({"kind": "future_touch", "future_payload": {"a": [1, 2]}})
        );
        assert_eq!(raw["alternatives"][1]["expression"]["future_atom"], 7);
        set.remove_alternative(20);
        assert_eq!(
            set.add_expression(BindingExpression::keyboard(KeyCode::KeyZ))
                .unwrap(),
            21
        );
        let raw = serde_json::to_value(&set).unwrap();
        assert_eq!(raw["future_set"]["keep"], true);
        assert_eq!(raw["alternatives"][0]["future_alternative"], "first");
        assert!(raw["alternatives"][1].get("future_alternative").is_none());
    }

    #[test]
    fn removed_ids_are_not_reused_and_unbound_removes_stale_payload() {
        let mut set = BindingSet::new(BindingExpression::keyboard(KeyCode::KeyX));
        let second = set
            .add_expression(BindingExpression::keyboard(KeyCode::KeyZ))
            .unwrap();
        set.remove_alternative(second);
        assert!(
            set.add_expression(BindingExpression::keyboard(KeyCode::KeyC))
                .unwrap()
                > second
        );
        let mut record: BindingSetOverride = serde_json::from_value(
            json!({"state":"set", "value":serde_json::to_value(set).unwrap(), "future":9}),
        )
        .unwrap();
        record.replace(None);
        let value = serde_json::to_value(record).unwrap();
        assert_eq!(value, json!({"state":"unbound", "future":9}));
    }

    #[test]
    fn axis_transform_hysteresis_and_invalid_samples_are_explicit() {
        let mut binding = AxisBinding::new(InputAxis::RightY, AxisDirection::Negative);
        binding.press_threshold = 0.75;
        binding.release_threshold = 0.25;
        assert!(!binding.evaluate(-0.5, false));
        assert!(binding.evaluate(-0.75, false));
        assert!(binding.evaluate(-0.5, true));
        assert!(!binding.evaluate(-0.25, true));
        assert!(!binding.evaluate(f32::NAN, true));
        binding.transform.deadzone = 0.5;
        binding.transform.curve = 2.0;
        assert_eq!(binding.transformed_value(-0.75), 0.25);
        binding.transform.invert = true;
        assert_eq!(binding.transformed_value(0.75), 0.25);
        assert_eq!(binding.transformed_value(20.0), 1.0);
        binding.release_threshold = binding.press_threshold;
        assert!(binding.validate().is_err());
        assert!(!binding.evaluate(1.0, true));
    }

    #[test]
    fn future_override_state_roundtrips_without_activation() {
        let raw = json!({"state":"future_mode", "value":{"key":"KeyX"}, "future":1});
        let value: BindingSetOverride = serde_json::from_value(raw.clone()).unwrap();
        assert!(value.value.is_none());
        assert_eq!(serde_json::to_value(value).unwrap(), raw);
    }
}
