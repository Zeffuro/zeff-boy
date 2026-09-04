use std::path::PathBuf;

use crate::input::HostButton;
use serde::Deserialize;
use serde_json::Value;

use super::types::{LiveCommand, LiveMemorySpace, TasDigitalInput, TasRecordMode};

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
    control: Option<String>,
    keypad: Option<String>,
    #[serde(alias = "controller", alias = "port")]
    player: Option<u8>,
    pressed: Option<bool>,
    enabled: Option<bool>,
    frames: Option<usize>,
    path: Option<String>,
    replace_existing: Option<bool>,
    at_end: Option<bool>,
    record: Option<bool>,
    mode: Option<String>,
    boundary: Option<u64>,
    cursor: Option<u64>,
    branch_id: Option<String>,
    name: Option<String>,
    action: Option<String>,
    keep: Option<bool>,
    slot: Option<u8>,
    addr: Option<String>,
    space: Option<String>,
    start: Option<Value>,
    end: Option<Value>,
    count: Option<u64>,
    frame: Option<u64>,
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
        "keypad_press" | "keypadpress" | "press_keypad" | "presskeypad" => {
            LiveCommand::ColecoKeypad {
                player: optional_player(&request)?,
                key: required_coleco_keypad(&request)?,
                pressed: true,
            }
        }
        "keypad_release" | "keypadrelease" | "release_keypad" | "releasekeypad" => {
            LiveCommand::ColecoKeypad {
                player: optional_player(&request)?,
                key: required_coleco_keypad(&request)?,
                pressed: false,
            }
        }
        "keypad" | "set_keypad" | "setkeypad" => LiveCommand::ColecoKeypad {
            player: optional_player(&request)?,
            key: required_coleco_keypad(&request)?,
            pressed: request.pressed.unwrap_or(true),
        },
        "tap_keypad" | "tapkeypad" => LiveCommand::TapColecoKeypad {
            player: optional_player(&request)?,
            key: required_coleco_keypad(&request)?,
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
        "save_state_slot" | "savestateslot" | "state_save_slot" | "statesaveslot" => {
            LiveCommand::SaveStateSlot {
                slot: required_slot(&request)?,
            }
        }
        "load_state_slot" | "loadstateslot" | "state_load_slot" | "stateloadslot" => {
            LiveCommand::LoadStateSlot {
                slot: required_slot(&request)?,
            }
        }
        "start_replay_recording" | "startreplayrecording" | "record_replay" | "recordreplay" => {
            LiveCommand::StartReplayRecording {
                path: request
                    .path
                    .as_ref()
                    .map(PathBuf::from)
                    .ok_or_else(|| "missing required field: path".to_string())?,
            }
        }
        "stop_replay_recording" | "stopreplayrecording" | "stop_replay" | "stopreplay" => {
            LiveCommand::StopReplayRecording
        }
        "host_link" | "hostlink" | "link_host" | "linkhost" => {
            LiveCommand::HostLink { addr: request.addr }
        }
        "join_link" | "joinlink" | "connect_link" | "connectlink" | "link_join" | "linkjoin" => {
            LiveCommand::JoinLink { addr: request.addr }
        }
        "disconnect_link" | "disconnectlink" | "link_disconnect" | "linkdisconnect" => {
            LiveCommand::DisconnectLink
        }
        "tas_open" | "tasopen" | "tas_open_project" | "tasopenproject" => {
            LiveCommand::TasOpenProject {
                path: request
                    .path
                    .as_ref()
                    .map(PathBuf::from)
                    .ok_or_else(|| "missing required field: path".to_string())?,
            }
        }
        "tas_create" | "tascreate" | "tas_create_project" | "tascreateproject" => {
            LiveCommand::TasCreateProject {
                path: request
                    .path
                    .as_ref()
                    .map(PathBuf::from)
                    .ok_or_else(|| "missing required field: path".to_string())?,
                replace_existing: request.replace_existing.unwrap_or(false),
            }
        }
        "tas_status" | "tasstatus" => LiveCommand::TasStatus,
        "tas_select" | "tasselect" | "tas_select_boundary" | "tasselectboundary" => {
            LiveCommand::TasSelectBoundary {
                boundary: request
                    .boundary
                    .or(request.cursor)
                    .ok_or_else(|| "missing required integer field: boundary".to_string())?,
            }
        }
        "tas_select_range" | "tasselectrange" => {
            let start = required_u64_value(&request.start, "start")?;
            let end = required_u64_value(&request.end, "end")?;
            if start >= end {
                return Err("TAS selection range must satisfy start < end".to_owned());
            }
            if end > crate::tas_project::MAX_PROJECT_FRAMES {
                return Err(format!(
                    "TAS selection end must not exceed {}",
                    crate::tas_project::MAX_PROJECT_FRAMES
                ));
            }
            LiveCommand::TasSelectRange { start, end }
        }
        "tas_delete_selected_frames"
        | "tasdeleteselectedframes"
        | "tas_delete_range"
        | "tasdeleterange" => LiveCommand::TasDeleteSelectedFrames,
        "tas_insert_neutral_frames" | "tasinsertneutralframes" => {
            let boundary = request
                .boundary
                .or(request.cursor)
                .ok_or_else(|| "missing required integer field: boundary".to_string())?;
            if boundary > crate::tas_project::MAX_PROJECT_FRAMES {
                return Err(format!(
                    "TAS insertion boundary must not exceed {}",
                    crate::tas_project::MAX_PROJECT_FRAMES
                ));
            }
            LiveCommand::TasInsertNeutralFrames {
                boundary,
                count: required_tas_frame_count(&request)?,
            }
        }
        "tas_set_input" | "tassetinput" | "tas_set_digital_input" | "tassetdigitalinput" => {
            let frame = request
                .frame
                .ok_or_else(|| "missing required integer field: frame".to_owned())?;
            if frame >= crate::tas_project::MAX_PROJECT_FRAMES {
                return Err(format!(
                    "TAS input frame must be less than {}",
                    crate::tas_project::MAX_PROJECT_FRAMES
                ));
            }
            LiveCommand::TasSetDigitalInput {
                frame,
                player: optional_player(&request)?,
                input: required_tas_digital_input(&request)?,
                pressed: request
                    .pressed
                    .ok_or_else(|| "missing required boolean field: pressed".to_owned())?,
            }
        }
        "tas_go_to_selection" | "tasgotoselection" => LiveCommand::TasGoToSelection,
        "tas_fork_branch" | "tasforkbranch" | "tas_create_branch" | "tascreatebranch" => {
            LiveCommand::TasForkBranch {
                id: request
                    .branch_id
                    .clone()
                    .ok_or_else(|| "missing required field: branch_id".to_string())?,
                name: request.name.clone(),
            }
        }
        "tas_recording" | "tasrecording" | "tas_realtime_recording" | "tasrealtimerecording" => {
            LiveCommand::TasSetRealtimeRecording {
                active: tas_recording_active(&request)?,
            }
        }
        "tas_playback" | "tasplayback" | "tas_play" | "tasplay" => LiveCommand::TasSetPlayback {
            active: tas_playback_active(&request)?,
        },
        "tas_link" | "taslink" | "tas_connect" | "tasconnect" => LiveCommand::TasLink {
            at_end: request.at_end.unwrap_or(false),
            record: request.record.unwrap_or(false),
        },
        "tas_reload_game" | "tasreloadgame" | "tas_reload" | "tasreload" => {
            LiveCommand::TasReloadGame
        }
        "tas_record_frame" | "tasrecordframe" | "tas_record_one" | "tasrecordone" => {
            LiveCommand::TasRecordFrame {
                mode: tas_record_mode(&request)?,
            }
        }
        "tas_disconnect" | "tasdisconnect" => LiveCommand::TasDisconnect {
            keep: request.keep.unwrap_or(false),
        },
        "memory" | "read_memory" | "readmemory" => LiveCommand::MemoryRead {
            space: request
                .space
                .map(LiveMemorySpace::from_wire)
                .unwrap_or_default(),
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

fn tas_record_mode(request: &WireRequest) -> Result<TasRecordMode, String> {
    match request.mode.as_deref().unwrap_or("replace") {
        "replace" => Ok(TasRecordMode::Replace),
        "insert" => Ok(TasRecordMode::Insert),
        _ => Err("TAS record mode must be \"replace\" or \"insert\"".to_owned()),
    }
}

fn required_tas_frame_count(request: &WireRequest) -> Result<u64, String> {
    let count = request
        .count
        .ok_or_else(|| "missing required integer field: count".to_owned())?;
    if (1..=crate::tas_project::MAX_PROJECT_FRAMES).contains(&count) {
        Ok(count)
    } else {
        Err(format!(
            "TAS frame count must be between 1 and {}",
            crate::tas_project::MAX_PROJECT_FRAMES
        ))
    }
}

fn tas_recording_active(request: &WireRequest) -> Result<bool, String> {
    match request.action.as_deref().unwrap_or("start") {
        "start" => Ok(true),
        "stop" => Ok(false),
        _ => Err("TAS recording action must be \"start\" or \"stop\"".to_owned()),
    }
}

fn tas_playback_active(request: &WireRequest) -> Result<bool, String> {
    match request.action.as_deref().unwrap_or("start") {
        "start" | "play" => Ok(true),
        "pause" | "stop" => Ok(false),
        _ => Err("TAS playback action must be \"start\" or \"pause\"".to_owned()),
    }
}

fn required_slot(request: &WireRequest) -> Result<u8, String> {
    let slot = request
        .slot
        .ok_or_else(|| "missing required field: slot".to_string())?;
    if slot <= 9 {
        Ok(slot)
    } else {
        Err("slot must be between 0 and 9".to_string())
    }
}

fn optional_u32(value: &Option<Value>) -> Option<u32> {
    let value = value.as_ref()?;
    match value {
        Value::Number(number) => number.as_u64().and_then(|value| u32::try_from(value).ok()),
        Value::String(text) => parse_u32_text(text).ok(),
        _ => None,
    }
}

fn required_u64_value(value: &Option<Value>, field: &str) -> Result<u64, String> {
    value
        .as_ref()
        .and_then(|value| match value {
            Value::Number(number) => number.as_u64(),
            Value::String(text) => text.trim().parse().ok(),
            _ => None,
        })
        .ok_or_else(|| format!("missing or invalid non-negative integer field: {field}"))
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
    if (1..=5).contains(&player) {
        Ok(player)
    } else {
        Err("player must be 1 through 5".to_string())
    }
}

fn required_button(request: &WireRequest) -> Result<HostButton, String> {
    let button = request
        .button
        .as_deref()
        .ok_or_else(|| "missing required field: button".to_string())?;
    HostButton::from_name(button).ok_or_else(|| format!("unknown button: {button}"))
}

fn required_tas_digital_input(request: &WireRequest) -> Result<TasDigitalInput, String> {
    let control = request
        .control
        .as_deref()
        .or(request.button.as_deref())
        .ok_or_else(|| "missing required field: control".to_owned())?;
    let normalized = normalized_name(control);
    let input = match normalized.as_str() {
        "right" | "dpadright" => TasDigitalInput::Dpad(1 << 0),
        "left" | "dpadleft" => TasDigitalInput::Dpad(1 << 1),
        "up" | "dpadup" => TasDigitalInput::Dpad(1 << 2),
        "down" | "dpaddown" => TasDigitalInput::Dpad(1 << 3),
        "a" | "buttona" | "i" => TasDigitalInput::Buttons(1 << 0),
        "b" | "buttonb" | "ii" => TasDigitalInput::Buttons(1 << 1),
        "select" | "sel" => TasDigitalInput::Buttons(1 << 2),
        "start" | "st" | "run" => TasDigitalInput::Buttons(1 << 3),
        "l" | "buttonl" | "iii" => TasDigitalInput::Buttons(1 << 4),
        "r" | "buttonr" | "iv" => TasDigitalInput::Buttons(1 << 5),
        "v" => TasDigitalInput::Buttons(1 << 6),
        "vi" => TasDigitalInput::Buttons(1 << 7),
        name => parse_raw_tas_digital_input(name)
            .ok_or_else(|| format!("unknown TAS digital control: {control}"))?,
    };
    Ok(input)
}

fn parse_raw_tas_digital_input(name: &str) -> Option<TasDigitalInput> {
    let (field, bit) = if let Some(bit) = name.strip_prefix("dpad") {
        (DigitalInputField::Dpad, bit)
    } else if let Some(bit) = name.strip_prefix('d') {
        (DigitalInputField::Dpad, bit)
    } else if let Some(bit) = name.strip_prefix("button") {
        (DigitalInputField::Buttons, bit)
    } else {
        (DigitalInputField::Buttons, name.strip_prefix('b')?)
    };
    let bit = bit.parse::<u8>().ok()?;
    let mask = 1_u8.checked_shl(u32::from(bit))?;
    match field {
        DigitalInputField::Buttons => Some(TasDigitalInput::Buttons(mask)),
        DigitalInputField::Dpad => Some(TasDigitalInput::Dpad(mask)),
    }
}

enum DigitalInputField {
    Buttons,
    Dpad,
}

fn required_coleco_keypad(request: &WireRequest) -> Result<u8, String> {
    let key = request
        .keypad
        .as_deref()
        .ok_or_else(|| "missing required field: keypad".to_string())?;
    match normalized_name(key).as_str() {
        "0" | "keypad0" | "kp0" => Ok(0),
        "1" | "keypad1" | "kp1" => Ok(1),
        "2" | "keypad2" | "kp2" => Ok(2),
        "3" | "keypad3" | "kp3" => Ok(3),
        "4" | "keypad4" | "kp4" => Ok(4),
        "5" | "keypad5" | "kp5" => Ok(5),
        "6" | "keypad6" | "kp6" => Ok(6),
        "7" | "keypad7" | "kp7" => Ok(7),
        "8" | "keypad8" | "kp8" => Ok(8),
        "9" | "keypad9" | "kp9" => Ok(9),
        "*" | "star" | "asterisk" | "keypadstar" | "kpstar" => Ok(10),
        "#" | "pound" | "hash" | "keypadpound" | "kppound" => Ok(11),
        _ => Err(format!("unknown ColecoVision keypad key: {key}")),
    }
}

fn normalized_name(s: &str) -> String {
    s.trim()
        .to_ascii_lowercase()
        .chars()
        .filter(|c| !matches!(c, '-' | ' ' | '.'))
        .collect()
}
