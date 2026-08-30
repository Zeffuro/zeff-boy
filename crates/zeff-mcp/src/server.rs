use std::ffi::OsString;
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::{Context, bail};
use serde_json::{Value, json};

use crate::args::{
    normalized_action, optional_bool, optional_string, optional_u64, required_string,
};
use crate::live_client;
use crate::protocol::{DEFAULT_CONTROL_ADDR, initialize_result, jsonrpc_error, tool_result, tools};
use crate::repo::{repo_root_is_valid, resolve_repo_root};

mod pair;
mod pair_gb_trade_fixture;
mod pair_gb_trade_fixture_config;
mod pair_gb_trade_fixture_replay;
mod pair_gb_trade_fixture_route;
mod pair_gb_trade_fixture_screen;
mod pair_sequence;
mod sequence;

struct LaunchOptions<'a> {
    release: bool,
    mute_audio: bool,
    zeff_boy_exe: Option<&'a str>,
}

const MAX_SEQUENCE_FRAME_ADVANCE: u64 = 600;

pub(crate) fn run() -> anyhow::Result<()> {
    Server::new().run()
}

struct Server {
    state: ServerState,
}

struct ServerState {
    control_addr: String,
    repo_root: PathBuf,
    child: Option<Child>,
    pair: Option<PairState>,
}

struct PairState {
    left: LiveInstance,
    right: LiveInstance,
}

struct LiveInstance {
    control_addr: String,
    child: Child,
}

impl Server {
    fn new() -> Self {
        Self {
            state: ServerState {
                control_addr: DEFAULT_CONTROL_ADDR.to_string(),
                repo_root: resolve_repo_root().unwrap_or_else(|| {
                    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
                }),
                child: None,
                pair: None,
            },
        }
    }

    fn run(&mut self) -> anyhow::Result<()> {
        let stdin = io::stdin();
        let mut stdout = io::stdout().lock();

        for line in stdin.lock().lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            let Some(response) = self.handle_line(&line) else {
                continue;
            };
            writeln!(stdout, "{response}")?;
            stdout.flush()?;
        }

        Ok(())
    }

    fn handle_line(&mut self, line: &str) -> Option<Value> {
        let request: Value = match serde_json::from_str(line) {
            Ok(request) => request,
            Err(err) => {
                return Some(jsonrpc_error(
                    Value::Null,
                    -32700,
                    format!("parse error: {err}"),
                ));
            }
        };

        let id = request.get("id").cloned();
        let method = request.get("method").and_then(Value::as_str).unwrap_or("");
        let params = request.get("params").cloned().unwrap_or(Value::Null);

        let result = match method {
            "initialize" => Ok(initialize_result(&params)),
            "ping" => Ok(json!({})),
            "tools/list" => Ok(json!({ "tools": tools() })),
            "tools/call" => self.handle_tool_call(&params),
            "resources/list" => Ok(json!({ "resources": [] })),
            "prompts/list" => Ok(json!({ "prompts": [] })),
            "notifications/initialized" | "notifications/cancelled" => return None,
            "" => Err(anyhow::anyhow!("missing method")),
            other => {
                return id.map(|id| jsonrpc_error(id, -32601, format!("unknown method: {other}")));
            }
        };

        let id = id?;
        Some(match result {
            Ok(result) => json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": result,
            }),
            Err(err) => jsonrpc_error(id, -32000, err.to_string()),
        })
    }

    fn handle_tool_call(&mut self, params: &Value) -> anyhow::Result<Value> {
        let name = params
            .get("name")
            .and_then(Value::as_str)
            .context("tools/call missing params.name")?;
        let args = params.get("arguments").unwrap_or(&Value::Null);

        let result = match name {
            "zeff_start" => self.tool_start(args),
            "zeff_status" => self.call_live(json!({ "command": "status" })),
            "zeff_debug_info" => self.call_live(json!({ "command": "debug_info" })),
            "zeff_button" => self.tool_button(args),
            "zeff_zapper" => self.tool_zapper(args),
            "zeff_pause" => self.tool_pause(args),
            "zeff_speed" => self.tool_speed(args),
            "zeff_screenshot" => self.tool_screenshot(args),
            "zeff_save_state" => self.tool_save_state(args),
            "zeff_load_state" => self.tool_load_state(args),
            "zeff_state_slot" => self.tool_state_slot(args),
            "zeff_replay" => self.tool_replay(args),
            "zeff_tas_open" => self.tool_tas_open(args),
            "zeff_tas_status" => self.call_live(json!({ "command": "tas_status" })),
            "zeff_tas_link" => self.tool_tas_link(args),
            "zeff_tas_record_frame" => self.call_live(json!({ "command": "tas_record_frame" })),
            "zeff_tas_disconnect" => self.tool_tas_disconnect(args),
            "zeff_link" => self.tool_link(args),
            "zeff_memory" => self.tool_memory(args),
            "zeff_graphics" => self.tool_graphics(),
            "zeff_sequence" => self.tool_sequence(args),
            "zeff_pair_start" => self.tool_pair_start(args),
            "zeff_pair_status" => self.tool_pair_status(),
            "zeff_pair_sequence" => self.tool_pair_sequence(args),
            "zeff_pair_gb_trade_fixture" => self.tool_pair_gb_trade_fixture(args),
            "zeff_pair_stop" => self.tool_pair_stop(),
            "zeff_stop" => self.tool_stop(),
            other => bail!("unknown tool: {other}"),
        };

        Ok(tool_result(result))
    }

    fn tool_start(&mut self, args: &Value) -> anyhow::Result<Value> {
        let rom_path = required_string(args, "rom_path")?;
        let addr = optional_string(args, "addr").unwrap_or_else(|| self.state.control_addr.clone());
        let release = optional_bool(args, "release").unwrap_or(false);
        let mute_audio = optional_bool(args, "mute_audio").unwrap_or(true);
        let wait_seconds = optional_u64(args, "wait_seconds").unwrap_or(45).min(180);
        let zeff_boy_exe = optional_string(args, "zeff_boy_exe");
        if let Some(repo_root) = optional_string(args, "repo_root") {
            self.state.repo_root = PathBuf::from(repo_root);
        }

        self.state.control_addr = addr.clone();

        if self.pair_is_running()? {
            bail!("a tracked Zeff Boy pair is already running");
        }

        if self.child_is_running()? {
            return Ok(json!({
                "started": false,
                "already_running": true,
                "addr": self.state.control_addr,
                "status": self.call_live(json!({ "command": "status" })).ok(),
            }));
        }

        let launch = LaunchOptions {
            release,
            mute_audio,
            zeff_boy_exe: zeff_boy_exe.as_deref(),
        };
        let child = self.spawn_instance(&rom_path, &addr, &launch)?;
        self.state.child = Some(child);

        let status = self.wait_for_status_at(&addr, Duration::from_secs(wait_seconds));
        Ok(json!({
            "started": true,
            "ready": status.is_ok(),
            "addr": self.state.control_addr,
            "repo_root_detected": repo_root_is_valid(&self.state.repo_root),
            "rom_path_redacted": true,
            "audio_muted": mute_audio,
            "status": status.ok(),
        }))
    }

    fn spawn_instance(
        &self,
        rom_path: &str,
        addr: &str,
        launch: &LaunchOptions<'_>,
    ) -> anyhow::Result<Child> {
        let mut command = if let Some(exe) = launch.zeff_boy_exe {
            let mut command = Command::new(exe);
            command.arg(rom_path);
            command
        } else {
            let mut command = Command::new("cargo");
            command.args(zeff_boy_cargo_args(&self.state.repo_root, launch.release));
            command.arg("--").arg(rom_path);
            command
        };

        command
            .current_dir(&self.state.repo_root)
            .env("ZEFF_REMOTE_CONTROL", addr)
            .env("ZEFF_REMOTE_AUTOMATION", "1")
            .env("ZEFF_MUTE_AUDIO", if launch.mute_audio { "1" } else { "0" })
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());

        command.spawn().context("failed to start zeff-boy")
    }

    fn child_is_running(&mut self) -> anyhow::Result<bool> {
        let Some(child) = &mut self.state.child else {
            return Ok(false);
        };
        if child.try_wait()?.is_none() {
            Ok(true)
        } else {
            self.state.child = None;
            Ok(false)
        }
    }

    fn wait_for_status_at(&self, addr: &str, timeout: Duration) -> anyhow::Result<Value> {
        let deadline = Instant::now() + timeout;
        loop {
            match self.call_live_at(addr, json!({ "command": "status" })) {
                Ok(status) => return Ok(status),
                Err(err) if Instant::now() >= deadline => return Err(err),
                Err(_) => std::thread::sleep(Duration::from_millis(250)),
            }
        }
    }

    fn tool_button(&self, args: &Value) -> anyhow::Result<Value> {
        let button = required_string(args, "button")?;
        let action = optional_string(args, "action").unwrap_or_else(|| "tap".to_string());
        let mut request = json!({
            "command": action,
            "button": button,
        });
        if let Some(frames) = optional_u64(args, "frames") {
            request["frames"] = json!(frames);
        }
        if let Some(player) = optional_u64(args, "player")
            .or_else(|| optional_u64(args, "controller"))
            .or_else(|| optional_u64(args, "port"))
        {
            request["player"] = json!(player);
        }
        self.call_live(request)
    }

    fn tool_zapper(&self, args: &Value) -> anyhow::Result<Value> {
        let mut request = json!({
            "command": "zapper",
            "enabled": optional_bool(args, "enabled").unwrap_or(true),
            "trigger": optional_bool(args, "trigger").unwrap_or(false),
            "hit": optional_bool(args, "hit").unwrap_or(false),
        });
        if let (Some(x), Some(y)) = (
            optional_u64(args, "x").or_else(|| optional_u64(args, "screen_x")),
            optional_u64(args, "y").or_else(|| optional_u64(args, "screen_y")),
        ) {
            request["x"] = json!(x);
            request["y"] = json!(y);
        }
        self.call_live(request)
    }

    fn tool_pause(&self, args: &Value) -> anyhow::Result<Value> {
        let action = optional_string(args, "action").unwrap_or_else(|| "toggle_pause".to_string());
        let command = match action.as_str() {
            "pause" => "pause",
            "resume" => "resume",
            "toggle" | "toggle_pause" => "toggle_pause",
            "frame_advance" | "step_frame" => "frame_advance",
            other => bail!("unknown pause action: {other}"),
        };
        self.call_live(json!({ "command": command }))
    }

    fn tool_speed(&self, args: &Value) -> anyhow::Result<Value> {
        let mode = required_string(args, "mode")?;
        let enabled = optional_bool(args, "enabled").unwrap_or(true);
        let command = match mode.as_str() {
            "slow" | "slow_motion" => "slow_motion",
            "fast" | "fast_forward" => "fast_forward",
            "uncapped" => "uncapped",
            other => bail!("unknown speed mode: {other}"),
        };
        self.call_live(json!({
            "command": command,
            "enabled": enabled,
        }))
    }

    fn tool_screenshot(&self, args: &Value) -> anyhow::Result<Value> {
        let mut request = json!({ "command": "screenshot" });
        if let Some(path) = optional_string(args, "path") {
            request["path"] = json!(path);
        }
        self.call_live(request)
    }

    fn tool_save_state(&self, args: &Value) -> anyhow::Result<Value> {
        let mut request = json!({ "command": "save_state" });
        if let Some(path) = optional_string(args, "path") {
            request["path"] = json!(path);
        }
        self.call_live(request)
    }

    fn tool_load_state(&self, args: &Value) -> anyhow::Result<Value> {
        let path = required_string(args, "path")?;
        self.call_live(json!({
            "command": "load_state",
            "path": path,
        }))
    }

    fn tool_state_slot(&self, args: &Value) -> anyhow::Result<Value> {
        let action = optional_string(args, "action").unwrap_or_else(|| "load".to_string());
        let command = match normalized_action(&action).as_str() {
            "save" | "savestate" | "statesave" => "save_state_slot",
            "load" | "loadstate" | "stateload" => "load_state_slot",
            other => bail!("unknown state-slot action: {other}"),
        };
        self.call_live(json!({
            "command": command,
            "slot": required_slot(args)?,
        }))
    }

    fn tool_replay(&self, args: &Value) -> anyhow::Result<Value> {
        let action = optional_string(args, "action").unwrap_or_else(|| "start".to_string());
        match normalized_action(&action).as_str() {
            "start" | "record" | "recordreplay" | "startrecording" => {
                let path = required_string(args, "path")?;
                self.call_live(json!({
                    "command": "record_replay",
                    "path": path,
                }))
            }
            "stop" | "stopreplay" | "stoprecording" => {
                self.call_live(json!({ "command": "stop_replay" }))
            }
            other => bail!("unknown replay action: {other}"),
        }
    }

    fn tool_tas_open(&self, args: &Value) -> anyhow::Result<Value> {
        self.call_live(json!({
            "command": "tas_open",
            "path": required_string(args, "path")?,
        }))
    }

    fn tool_tas_link(&self, args: &Value) -> anyhow::Result<Value> {
        self.call_live(json!({
            "command": "tas_link",
            "at_end": optional_bool(args, "at_end").unwrap_or(false),
            "record": optional_bool(args, "record").unwrap_or(false),
        }))
    }

    fn tool_tas_disconnect(&self, args: &Value) -> anyhow::Result<Value> {
        self.call_live(json!({
            "command": "tas_disconnect",
            "keep": optional_bool(args, "keep").unwrap_or(false),
        }))
    }

    fn tool_link(&self, args: &Value) -> anyhow::Result<Value> {
        let action = required_string(args, "action")?;
        let command = match normalized_action(&action).as_str() {
            "host" | "hostlink" => "host_link",
            "join" | "connect" | "connectlink" | "joinlink" => "join_link",
            "disconnect" | "disconnectlink" => "disconnect_link",
            other => bail!("unknown link action: {other}"),
        };
        let mut request = json!({ "command": command });
        if let Some(addr) = optional_string(args, "addr")
            .or_else(|| optional_string(args, "address"))
            .or_else(|| optional_string(args, "connect_addr"))
        {
            request["addr"] = json!(addr);
        }
        self.call_live(request)
    }

    fn tool_memory(&self, args: &Value) -> anyhow::Result<Value> {
        let space = optional_string(args, "space").unwrap_or_else(|| "cpu".to_string());
        let start = optional_u64(args, "start")
            .or_else(|| optional_u64(args, "address"))
            .unwrap_or(0);
        let length = optional_u64(args, "length").unwrap_or(64);
        self.call_live(json!({
            "command": "memory",
            "space": space,
            "start": start,
            "length": length,
        }))
    }

    fn tool_graphics(&self) -> anyhow::Result<Value> {
        self.call_live(json!({ "command": "graphics" }))
    }

    fn tool_stop(&mut self) -> anyhow::Result<Value> {
        let Some(mut child) = self.state.child.take() else {
            return Ok(json!({
                "stopped": false,
                "reason": "no tracked zeff-boy process",
            }));
        };

        if let Some(status) = child.try_wait()? {
            return Ok(json!({
                "stopped": false,
                "already_exited": true,
                "exit_status": status.to_string(),
            }));
        }

        child.kill().context("failed to stop zeff-boy")?;
        let status = child.wait().context("failed to wait for zeff-boy exit")?;
        Ok(json!({
            "stopped": true,
            "exit_status": status.to_string(),
        }))
    }

    fn call_live(&self, request: Value) -> anyhow::Result<Value> {
        self.call_live_at(&self.state.control_addr, request)
    }

    fn call_live_at(&self, addr: &str, request: Value) -> anyhow::Result<Value> {
        live_client::call_live(addr, request)
    }
}

fn required_slot(args: &Value) -> anyhow::Result<u8> {
    let slot = optional_u64(args, "slot").context("missing required integer argument: slot")?;
    if slot <= 9 {
        Ok(slot as u8)
    } else {
        bail!("slot must be between 0 and 9")
    }
}

fn zeff_boy_cargo_args(repo_root: &Path, release: bool) -> Vec<OsString> {
    let mut args = vec![
        OsString::from("run"),
        OsString::from("--manifest-path"),
        repo_root.join("Cargo.toml").into_os_string(),
        OsString::from("-p"),
        OsString::from("zeff-boy"),
        OsString::from("--bin"),
        OsString::from("zeff-boy"),
    ];
    if release {
        args.push(OsString::from("--release"));
    }
    args
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initialize_echoes_client_protocol_version() {
        let result = initialize_result(&json!({ "protocolVersion": "test-version" }));
        assert_eq!(result["protocolVersion"], "test-version");
        assert_eq!(result["serverInfo"]["name"], "zeff-mcp");
    }

    #[test]
    fn tools_include_start_and_screenshot() {
        let names = tools()
            .into_iter()
            .filter_map(|tool| tool["name"].as_str().map(ToOwned::to_owned))
            .collect::<Vec<_>>();
        assert!(names.contains(&"zeff_start".to_string()));
        assert!(names.contains(&"zeff_screenshot".to_string()));
        assert!(names.contains(&"zeff_save_state".to_string()));
        assert!(names.contains(&"zeff_load_state".to_string()));
        assert!(names.contains(&"zeff_state_slot".to_string()));
        assert!(names.contains(&"zeff_replay".to_string()));
        assert!(names.contains(&"zeff_tas_open".to_string()));
        assert!(names.contains(&"zeff_tas_status".to_string()));
        assert!(names.contains(&"zeff_tas_link".to_string()));
        assert!(names.contains(&"zeff_tas_record_frame".to_string()));
        assert!(names.contains(&"zeff_tas_disconnect".to_string()));
        assert!(names.contains(&"zeff_link".to_string()));
        assert!(names.contains(&"zeff_memory".to_string()));
        assert!(names.contains(&"zeff_graphics".to_string()));
        assert!(names.contains(&"zeff_sequence".to_string()));
        assert!(names.contains(&"zeff_pair_start".to_string()));
        assert!(names.contains(&"zeff_pair_status".to_string()));
        assert!(names.contains(&"zeff_pair_sequence".to_string()));
        assert!(names.contains(&"zeff_pair_gb_trade_fixture".to_string()));
        assert!(names.contains(&"zeff_pair_stop".to_string()));
        assert!(names.contains(&"zeff_stop".to_string()));
    }

    #[test]
    fn compiled_manifest_dir_resolves_repo_root() {
        let root = resolve_repo_root().expect("repo root should resolve from CARGO_MANIFEST_DIR");
        assert!(repo_root_is_valid(&root));
    }

    #[test]
    fn cargo_launcher_selects_zeff_boy_binary() {
        let args = zeff_boy_cargo_args(Path::new("repo"), false)
            .into_iter()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        assert!(
            args.windows(2)
                .any(|window| window == ["--bin", "zeff-boy"])
        );
    }
}
