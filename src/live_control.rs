use std::io::{BufRead, BufReader, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::Duration;

use serde::Deserialize;
use serde_json::{Value, json};
use zeff_gb_core::hardware::joypad::JoypadKey;

const ENV_VAR: &str = "ZEFF_REMOTE_CONTROL";
const DEFAULT_ADDR: &str = "127.0.0.1:17684";
const RESPONSE_TIMEOUT: Duration = Duration::from_secs(5);

pub(crate) struct LiveControl {
    rx: Option<Receiver<LiveRequest>>,
    addr: Option<SocketAddr>,
}

pub(crate) struct LiveRequest {
    pub(crate) command: LiveCommand,
    response_tx: Sender<LiveReply>,
}

#[derive(Clone, Copy)]
pub(crate) struct PendingButtonRelease {
    pub(crate) key: JoypadKey,
    pub(crate) frames_remaining: usize,
}

#[derive(Debug)]
pub(crate) enum LiveCommand {
    Status,
    DebugInfo,
    Pause,
    Resume,
    TogglePause,
    FrameAdvance,
    SetSlowMotion(bool),
    SetFastForward(bool),
    SetUncapped(bool),
    Button {
        key: JoypadKey,
        pressed: bool,
    },
    Tap {
        key: JoypadKey,
        frames: usize,
    },
    Screenshot {
        path: Option<PathBuf>,
    },
    SaveState {
        path: Option<PathBuf>,
    },
    LoadState {
        path: PathBuf,
    },
    MemoryRead {
        space: String,
        start: u32,
        length: usize,
    },
    GraphicsInfo,
}

pub(crate) enum LiveReply {
    Ok(Value),
    Error(String),
}

impl LiveControl {
    pub(crate) fn from_env() -> Self {
        let Some(addr) = configured_addr() else {
            return Self {
                rx: None,
                addr: None,
            };
        };

        if !addr.ip().is_loopback() {
            log::warn!(
                "{ENV_VAR} must bind to a loopback address; refusing live control on {addr}"
            );
            return Self {
                rx: None,
                addr: None,
            };
        }

        let listener = match TcpListener::bind(addr) {
            Ok(listener) => listener,
            Err(err) => {
                log::warn!("Failed to start live control on {addr}: {err}");
                return Self {
                    rx: None,
                    addr: None,
                };
            }
        };

        let actual_addr = listener.local_addr().ok();
        let (tx, rx) = mpsc::channel();
        spawn_listener(listener, tx);

        if let Some(addr) = actual_addr {
            log::info!("Live control listening on {addr}");
        }

        Self {
            rx: Some(rx),
            addr: actual_addr,
        }
    }

    pub(crate) fn try_recv(&self) -> Option<LiveRequest> {
        self.rx.as_ref()?.try_recv().ok()
    }

    pub(crate) fn is_enabled(&self) -> bool {
        self.rx.is_some()
    }

    pub(crate) fn addr(&self) -> Option<SocketAddr> {
        self.addr
    }
}

impl LiveRequest {
    pub(crate) fn respond_with(self, f: impl FnOnce(LiveCommand) -> LiveReply) {
        let reply = f(self.command);
        let _ = self.response_tx.send(reply);
    }
}

impl LiveReply {
    pub(crate) fn ok(value: Value) -> Self {
        Self::Ok(value)
    }

    pub(crate) fn error(message: impl Into<String>) -> Self {
        Self::Error(message.into())
    }
}

fn configured_addr() -> Option<SocketAddr> {
    let value = std::env::var(ENV_VAR).ok()?;
    let trimmed = value.trim();
    if trimmed.is_empty() || matches!(trimmed, "0" | "false" | "False" | "off" | "Off" | "no") {
        return None;
    }

    let addr = if matches!(trimmed, "1" | "true" | "True" | "on" | "On" | "yes") {
        DEFAULT_ADDR
    } else {
        trimmed
    };

    match addr.parse() {
        Ok(addr) => Some(addr),
        Err(err) => {
            log::warn!("Ignoring invalid {ENV_VAR} value {trimmed:?}: {err}");
            None
        }
    }
}

fn spawn_listener(listener: TcpListener, tx: Sender<LiveRequest>) {
    if let Err(err) = thread::Builder::new()
        .name("zeff-live-control".into())
        .spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else {
                    continue;
                };
                let tx = tx.clone();
                let _ = thread::Builder::new()
                    .name("zeff-live-control-client".into())
                    .spawn(move || handle_client(stream, tx));
            }
        })
    {
        log::warn!("Failed to spawn live control listener: {err}");
    }
}

fn handle_client(mut stream: TcpStream, tx: Sender<LiveRequest>) {
    let Ok(reader_stream) = stream.try_clone() else {
        let _ = writeln!(
            stream,
            "{}",
            wire_error(Value::Null, "failed to clone stream")
        );
        return;
    };
    let mut reader = BufReader::new(reader_stream);
    let mut line = String::new();

    loop {
        line.clear();
        let read = match reader.read_line(&mut line) {
            Ok(read) => read,
            Err(err) => {
                let _ = writeln!(
                    stream,
                    "{}",
                    wire_error(Value::Null, format!("failed to read request: {err}"))
                );
                break;
            }
        };
        if read == 0 {
            break;
        }

        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let (id, response) = dispatch_wire_request(trimmed, &tx);
        let wire = wire_reply(id, response);
        if writeln!(stream, "{wire}").is_err() {
            break;
        }
    }
}

fn dispatch_wire_request(line: &str, tx: &Sender<LiveRequest>) -> (Value, LiveReply) {
    let parsed = match parse_wire_request(line) {
        Ok(parsed) => parsed,
        Err(err) => return (Value::Null, LiveReply::error(err)),
    };

    let id = parsed.id.unwrap_or(Value::Null);
    let (response_tx, response_rx) = mpsc::channel();
    let request = LiveRequest {
        command: parsed.command,
        response_tx,
    };

    if tx.send(request).is_err() {
        return (id, LiveReply::error("live control is shutting down"));
    }

    match response_rx.recv_timeout(RESPONSE_TIMEOUT) {
        Ok(response) => (id, response),
        Err(_) => (id, LiveReply::error("live control response timed out")),
    }
}

fn wire_reply(id: Value, reply: LiveReply) -> Value {
    match reply {
        LiveReply::Ok(result) => json!({
            "id": id,
            "ok": true,
            "result": result,
        }),
        LiveReply::Error(error) => wire_error(id, error),
    }
}

fn wire_error(id: Value, error: impl Into<String>) -> Value {
    json!({
        "id": id,
        "ok": false,
        "error": error.into(),
    })
}

#[derive(Debug)]
struct ParsedWireRequest {
    id: Option<Value>,
    command: LiveCommand,
}

#[derive(Deserialize)]
struct WireRequest {
    id: Option<Value>,
    #[serde(alias = "method")]
    command: String,
    button: Option<String>,
    pressed: Option<bool>,
    enabled: Option<bool>,
    frames: Option<usize>,
    path: Option<String>,
    space: Option<String>,
    start: Option<Value>,
    address: Option<Value>,
    length: Option<usize>,
}

fn parse_wire_request(line: &str) -> Result<ParsedWireRequest, String> {
    let request: WireRequest =
        serde_json::from_str(line).map_err(|err| format!("invalid JSON request: {err}"))?;
    let command_name = normalized_name(&request.command);
    let command = match command_name.as_str() {
        "status" => LiveCommand::Status,
        "debug" | "debug_info" | "debuginfo" => LiveCommand::DebugInfo,
        "pause" => LiveCommand::Pause,
        "resume" => LiveCommand::Resume,
        "toggle_pause" | "togglepause" => LiveCommand::TogglePause,
        "frame_advance" | "frameadvance" | "step_frame" | "stepframe" => LiveCommand::FrameAdvance,
        "slow_motion" | "slowmotion" | "set_slow_motion" | "setslowmotion" => {
            LiveCommand::SetSlowMotion(required_enabled(&request)?)
        }
        "fast_forward" | "fastforward" | "set_fast_forward" | "setfastforward" => {
            LiveCommand::SetFastForward(required_enabled(&request)?)
        }
        "uncapped" | "set_uncapped" | "setuncapped" => {
            LiveCommand::SetUncapped(required_enabled(&request)?)
        }
        "press" => LiveCommand::Button {
            key: required_button(&request)?,
            pressed: true,
        },
        "release" => LiveCommand::Button {
            key: required_button(&request)?,
            pressed: false,
        },
        "button" | "set_button" | "setbutton" => LiveCommand::Button {
            key: required_button(&request)?,
            pressed: request.pressed.unwrap_or(true),
        },
        "tap" => LiveCommand::Tap {
            key: required_button(&request)?,
            frames: request.frames.unwrap_or(4).clamp(1, 60),
        },
        "screenshot" => LiveCommand::Screenshot {
            path: request.path.map(PathBuf::from),
        },
        "save_state" | "savestate" | "state_save" | "statesave" => LiveCommand::SaveState {
            path: request.path.map(PathBuf::from),
        },
        "load_state" | "loadstate" | "state_load" | "stateload" => LiveCommand::LoadState {
            path: request
                .path
                .as_ref()
                .map(PathBuf::from)
                .ok_or_else(|| "missing required field: path".to_string())?,
        },
        "memory" | "read_memory" | "readmemory" => LiveCommand::MemoryRead {
            space: request.space.clone().unwrap_or_else(|| "cpu".to_string()),
            start: optional_u32(&request.start)
                .or_else(|| optional_u32(&request.address))
                .unwrap_or(0),
            length: request.length.unwrap_or(64).clamp(1, 4096),
        },
        "graphics" | "graphics_info" | "graphicsinfo" | "ppu" => LiveCommand::GraphicsInfo,
        other => return Err(format!("unknown live control command: {other}")),
    };

    Ok(ParsedWireRequest {
        id: request.id,
        command,
    })
}

fn optional_u32(value: &Option<Value>) -> Option<u32> {
    let value = value.as_ref()?;
    match value {
        Value::Number(number) => number.as_u64().and_then(|value| u32::try_from(value).ok()),
        Value::String(text) => parse_u32_text(text).ok(),
        _ => None,
    }
}

fn parse_u32_text(text: &str) -> Result<u32, std::num::ParseIntError> {
    let trimmed = text.trim();
    if let Some(hex) = trimmed
        .strip_prefix("0x")
        .or_else(|| trimmed.strip_prefix("0X"))
        .or_else(|| trimmed.strip_prefix('$'))
    {
        u32::from_str_radix(hex, 16)
    } else {
        trimmed.parse()
    }
}

fn required_enabled(request: &WireRequest) -> Result<bool, String> {
    request
        .enabled
        .ok_or_else(|| "missing required boolean field: enabled".to_string())
}

fn required_button(request: &WireRequest) -> Result<JoypadKey, String> {
    let button = request
        .button
        .as_deref()
        .ok_or_else(|| "missing required field: button".to_string())?;
    parse_button(button).ok_or_else(|| format!("unknown button: {button}"))
}

fn parse_button(button: &str) -> Option<JoypadKey> {
    match normalized_name(button).as_str() {
        "right" => Some(JoypadKey::Right),
        "left" => Some(JoypadKey::Left),
        "up" => Some(JoypadKey::Up),
        "down" => Some(JoypadKey::Down),
        "a" => Some(JoypadKey::A),
        "b" => Some(JoypadKey::B),
        "select" => Some(JoypadKey::Select),
        "start" => Some(JoypadKey::Start),
        _ => None,
    }
}

fn normalized_name(s: &str) -> String {
    s.trim()
        .to_ascii_lowercase()
        .chars()
        .filter(|c| !matches!(c, '-' | ' ' | '.'))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_status_command() {
        let parsed = parse_wire_request(r#"{"id":1,"command":"status"}"#).unwrap();
        assert!(matches!(parsed.command, LiveCommand::Status));
        assert_eq!(parsed.id, Some(json!(1)));
    }

    #[test]
    fn parses_tap_button_with_default_frames() {
        let parsed = parse_wire_request(r#"{"command":"tap","button":"Start"}"#).unwrap();
        assert!(matches!(
            parsed.command,
            LiveCommand::Tap {
                key: JoypadKey::Start,
                frames: 4
            }
        ));
    }

    #[test]
    fn rejects_non_loopback_bind_addr() {
        let addr: SocketAddr = "0.0.0.0:17684".parse().unwrap();
        assert!(!addr.ip().is_loopback());
    }

    #[test]
    fn rejects_unknown_button() {
        let err = parse_wire_request(r#"{"command":"press","button":"coin"}"#).unwrap_err();
        assert!(err.contains("unknown button"));
    }

    #[test]
    fn parses_memory_command_with_hex_start() {
        let parsed = parse_wire_request(
            r#"{"command":"memory","space":"vram","start":"0x1800","length":32}"#,
        )
        .unwrap();
        assert!(matches!(
            parsed.command,
            LiveCommand::MemoryRead {
                ref space,
                start: 0x1800,
                length: 32
            } if space == "vram"
        ));
    }
}
