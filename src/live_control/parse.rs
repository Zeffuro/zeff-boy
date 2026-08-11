use std::path::PathBuf;

use serde::Deserialize;
use serde_json::Value;
use zeff_gb_core::hardware::joypad::JoypadKey;

use super::types::LiveCommand;

#[derive(Debug)]
pub(super) struct ParsedWireRequest {
    pub(super) id: Option<Value>,
    pub(super) command: LiveCommand,
}

#[derive(Deserialize)]
struct WireRequest {
    id: Option<Value>,
    #[serde(alias = "method")]
    command: String,
    button: Option<String>,
    #[serde(alias = "controller", alias = "port")]
    player: Option<u8>,
    pressed: Option<bool>,
    enabled: Option<bool>,
    frames: Option<usize>,
    path: Option<String>,
    space: Option<String>,
    start: Option<Value>,
    address: Option<Value>,
    length: Option<usize>,
    x: Option<Value>,
    y: Option<Value>,
    screen_x: Option<Value>,
    screen_y: Option<Value>,
    trigger: Option<bool>,
    hit: Option<bool>,
}

pub(super) fn parse_wire_request(line: &str) -> Result<ParsedWireRequest, String> {
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
            player: optional_player(&request)?,
            key: required_button(&request)?,
            pressed: true,
        },
        "release" => LiveCommand::Button {
            player: optional_player(&request)?,
            key: required_button(&request)?,
            pressed: false,
        },
        "button" | "set_button" | "setbutton" => LiveCommand::Button {
            player: optional_player(&request)?,
            key: required_button(&request)?,
            pressed: request.pressed.unwrap_or(true),
        },
        "tap" => LiveCommand::Tap {
            player: optional_player(&request)?,
            key: required_button(&request)?,
            frames: request.frames.unwrap_or(4).clamp(1, 60),
        },
        "zapper" | "set_zapper" | "setzapper" | "lightgun" | "set_lightgun" | "setlightgun" => {
            LiveCommand::Zapper {
                enabled: request.enabled.unwrap_or(true),
                trigger: request.trigger.unwrap_or(false),
                hit: request.hit.unwrap_or(false),
                screen_pos: optional_screen_pos(&request),
            }
        }
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

fn optional_u16(value: &Option<Value>) -> Option<u16> {
    optional_u32(value).and_then(|value| u16::try_from(value).ok())
}

fn optional_screen_pos(request: &WireRequest) -> Option<(u16, u16)> {
    let x = optional_u16(&request.x).or_else(|| optional_u16(&request.screen_x))?;
    let y = optional_u16(&request.y).or_else(|| optional_u16(&request.screen_y))?;
    Some((x, y))
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

fn optional_player(request: &WireRequest) -> Result<u8, String> {
    let player = request.player.unwrap_or(1);
    if matches!(player, 1 | 2) {
        Ok(player)
    } else {
        Err("player must be 1 or 2".to_string())
    }
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
