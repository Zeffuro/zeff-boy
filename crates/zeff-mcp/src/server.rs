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
mod tas;

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
            "zeff_tas_create" => self.tool_tas_create(args),
            "zeff_tas_open" => self.tool_tas_open(args),
            "zeff_tas_status" => self.call_live(json!({ "command": "tas_status" })),
            "zeff_tas_select" => self.tool_tas_select(args),
            "zeff_tas_select_range" => self.tool_tas_select_range(args),
            "zeff_tas_delete_selected_frames" => {
                self.call_live(json!({ "command": "tas_delete_selected_frames" }))
            }
            "zeff_tas_insert_neutral_frames" => self.tool_tas_insert_neutral_frames(args),
            "zeff_tas_set_input" => self.tool_tas_set_input(args),
            "zeff_tas_go_to_selection" => {
                self.call_live(json!({ "command": "tas_go_to_selection" }))
            }
            "zeff_tas_fork_branch" => self.tool_tas_fork_branch(args),
            "zeff_tas_recording" => self.tool_tas_recording(args),
            "zeff_tas_playback" => self.tool_tas_playback(args),
            "zeff_tas_link" => self.tool_tas_link(args),
            "zeff_tas_connect" => self.tool_tas_link(args),
            "zeff_tas_reload_game" => self.call_live(json!({ "command": "tas_reload_game" })),
            "zeff_tas_record_frame" => self.tool_tas_record_frame(args),
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

fn is_tas_digital_control(control: &str) -> bool {
    matches!(
        normalized_action(control).as_str(),
        "right"
            | "left"
            | "up"
            | "down"
            | "a"
            | "b"
            | "select"
            | "start"
            | "l"
            | "r"
            | "i"
            | "ii"
            | "iii"
            | "iv"
            | "v"
            | "vi"
            | "run"
            | "d0"
            | "d1"
            | "d2"
            | "d3"
            | "d4"
            | "d5"
            | "d6"
            | "d7"
            | "b0"
            | "b1"
            | "b2"
            | "b3"
            | "b4"
            | "b5"
            | "b6"
            | "b7"
    )
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
        assert!(names.contains(&"zeff_tas_create".to_string()));
        assert!(names.contains(&"zeff_tas_open".to_string()));
        assert!(names.contains(&"zeff_tas_status".to_string()));
        assert!(names.contains(&"zeff_tas_select".to_string()));
        assert!(names.contains(&"zeff_tas_select_range".to_string()));
        assert!(names.contains(&"zeff_tas_delete_selected_frames".to_string()));
        assert!(names.contains(&"zeff_tas_insert_neutral_frames".to_string()));
        assert!(names.contains(&"zeff_tas_set_input".to_string()));
        assert!(names.contains(&"zeff_tas_go_to_selection".to_string()));
        assert!(names.contains(&"zeff_tas_fork_branch".to_string()));
        assert!(names.contains(&"zeff_tas_recording".to_string()));
        assert!(names.contains(&"zeff_tas_playback".to_string()));
        assert!(names.contains(&"zeff_tas_link".to_string()));
        assert!(names.contains(&"zeff_tas_connect".to_string()));
        assert!(names.contains(&"zeff_tas_reload_game".to_string()));
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
    fn tas_record_frame_advertises_replace_and_insert_modes() {
        let tool = tools()
            .into_iter()
            .find(|tool| tool["name"] == "zeff_tas_record_frame")
            .expect("TAS record tool must be advertised");
        assert_eq!(
            tool["inputSchema"]["properties"]["mode"]["default"],
            "replace"
        );
        assert_eq!(
            tool["inputSchema"]["properties"]["mode"]["enum"],
            json!(["replace", "insert"])
        );
    }

    #[test]
    fn tas_create_requires_a_path_and_explicit_replacement() {
        let tool = tools()
            .into_iter()
            .find(|tool| tool["name"] == "zeff_tas_create")
            .expect("TAS create tool must be advertised");
        assert_eq!(tool["inputSchema"]["required"], json!(["path"]));
        assert_eq!(
            tool["inputSchema"]["properties"]["replace_existing"]["default"],
            false
        );
    }

    #[test]
    fn tas_go_to_selection_has_an_empty_schema() {
        let tool = tools()
            .into_iter()
            .find(|tool| tool["name"] == "zeff_tas_go_to_selection")
            .expect("TAS go-to-selection tool must be advertised");
        assert_eq!(
            tool["inputSchema"],
            json!({ "type": "object", "properties": {} })
        );
    }

    #[test]
    fn tas_range_edit_tools_advertise_half_open_bounded_inputs() {
        let tools = tools();
        let select = tools
            .iter()
            .find(|tool| tool["name"] == "zeff_tas_select_range")
            .expect("TAS range selection tool must be advertised");
        assert_eq!(select["inputSchema"]["required"], json!(["start", "end"]));
        assert!(
            select["description"]
                .as_str()
                .is_some_and(|description| description.contains("[start, end)"))
        );

        let delete = tools
            .iter()
            .find(|tool| tool["name"] == "zeff_tas_delete_selected_frames")
            .expect("TAS range deletion tool must be advertised");
        assert_eq!(
            delete["inputSchema"],
            json!({ "type": "object", "properties": {} })
        );

        let insert = tools
            .iter()
            .find(|tool| tool["name"] == "zeff_tas_insert_neutral_frames")
            .expect("TAS neutral insertion tool must be advertised");
        assert_eq!(
            insert["inputSchema"]["required"],
            json!(["boundary", "count"])
        );
        assert_eq!(insert["inputSchema"]["properties"]["count"]["minimum"], 1);
        assert_eq!(
            insert["inputSchema"]["properties"]["count"]["maximum"],
            1_000_000_000_u64
        );
    }

    #[test]
    fn tas_set_input_advertises_absolute_bounded_input() {
        let tool = tools()
            .into_iter()
            .find(|tool| tool["name"] == "zeff_tas_set_input")
            .expect("TAS input tool must be advertised");
        assert_eq!(
            tool["inputSchema"]["required"],
            json!(["frame", "control", "pressed"])
        );
        assert_eq!(tool["inputSchema"]["properties"]["player"]["minimum"], 1);
        assert_eq!(tool["inputSchema"]["properties"]["player"]["maximum"], 5);
        assert_eq!(
            tool["inputSchema"]["properties"]["frame"]["maximum"],
            999_999_999_u64
        );
        assert!(
            tool["description"]
                .as_str()
                .is_some_and(|description| description.contains("no-op"))
        );
    }

    #[test]
    fn tas_range_edit_routes_reject_invalid_arguments_before_live_control() {
        let server = Server::new();
        assert!(
            server
                .tool_tas_select_range(&json!({ "start": 7, "end": 7 }))
                .unwrap_err()
                .to_string()
                .contains("start < end")
        );
        assert!(
            server
                .tool_tas_insert_neutral_frames(&json!({ "boundary": 7, "count": 0 }))
                .unwrap_err()
                .to_string()
                .contains("between 1 and 1000000000")
        );
        assert!(
            server
                .tool_tas_set_input(
                    &json!({ "frame": 7, "player": 6, "control": "a", "pressed": true })
                )
                .unwrap_err()
                .to_string()
                .contains("player must be 1 through 5")
        );
        assert!(
            server
                .tool_tas_set_input(&json!({ "frame": 7, "control": "unknown", "pressed": true }))
                .unwrap_err()
                .to_string()
                .contains("unknown TAS digital control")
        );
    }

    #[test]
    fn tas_fork_branch_requires_an_id() {
        let tool = tools()
            .into_iter()
            .find(|tool| tool["name"] == "zeff_tas_fork_branch")
            .expect("TAS branch tool must be advertised");
        assert_eq!(tool["inputSchema"]["required"], json!(["id"]));
    }

    #[test]
    fn tas_recording_advertises_start_and_stop_actions() {
        let tool = tools()
            .into_iter()
            .find(|tool| tool["name"] == "zeff_tas_recording")
            .expect("TAS recording tool must be advertised");
        assert_eq!(
            tool["inputSchema"]["properties"]["action"]["enum"],
            json!(["start", "stop"])
        );
    }

    #[test]
    fn tas_reload_and_restore_descriptions_publish_transactional_rollback() {
        let tools = tools();
        let reload = tools
            .iter()
            .find(|tool| tool["name"] == "zeff_tas_reload_game")
            .expect("TAS reload tool must be advertised");
        assert!(reload["description"].as_str().is_some_and(|description| {
            description.contains("park the current game")
                && description.contains("restores the exact parked game")
        }));
        let disconnect = tools
            .iter()
            .find(|tool| tool["name"] == "zeff_tas_disconnect")
            .expect("TAS disconnect tool must be advertised");
        assert!(
            disconnect["description"]
                .as_str()
                .is_some_and(|description| {
                    description.contains("exact parked pre-reload game")
                })
        );
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
