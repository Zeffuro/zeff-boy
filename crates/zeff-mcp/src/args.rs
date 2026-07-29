use anyhow::Context;
use serde_json::Value;

pub(crate) fn required_string(args: &Value, key: &str) -> anyhow::Result<String> {
    args.get(key)
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
        .with_context(|| format!("missing required string argument: {key}"))
}

pub(crate) fn optional_string(args: &Value, key: &str) -> Option<String> {
    args.get(key).and_then(Value::as_str).map(ToOwned::to_owned)
}

pub(crate) fn optional_bool(args: &Value, key: &str) -> Option<bool> {
    args.get(key).and_then(Value::as_bool)
}

pub(crate) fn optional_u64(args: &Value, key: &str) -> Option<u64> {
    args.get(key).and_then(Value::as_u64)
}

pub(crate) fn sequence_wait_millis(step: &Value) -> u64 {
    optional_u64(step, "ms")
        .or_else(|| optional_u64(step, "wait_ms"))
        .or_else(|| optional_u64(step, "milliseconds"))
        .or_else(|| optional_u64(step, "frames").map(frame_duration_ms))
        .unwrap_or_else(|| {
            optional_u64(step, "seconds")
                .unwrap_or(1)
                .saturating_mul(1000)
        })
        .min(60_000)
}

pub(crate) fn frame_duration_ms(frames: u64) -> u64 {
    frames.saturating_mul(1000).saturating_add(59) / 60
}

pub(crate) fn normalized_action(action: &str) -> String {
    action
        .trim()
        .to_ascii_lowercase()
        .chars()
        .filter(|ch| !matches!(ch, '-' | '_' | ' ' | '.'))
        .collect()
}
