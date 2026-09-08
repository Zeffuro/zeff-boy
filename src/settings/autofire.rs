use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use super::{BindingAction, InputScope};

pub(crate) const AUTOFIRE_ACTIONS: [BindingAction; 8] = [
    BindingAction::A,
    BindingAction::B,
    BindingAction::X,
    BindingAction::Y,
    BindingAction::L,
    BindingAction::R,
    BindingAction::Start,
    BindingAction::Select,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct AutofireTarget {
    pub(crate) player: u8,
    pub(crate) action: BindingAction,
}

impl AutofireTarget {
    pub(crate) fn all() -> impl Iterator<Item = Self> {
        (1..=5).flat_map(|player| {
            AUTOFIRE_ACTIONS
                .iter()
                .copied()
                .map(move |action| Self { player, action })
        })
    }

    pub(crate) fn key(self) -> String {
        format!("p{}.{}", self.player, action_key(self.action))
    }

    pub(crate) fn from_key(key: &str) -> Option<Self> {
        Self::all().find(|target| target.key() == key)
    }

    pub(crate) fn matrix_index(self) -> Option<(usize, usize)> {
        if self.player < 1 || self.player > 5 {
            return None;
        }
        let action = match self.action {
            BindingAction::A => 0,
            BindingAction::B => 1,
            BindingAction::X => 2,
            BindingAction::Y => 3,
            BindingAction::L => 4,
            BindingAction::R => 5,
            BindingAction::Start => 6,
            BindingAction::Select => 7,
            BindingAction::Up
            | BindingAction::Down
            | BindingAction::Left
            | BindingAction::Right => {
                return None;
            }
        };
        Some((self.player as usize - 1, action))
    }
}

const fn action_key(action: BindingAction) -> &'static str {
    match action {
        BindingAction::A => "a",
        BindingAction::B => "b",
        BindingAction::X => "x",
        BindingAction::Y => "y",
        BindingAction::L => "l",
        BindingAction::R => "r",
        BindingAction::Start => "start",
        BindingAction::Select => "select",
        BindingAction::Up => "up",
        BindingAction::Down => "down",
        BindingAction::Left => "left",
        BindingAction::Right => "right",
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct AutofirePattern {
    pub(crate) period_frames: u8,
    pub(crate) on_frames: u8,
}

impl Default for AutofirePattern {
    fn default() -> Self {
        Self {
            period_frames: 2,
            on_frames: 1,
        }
    }
}

impl AutofirePattern {
    pub(crate) const MAX_PERIOD_FRAMES: u8 = 60;

    pub(crate) const fn is_valid(self) -> bool {
        self.period_frames >= 1
            && self.period_frames <= Self::MAX_PERIOD_FRAMES
            && self.on_frames >= 1
            && self.on_frames <= self.period_frames
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct AutofireOverride {
    value: Option<AutofirePattern>,
    value_extensions: Map<String, Value>,
    unsupported: Option<Value>,
    extensions: Map<String, Value>,
}

impl AutofireOverride {
    pub(crate) fn disabled() -> Self {
        Self {
            value: None,
            value_extensions: Map::new(),
            unsupported: None,
            extensions: Map::new(),
        }
    }

    pub(crate) fn enabled(pattern: AutofirePattern) -> Self {
        Self {
            value: Some(pattern),
            value_extensions: Map::new(),
            unsupported: None,
            extensions: Map::new(),
        }
    }

    pub(crate) const fn value(&self) -> Option<AutofirePattern> {
        self.value
    }

    pub(crate) fn replace_value_preserving_extensions(
        &self,
        value: Option<AutofirePattern>,
    ) -> Self {
        let mut extensions = self.extensions.clone();
        let mut value_extensions = self.value_extensions.clone();
        if let Some(Value::Object(previous_value)) = extensions.remove("value") {
            for (key, value) in previous_value {
                if !matches!(key.as_str(), "period_frames" | "on_frames") {
                    value_extensions.entry(key).or_insert(value);
                }
            }
        }
        if value.is_none() && !value_extensions.is_empty() {
            extensions.insert("value".into(), Value::Object(value_extensions.clone()));
        }
        Self {
            value,
            value_extensions,
            unsupported: None,
            extensions,
        }
    }
}

impl Serialize for AutofireOverride {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        if let Some(value) = &self.unsupported {
            return value.serialize(serializer);
        }
        let mut object = self.extensions.clone();
        match self.value {
            Some(pattern) => {
                let mut value = self.value_extensions.clone();
                value.insert("period_frames".into(), pattern.period_frames.into());
                value.insert("on_frames".into(), pattern.on_frames.into());
                object.insert("state".into(), "enabled".into());
                object.insert("value".into(), Value::Object(value));
            }
            None => {
                object.insert("state".into(), "disabled".into());
            }
        }
        object.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for AutofireOverride {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = Value::deserialize(deserializer)?;
        let mut object = raw
            .as_object()
            .cloned()
            .ok_or_else(|| serde::de::Error::custom("An autofire override must be an object."))?;
        let state = object
            .remove("state")
            .and_then(|value| value.as_str().map(str::to_owned))
            .ok_or_else(|| serde::de::Error::custom("An autofire override needs a state."))?;
        let (value, value_extensions) = match state.as_str() {
            "enabled" => {
                let mut value = object
                    .remove("value")
                    .and_then(|value| value.as_object().cloned())
                    .ok_or_else(|| {
                        serde::de::Error::custom("An enabled autofire override needs a value.")
                    })?;
                let period_frames =
                    serde_json::from_value(value.remove("period_frames").ok_or_else(|| {
                        serde::de::Error::custom(
                            "An enabled autofire override needs period_frames.",
                        )
                    })?)
                    .map_err(serde::de::Error::custom)?;
                let on_frames =
                    serde_json::from_value(value.remove("on_frames").ok_or_else(|| {
                        serde::de::Error::custom("An enabled autofire override needs on_frames.")
                    })?)
                    .map_err(serde::de::Error::custom)?;
                (
                    Some(AutofirePattern {
                        period_frames,
                        on_frames,
                    }),
                    value,
                )
            }
            "disabled" => (None, Map::new()),
            _ => {
                return Ok(Self {
                    value: None,
                    value_extensions: Map::new(),
                    unsupported: Some(raw),
                    extensions: Map::new(),
                });
            }
        };
        Ok(Self {
            value,
            value_extensions,
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
    fn future_records_round_trip_as_inactive() {
        let raw = json!({"state":"future_wave","value":{"period_frames":3},"keep":true});
        let record: AutofireOverride = serde_json::from_value(raw.clone()).unwrap();
        assert_eq!(record.value(), None);
        assert_eq!(serde_json::to_value(record).unwrap(), raw);
    }

    #[test]
    fn known_records_keep_extension_fields() {
        let raw = json!({
            "state":"enabled",
            "value":{"period_frames":3,"on_frames":2,"future_value":true},
            "future_setting":true
        });
        let record: AutofireOverride = serde_json::from_value(raw.clone()).unwrap();
        assert_eq!(
            record.value(),
            Some(AutofirePattern {
                period_frames: 3,
                on_frames: 2,
            })
        );
        assert_eq!(serde_json::to_value(record).unwrap(), raw);
    }

    #[test]
    fn replacing_a_known_value_retains_nested_extensions() {
        let record: AutofireOverride = serde_json::from_value(json!({
            "state":"enabled",
            "value":{"period_frames":3,"on_frames":2,"future_value":true},
            "future_setting":true
        }))
        .unwrap();
        let updated = record.replace_value_preserving_extensions(Some(AutofirePattern {
            period_frames: 4,
            on_frames: 1,
        }));
        assert_eq!(
            serde_json::to_value(updated).unwrap(),
            json!({
                "state":"enabled",
                "value":{"period_frames":4,"on_frames":1,"future_value":true},
                "future_setting":true
            })
        );
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ResolvedAutofire {
    pub(crate) value: Option<AutofirePattern>,
    pub(crate) origin: InputScope,
}
