use std::time::Duration;

use anyhow::{Context, bail};
use serde_json::{Value, json};

use crate::args::{
    normalized_action, optional_bool, optional_string, optional_u64, required_string,
    sequence_wait_millis,
};

use super::Server;

pub(super) fn run_pair_step(
    server: &Server,
    left_addr: &str,
    right_addr: &str,
    target: &str,
    action: &str,
    step: &Value,
) -> anyhow::Result<Value> {
    if matches!(normalized_action(action).as_str(), "wait" | "sleep") {
        let wait_ms = sequence_wait_millis(step);
        std::thread::sleep(Duration::from_millis(wait_ms));
        return Ok(json!({ "wait_ms": wait_ms }));
    }

    let request = live_request_for_action(action, step)?;
    let target = normalized_action(target);
    match target.as_str() {
        "left" | "host" | "p1" | "1" => server.call_live_at(left_addr, request),
        "right" | "join" | "peer" | "p2" | "2" => server.call_live_at(right_addr, request),
        "both" | "pair" | "all" => run_both_pair_step(server, left_addr, right_addr, request),
        other => bail!("unknown pair target: {other}"),
    }
}

fn run_both_pair_step(
    server: &Server,
    left_addr: &str,
    right_addr: &str,
    request: Value,
) -> anyhow::Result<Value> {
    let left = server.call_live_at(left_addr, request.clone());
    let left_ok = left.is_ok();
    let left = result_json(left);
    let right = server.call_live_at(right_addr, request);
    let right_ok = right.is_ok();
    let right = result_json(right);
    if left_ok && right_ok {
        Ok(json!({
            "left": left,
            "right": right,
        }))
    } else {
        bail!(
            "pair step failed: left={}, right={}",
            side_result_summary(&left),
            side_result_summary(&right)
        )
    }
}

fn live_request_for_action(action: &str, step: &Value) -> anyhow::Result<Value> {
    match normalized_action(action).as_str() {
        "status" => Ok(json!({ "command": "status" })),
        "debug" | "debuginfo" => Ok(json!({ "command": "debug_info" })),
        "graphics" | "ppu" => Ok(json!({ "command": "graphics" })),
        "memory" | "readmemory" => live_memory_request(step),
        "screenshot" => live_optional_path_request("screenshot", step),
        "savestate" | "statesave" => live_optional_path_request("save_state", step),
        "loadstate" | "stateload" => Ok(json!({
            "command": "load_state",
            "path": required_string(step, "path")?,
        })),
        "stateslot" | "loadstateslot" | "savestateslot" => live_state_slot_request(action, step),
        "replay" | "recordreplay" | "startreplay" | "stopreplay" => {
            live_replay_request(action, step)
        }
        "link" | "hostlink" | "joinlink" | "connectlink" | "disconnectlink" => {
            live_link_request(action, step)
        }
        "pause" => Ok(json!({ "command": "pause" })),
        "resume" => Ok(json!({ "command": "resume" })),
        "toggle" => Ok(json!({ "command": "toggle_pause" })),
        "frameadvance" | "stepframe" => Ok(json!({ "command": "frame_advance" })),
        "speed" | "slowmotion" | "fastforward" | "uncapped" => live_speed_request(action, step),
        "button" | "tap" | "press" | "release" => live_button_request(action, step),
        "zapper" | "lightgun" => live_zapper_request(step),
        other => bail!("unknown pair sequence action: {other}"),
    }
}

fn live_optional_path_request(command: &str, step: &Value) -> anyhow::Result<Value> {
    let mut request = json!({ "command": command });
    if let Some(path) = optional_string(step, "path") {
        request["path"] = json!(path);
    }
    Ok(request)
}

fn live_memory_request(step: &Value) -> anyhow::Result<Value> {
    Ok(json!({
        "command": "memory",
        "space": optional_string(step, "space").unwrap_or_else(|| "cpu".to_string()),
        "start": optional_u64(step, "start")
            .or_else(|| optional_u64(step, "address"))
            .unwrap_or(0),
        "length": optional_u64(step, "length").unwrap_or(64),
    }))
}

fn live_state_slot_request(action: &str, step: &Value) -> anyhow::Result<Value> {
    let action_name = optional_string(step, "state_action")
        .or_else(|| optional_string(step, "slot_action"))
        .or_else(|| optional_string(step, "operation"))
        .unwrap_or_else(|| action.to_string());
    let command = match normalized_action(&action_name).as_str() {
        "save" | "savestate" | "statesave" | "savestateslot" => "save_state_slot",
        "load" | "loadstate" | "stateload" | "loadstateslot" | "stateslot" => "load_state_slot",
        other => bail!("unknown state-slot action: {other}"),
    };
    Ok(json!({
        "command": command,
        "slot": required_slot(step)?,
    }))
}

fn live_replay_request(action: &str, step: &Value) -> anyhow::Result<Value> {
    let action_name = optional_string(step, "replay_action")
        .or_else(|| optional_string(step, "operation"))
        .unwrap_or_else(|| action.to_string());
    match normalized_action(&action_name).as_str() {
        "start" | "record" | "recordreplay" | "startreplay" | "startrecording" => Ok(json!({
            "command": "record_replay",
            "path": required_string(step, "path")?,
        })),
        "stop" | "stopreplay" | "stoprecording" => Ok(json!({ "command": "stop_replay" })),
        other => bail!("unknown replay action: {other}"),
    }
}

fn live_link_request(action: &str, step: &Value) -> anyhow::Result<Value> {
    let action_name = optional_string(step, "link_action")
        .or_else(|| optional_string(step, "operation"))
        .unwrap_or_else(|| action.to_string());
    let command = match normalized_action(&action_name).as_str() {
        "host" | "hostlink" => "host_link",
        "join" | "connect" | "joinlink" | "connectlink" => "join_link",
        "disconnect" | "disconnectlink" => "disconnect_link",
        other => bail!("unknown link action: {other}"),
    };
    let mut request = json!({ "command": command });
    if let Some(addr) = optional_string(step, "addr")
        .or_else(|| optional_string(step, "address"))
        .or_else(|| optional_string(step, "connect_addr"))
    {
        request["addr"] = json!(addr);
    }
    Ok(request)
}

fn live_speed_request(action: &str, step: &Value) -> anyhow::Result<Value> {
    let mode = optional_string(step, "mode").unwrap_or_else(|| action.to_string());
    let command = match normalized_action(&mode).as_str() {
        "slow" | "slowmotion" => "slow_motion",
        "fast" | "fastforward" => "fast_forward",
        "uncapped" => "uncapped",
        other => bail!("unknown speed mode: {other}"),
    };
    Ok(json!({
        "command": command,
        "enabled": optional_bool(step, "enabled").unwrap_or(true),
    }))
}

fn live_button_request(action: &str, step: &Value) -> anyhow::Result<Value> {
    let command = match normalized_action(action).as_str() {
        "button" => optional_string(step, "button_action")
            .or_else(|| optional_string(step, "operation"))
            .unwrap_or_else(|| "tap".to_string()),
        other => other.to_string(),
    };
    let mut request = json!({
        "command": command,
        "button": required_string(step, "button")?,
    });
    if let Some(frames) = optional_u64(step, "frames") {
        request["frames"] = json!(frames);
    }
    if let Some(player) = optional_u64(step, "player")
        .or_else(|| optional_u64(step, "controller"))
        .or_else(|| optional_u64(step, "port"))
    {
        request["player"] = json!(player);
    }
    Ok(request)
}

fn live_zapper_request(step: &Value) -> anyhow::Result<Value> {
    let mut request = json!({
        "command": "zapper",
        "enabled": optional_bool(step, "enabled").unwrap_or(true),
        "trigger": optional_bool(step, "trigger").unwrap_or(false),
        "hit": optional_bool(step, "hit").unwrap_or(false),
    });
    if let (Some(x), Some(y)) = (
        optional_u64(step, "x").or_else(|| optional_u64(step, "screen_x")),
        optional_u64(step, "y").or_else(|| optional_u64(step, "screen_y")),
    ) {
        request["x"] = json!(x);
        request["y"] = json!(y);
    }
    Ok(request)
}

fn required_slot(step: &Value) -> anyhow::Result<u8> {
    let slot = optional_u64(step, "slot").context("missing required integer argument: slot")?;
    if slot <= 9 {
        Ok(slot as u8)
    } else {
        bail!("slot must be between 0 and 9")
    }
}

fn result_json(result: anyhow::Result<Value>) -> Value {
    match result {
        Ok(value) => json!({ "ok": true, "result": value }),
        Err(err) => json!({ "ok": false, "error": err.to_string() }),
    }
}

fn side_result_summary(result: &Value) -> String {
    if result.get("ok").and_then(Value::as_bool) == Some(true) {
        "ok".to_string()
    } else {
        result
            .get("error")
            .and_then(Value::as_str)
            .unwrap_or("failed")
            .to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::args::frame_duration_ms;

    #[test]
    fn pair_button_action_builds_live_tap_request() {
        let request =
            live_request_for_action("tap", &json!({ "button": "a", "target": "left" })).unwrap();
        assert_eq!(request["command"], "tap");
        assert_eq!(request["button"], "a");
    }

    #[test]
    fn pair_state_slot_defaults_to_load() {
        let request = live_request_for_action("state_slot", &json!({ "slot": 3 })).unwrap();
        assert_eq!(request["command"], "load_state_slot");
        assert_eq!(request["slot"], 3);
    }

    #[test]
    fn pair_wait_frames_use_live_sequence_timing() {
        assert_eq!(
            sequence_wait_millis(&json!({ "frames": 2 })),
            frame_duration_ms(2)
        );
    }
}
