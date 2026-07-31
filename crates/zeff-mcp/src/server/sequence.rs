use std::time::Duration;

use anyhow::{Context, bail};
use serde_json::{Value, json};

use crate::args::{
    frame_duration_ms, normalized_action, optional_bool, optional_string, optional_u64,
    sequence_wait_millis,
};

use super::{MAX_SEQUENCE_FRAME_ADVANCE, Server};

impl Server {
    pub(super) fn tool_sequence(&self, args: &Value) -> anyhow::Result<Value> {
        let steps = args
            .get("steps")
            .and_then(Value::as_array)
            .context("zeff_sequence requires steps array")?;
        let stop_on_error = optional_bool(args, "stop_on_error").unwrap_or(true);
        let mut results = Vec::with_capacity(steps.len());

        for (index, step) in steps.iter().enumerate() {
            let action = optional_string(step, "action")
                .or_else(|| optional_string(step, "command"))
                .or_else(|| optional_string(step, "tool"))
                .context("sequence step missing action")?;
            let step_result = self.run_sequence_step(&action, step);
            let success = step_result.is_ok();
            let result = match step_result {
                Ok(value) => json!({
                    "index": index,
                    "action": action,
                    "ok": true,
                    "result": value,
                }),
                Err(err) => json!({
                    "index": index,
                    "action": action,
                    "ok": false,
                    "error": err.to_string(),
                }),
            };
            results.push(result);
            if !success && stop_on_error {
                break;
            }
        }

        Ok(json!({
            "steps": results.len(),
            "results": results,
        }))
    }

    fn run_sequence_step(&self, action: &str, step: &Value) -> anyhow::Result<Value> {
        match normalized_action(action).as_str() {
            "wait" | "sleep" => {
                let wait_ms = sequence_wait_millis(step);
                std::thread::sleep(Duration::from_millis(wait_ms));
                Ok(json!({ "wait_ms": wait_ms }))
            }
            "status" => self.call_live(json!({ "command": "status" })),
            "debug" | "debuginfo" => self.call_live(json!({ "command": "debug_info" })),
            "graphics" | "ppu" => self.tool_graphics(),
            "memory" | "readmemory" => self.tool_memory(step),
            "screenshot" => self.tool_screenshot(step),
            "savestate" | "statesave" => self.tool_save_state(step),
            "loadstate" | "stateload" => self.tool_load_state(step),
            "pause" | "resume" | "toggle" => {
                let mut args = step.clone();
                args["action"] = json!(match normalized_action(action).as_str() {
                    "toggle" => "toggle",
                    "resume" => "resume",
                    _ => "pause",
                });
                self.tool_pause(&args)
            }
            "frameadvance" | "stepframe" => self.sequence_frame_advance(step),
            "speed" | "slowmotion" | "fastforward" | "uncapped" => {
                self.sequence_speed(action, step)
            }
            "button" | "tap" | "press" | "release" => self.sequence_button(action, step),
            "zapper" | "lightgun" => self.tool_zapper(step),
            other => bail!("unknown sequence action: {other}"),
        }
    }

    fn sequence_frame_advance(&self, step: &Value) -> anyhow::Result<Value> {
        let frames = optional_u64(step, "frames")
            .unwrap_or(1)
            .clamp(1, MAX_SEQUENCE_FRAME_ADVANCE);
        let mut last = Value::Null;
        let args = json!({ "action": "frame_advance" });
        for _ in 0..frames {
            last = self.tool_pause(&args)?;
            std::thread::sleep(Duration::from_millis(frame_duration_ms(1)));
        }
        Ok(json!({
            "frames": frames,
            "last": last,
        }))
    }

    fn sequence_button(&self, action: &str, step: &Value) -> anyhow::Result<Value> {
        let mut args = step.clone();
        if args.get("action").is_none() || normalized_action(action) != "button" {
            args["action"] = json!(action);
        }
        self.tool_button(&args)
    }

    fn sequence_speed(&self, action: &str, step: &Value) -> anyhow::Result<Value> {
        let mut args = step.clone();
        if args.get("mode").is_none() && normalized_action(action) != "speed" {
            args["mode"] = json!(action);
        }
        self.tool_speed(&args)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_sequence_action_names() {
        assert_eq!(normalized_action("frame_advance"), "frameadvance");
        assert_eq!(normalized_action("slow-motion"), "slowmotion");
    }

    #[test]
    fn wait_frames_use_rough_sixty_hz_duration() {
        assert_eq!(sequence_wait_millis(&json!({ "frames": 1 })), 17);
        assert_eq!(sequence_wait_millis(&json!({ "frames": 60 })), 1000);
        assert_eq!(sequence_wait_millis(&json!({ "frames": 10_000 })), 60_000);
    }

    #[test]
    fn explicit_wait_millis_override_frames() {
        assert_eq!(
            sequence_wait_millis(&json!({ "frames": 60, "ms": 123 })),
            123
        );
    }

    #[test]
    fn max_sequence_frame_advance_is_bounded() {
        assert_eq!(MAX_SEQUENCE_FRAME_ADVANCE, 600);
    }
}
