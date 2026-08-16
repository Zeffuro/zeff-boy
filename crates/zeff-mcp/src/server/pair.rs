use std::time::Duration;

use anyhow::{Context, bail};
use serde_json::{Value, json};

use crate::args::{optional_bool, optional_string, optional_u64, required_string};
use crate::protocol::DEFAULT_CONTROL_ADDR;
use crate::repo::repo_root_is_valid;

use super::pair_sequence::run_pair_step;
use super::{LaunchOptions, LiveInstance, PairState, Server};

const DEFAULT_RIGHT_ADDR: &str = "127.0.0.1:17685";

impl Server {
    pub(super) fn tool_pair_start(&mut self, args: &Value) -> anyhow::Result<Value> {
        let rom_path = required_string(args, "rom_path")?;
        let left_addr = optional_string(args, "left_addr")
            .or_else(|| optional_string(args, "addr_left"))
            .unwrap_or_else(|| DEFAULT_CONTROL_ADDR.to_string());
        let right_addr = optional_string(args, "right_addr")
            .or_else(|| optional_string(args, "addr_right"))
            .unwrap_or_else(|| DEFAULT_RIGHT_ADDR.to_string());
        if left_addr == right_addr {
            bail!("left_addr and right_addr must be different");
        }

        let release = optional_bool(args, "release").unwrap_or(false);
        let mute_audio = optional_bool(args, "mute_audio").unwrap_or(true);
        let wait_seconds = optional_u64(args, "wait_seconds").unwrap_or(45).min(180);
        let zeff_boy_exe = optional_string(args, "zeff_boy_exe");
        if let Some(repo_root) = optional_string(args, "repo_root") {
            self.state.repo_root = repo_root.into();
        }

        if self.child_is_running()? {
            bail!("a tracked single Zeff Boy instance is already running");
        }

        if self.pair_is_running()? {
            return Ok(json!({
                "started": false,
                "already_running": true,
                "status": self.tool_pair_status().ok(),
            }));
        }

        let launch = LaunchOptions {
            release,
            mute_audio,
            zeff_boy_exe: zeff_boy_exe.as_deref(),
        };
        let left_child = self.spawn_instance(&rom_path, &left_addr, &launch)?;
        let right_child = match self.spawn_instance(&rom_path, &right_addr, &launch) {
            Ok(child) => child,
            Err(err) => {
                let _ = stop_child(left_child);
                return Err(err);
            }
        };

        self.state.pair = Some(PairState {
            left: LiveInstance {
                control_addr: left_addr.clone(),
                child: left_child,
            },
            right: LiveInstance {
                control_addr: right_addr.clone(),
                child: right_child,
            },
        });

        let timeout = Duration::from_secs(wait_seconds);
        let left_status = self.wait_for_status_at(&left_addr, timeout);
        let right_status = self.wait_for_status_at(&right_addr, timeout);

        Ok(json!({
            "started": true,
            "ready": left_status.is_ok() && right_status.is_ok(),
            "repo_root_detected": repo_root_is_valid(&self.state.repo_root),
            "rom_path_redacted": true,
            "audio_muted": mute_audio,
            "left": {
                "addr": left_addr,
                "status": left_status.ok(),
            },
            "right": {
                "addr": right_addr,
                "status": right_status.ok(),
            },
        }))
    }

    pub(super) fn tool_pair_status(&mut self) -> anyhow::Result<Value> {
        if !self.pair_is_running()? {
            bail!("no tracked Zeff Boy pair is running");
        }
        let (left_addr, right_addr) = self.pair_addrs()?;
        Ok(json!({
            "left": {
                "addr": left_addr,
                "status": self.call_live_at(&left_addr, json!({ "command": "status" })).ok(),
            },
            "right": {
                "addr": right_addr,
                "status": self.call_live_at(&right_addr, json!({ "command": "status" })).ok(),
            },
        }))
    }

    pub(super) fn tool_pair_sequence(&mut self, args: &Value) -> anyhow::Result<Value> {
        if !self.pair_is_running()? {
            bail!("no tracked Zeff Boy pair is running");
        }
        let (left_addr, right_addr) = self.pair_addrs()?;
        let steps = args
            .get("steps")
            .and_then(Value::as_array)
            .context("zeff_pair_sequence requires steps array")?;
        let stop_on_error = optional_bool(args, "stop_on_error").unwrap_or(true);
        let mut results = Vec::with_capacity(steps.len());

        for (index, step) in steps.iter().enumerate() {
            let action = optional_string(step, "action")
                .or_else(|| optional_string(step, "command"))
                .or_else(|| optional_string(step, "tool"))
                .context("sequence step missing action")?;
            let target = optional_string(step, "target")
                .or_else(|| optional_string(step, "side"))
                .unwrap_or_else(|| "both".to_string());

            let step_result = run_pair_step(self, &left_addr, &right_addr, &target, &action, step);
            let ok = step_result.is_ok();
            results.push(match step_result {
                Ok(value) => json!({
                    "index": index,
                    "target": target,
                    "action": action,
                    "ok": true,
                    "result": value,
                }),
                Err(err) => json!({
                    "index": index,
                    "target": target,
                    "action": action,
                    "ok": false,
                    "error": err.to_string(),
                }),
            });
            if !ok && stop_on_error {
                break;
            }
        }

        Ok(json!({
            "steps": results.len(),
            "results": results,
        }))
    }

    pub(super) fn tool_pair_stop(&mut self) -> anyhow::Result<Value> {
        let Some(pair) = self.state.pair.take() else {
            return Ok(json!({
                "stopped": false,
                "reason": "no tracked Zeff Boy pair",
            }));
        };

        Ok(json!({
            "stopped": true,
            "left": stop_child(pair.left.child),
            "right": stop_child(pair.right.child),
        }))
    }

    pub(super) fn pair_is_running(&mut self) -> anyhow::Result<bool> {
        let Some(pair) = &mut self.state.pair else {
            return Ok(false);
        };
        let left_running = pair.left.child.try_wait()?.is_none();
        let right_running = pair.right.child.try_wait()?.is_none();
        if left_running || right_running {
            Ok(true)
        } else {
            self.state.pair = None;
            Ok(false)
        }
    }

    pub(super) fn pair_addrs(&self) -> anyhow::Result<(String, String)> {
        let pair = self
            .state
            .pair
            .as_ref()
            .context("no tracked Zeff Boy pair")?;
        Ok((
            pair.left.control_addr.clone(),
            pair.right.control_addr.clone(),
        ))
    }
}

fn stop_child(mut child: std::process::Child) -> Value {
    match child.try_wait() {
        Ok(Some(status)) => json!({
            "stopped": false,
            "already_exited": true,
            "exit_status": status.to_string(),
        }),
        Ok(None) => match child.kill().and_then(|()| child.wait()) {
            Ok(status) => json!({
                "stopped": true,
                "exit_status": status.to_string(),
            }),
            Err(err) => json!({
                "stopped": false,
                "error": err.to_string(),
            }),
        },
        Err(err) => json!({
            "stopped": false,
            "error": err.to_string(),
        }),
    }
}
