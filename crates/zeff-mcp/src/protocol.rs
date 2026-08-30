use serde_json::{Value, json};

pub(crate) const DEFAULT_CONTROL_ADDR: &str = "127.0.0.1:17684";

const DEFAULT_PROTOCOL_VERSION: &str = "2025-06-18";

pub(crate) fn initialize_result(params: &Value) -> Value {
    let protocol_version = params
        .get("protocolVersion")
        .and_then(Value::as_str)
        .unwrap_or(DEFAULT_PROTOCOL_VERSION);
    json!({
        "protocolVersion": protocol_version,
        "capabilities": {
            "tools": {}
        },
        "serverInfo": {
            "name": "zeff-mcp",
            "version": env!("CARGO_PKG_VERSION")
        }
    })
}

pub(crate) fn tools() -> Vec<Value> {
    vec![
        json!({
            "name": "zeff_start",
            "description": "Start Zeff Boy with a ROM and enable the local live-control socket. The ROM path is not echoed back.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "rom_path": { "type": "string" },
                    "addr": { "type": "string", "default": DEFAULT_CONTROL_ADDR },
                    "release": { "type": "boolean", "default": false },
                    "mute_audio": { "type": "boolean", "default": true },
                    "wait_seconds": { "type": "integer", "default": 45 },
                    "zeff_boy_exe": { "type": "string" },
                    "repo_root": {
                        "type": "string",
                        "description": "Optional Zeff Boy repository root override. Normally auto-detected."
                    }
                },
                "required": ["rom_path"]
            }
        }),
        json!({
            "name": "zeff_status",
            "description": "Read current Zeff Boy status without exposing ROM paths.",
            "inputSchema": empty_schema()
        }),
        json!({
            "name": "zeff_debug_info",
            "description": "Request cached CPU/debug information from the running emulator.",
            "inputSchema": empty_schema()
        }),
        json!({
            "name": "zeff_button",
            "description": "Press, release, or tap a joypad button.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "button": {
                        "type": "string",
                        "enum": ["a", "b", "start", "select", "up", "down", "left", "right"]
                    },
                    "action": {
                        "type": "string",
                        "enum": ["tap", "press", "release"],
                        "default": "tap"
                    },
                    "frames": { "type": "integer", "default": 4 },
                    "player": {
                        "type": "integer",
                        "enum": [1, 2],
                        "default": 1
                    }
                },
                "required": ["button"]
            }
        }),
        json!({
            "name": "zeff_zapper",
            "description": "Set NES Zapper/lightgun state for live-control automation.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "enabled": { "type": "boolean", "default": true },
                    "trigger": { "type": "boolean", "default": false },
                    "hit": { "type": "boolean", "default": false },
                    "x": {
                        "type": "integer",
                        "description": "NES screen X coordinate, 0-255"
                    },
                    "y": {
                        "type": "integer",
                        "description": "NES screen Y coordinate, 0-239"
                    },
                    "screen_x": {
                        "type": "integer",
                        "description": "Alias for x"
                    },
                    "screen_y": {
                        "type": "integer",
                        "description": "Alias for y"
                    }
                }
            }
        }),
        json!({
            "name": "zeff_pause",
            "description": "Pause, resume, toggle pause, or advance one frame.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": ["pause", "resume", "toggle", "frame_advance"],
                        "default": "toggle"
                    }
                }
            }
        }),
        json!({
            "name": "zeff_speed",
            "description": "Toggle slow motion, fast forward, or uncapped mode.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "mode": {
                        "type": "string",
                        "enum": ["slow_motion", "fast_forward", "uncapped"]
                    },
                    "enabled": { "type": "boolean", "default": true }
                },
                "required": ["mode"]
            }
        }),
        json!({
            "name": "zeff_screenshot",
            "description": "Save the current framebuffer as a PNG under ignored rom-tests/results/ or temp/.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Optional output path. Must be under rom-tests/results/ or temp/."
                    }
                }
            }
        }),
        json!({
            "name": "zeff_save_state",
            "description": "Save emulator state under ignored rom-tests/results/ or temp/.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Optional output path. Must be under rom-tests/results/ or temp/."
                    }
                }
            }
        }),
        json!({
            "name": "zeff_load_state",
            "description": "Load emulator state from ignored rom-tests/results/ or temp/.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "State path. Must be under rom-tests/results/ or temp/."
                    }
                },
                "required": ["path"]
            }
        }),
        json!({
            "name": "zeff_state_slot",
            "description": "Load or save one of the app's numbered state slots.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": ["load", "save"],
                        "default": "load"
                    },
                    "slot": {
                        "type": "integer",
                        "minimum": 0,
                        "maximum": 9
                    }
                },
                "required": ["slot"]
            }
        }),
        json!({
            "name": "zeff_replay",
            "description": "Start or stop replay recording without opening a file dialog.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": ["start", "stop"],
                        "default": "start"
                    },
                    "path": {
                        "type": "string",
                        "description": "Required when action=start."
                    }
                }
            }
        }),
        json!({
            "name": "zeff_tas_open",
            "description": "Open a local .ztas project in the running Zeff Boy instance without a file dialog.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": { "type": "string" }
                },
                "required": ["path"]
            }
        }),
        json!({
            "name": "zeff_tas_status",
            "description": "Read TAS project, selected cursor, frame count, and live-link state.",
            "inputSchema": empty_schema()
        }),
        json!({
            "name": "zeff_tas_link",
            "description": "Link the loaded game to the selected TAS cursor or the branch end. Recording is available only for compatible direct NES projects.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "at_end": { "type": "boolean", "default": false },
                    "record": { "type": "boolean", "default": false }
                }
            }
        }),
        json!({
            "name": "zeff_tas_disconnect",
            "description": "Disconnect TAS live control, either restoring the pre-link game state or keeping the linked position.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "keep": { "type": "boolean", "default": false }
                }
            }
        }),
        json!({
            "name": "zeff_tas_record_frame",
            "description": "Record exactly one frame from the current live controller state into a linked direct NES TAS project.",
            "inputSchema": empty_schema()
        }),
        json!({
            "name": "zeff_link",
            "description": "Host, join, or disconnect the local TCP link cable.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": ["host", "join", "disconnect"]
                    },
                    "addr": {
                        "type": "string",
                        "description": "Optional host/connect address, for example 127.0.0.1:8765."
                    }
                },
                "required": ["action"]
            }
        }),
        json!({
            "name": "zeff_memory",
            "description": "Read a compact byte range from CPU memory, VRAM, OAM, palette RAM, CHR/nametable data, or the framebuffer.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "space": {
                        "type": "string",
                        "default": "cpu",
                        "description": "cpu, vram, oam, palette, bg_palette, obj_palette, chr, nametable, or framebuffer"
                    },
                    "start": {
                        "type": "integer",
                        "default": 0
                    },
                    "address": {
                        "type": "integer",
                        "description": "Alias for start"
                    },
                    "length": {
                        "type": "integer",
                        "default": 64
                    }
                }
            }
        }),
        json!({
            "name": "zeff_graphics",
            "description": "Request compact PPU/graphics state and buffer digests from the running emulator.",
            "inputSchema": empty_schema()
        }),
        json!({
            "name": "zeff_sequence",
            "description": "Run multiple live-control actions in one MCP call, useful for input scripts and screenshot capture.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "steps": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "action": {
                                    "type": "string",
                                    "description": "wait, tap, press, release, zapper, screenshot, save_state, load_state, state_slot, replay, link, memory, graphics, status, debug_info, pause, resume, frame_advance, speed"
                                },
                                "button": { "type": "string" },
                                "player": { "type": "integer" },
                                "trigger": { "type": "boolean" },
                                "hit": { "type": "boolean" },
                                "x": { "type": "integer" },
                                "y": { "type": "integer" },
                                "screen_x": { "type": "integer" },
                                "screen_y": { "type": "integer" },
                                "frames": { "type": "integer" },
                                "ms": { "type": "integer" },
                                "seconds": { "type": "integer" },
                                "path": { "type": "string" },
                                "slot": { "type": "integer" },
                                "addr": { "type": "string" },
                                "space": { "type": "string" },
                                "start": { "type": "integer" },
                                "length": { "type": "integer" },
                                "mode": { "type": "string" },
                                "enabled": { "type": "boolean" }
                            },
                            "required": ["action"]
                        }
                    },
                    "stop_on_error": { "type": "boolean", "default": true }
                },
                "required": ["steps"]
            }
        }),
        json!({
            "name": "zeff_pair_start",
            "description": "Start two Zeff Boy instances with separate live-control sockets for link automation. ROM paths are not echoed back.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "rom_path": { "type": "string" },
                    "left_addr": { "type": "string", "default": DEFAULT_CONTROL_ADDR },
                    "right_addr": { "type": "string", "default": "127.0.0.1:17685" },
                    "release": { "type": "boolean", "default": false },
                    "mute_audio": { "type": "boolean", "default": true },
                    "wait_seconds": { "type": "integer", "default": 45 },
                    "zeff_boy_exe": { "type": "string" },
                    "repo_root": {
                        "type": "string",
                        "description": "Optional Zeff Boy repository root override. Normally auto-detected."
                    }
                },
                "required": ["rom_path"]
            }
        }),
        json!({
            "name": "zeff_pair_status",
            "description": "Read status from both tracked Zeff Boy link-test instances.",
            "inputSchema": empty_schema()
        }),
        json!({
            "name": "zeff_pair_sequence",
            "description": "Run live-control actions against the left, right, or both tracked link-test instances.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "steps": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "target": {
                                    "type": "string",
                                    "enum": ["left", "right", "both", "host", "join"],
                                    "default": "both"
                                },
                                "action": {
                                    "type": "string",
                                    "description": "wait, tap, press, release, zapper, screenshot, save_state, load_state, state_slot, replay, link, memory, graphics, status, debug_info, pause, resume, frame_advance, speed"
                                },
                                "button": { "type": "string" },
                                "player": { "type": "integer" },
                                "frames": { "type": "integer" },
                                "ms": { "type": "integer" },
                                "seconds": { "type": "integer" },
                                "path": { "type": "string" },
                                "slot": { "type": "integer" },
                                "addr": { "type": "string" },
                                "space": { "type": "string" },
                                "start": { "type": "integer" },
                                "length": { "type": "integer" },
                                "mode": { "type": "string" },
                                "enabled": { "type": "boolean" }
                            },
                            "required": ["action"]
                        }
                    },
                    "stop_on_error": { "type": "boolean", "default": true }
                },
                "required": ["steps"]
            }
        }),
        json!({
            "name": "zeff_pair_gb_trade_fixture",
            "description": "Drive a two-instance GB/GBC trade capture from a local trade-room state using live-control memory/framebuffer checks.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "state_path": {
                        "type": "string",
                        "default": "temp/pair-run/gb-trade-fixture.state"
                    },
                    "left_replay_path": {
                        "type": "string",
                        "default": "temp/pair-run/automated-recording/automated-host-trade.zrpl"
                    },
                    "right_replay_path": {
                        "type": "string",
                        "default": "temp/pair-run/automated-recording/automated-join-trade.zrpl"
                    },
                    "link_addr": {
                        "type": "string",
                        "default": "127.0.0.1:8765"
                    },
                    "left_party_index": {
                        "type": "integer",
                        "minimum": 0,
                        "maximum": 5,
                        "default": 2
                    },
                    "right_party_index": {
                        "type": "integer",
                        "minimum": 0,
                        "maximum": 5,
                        "default": 0
                    },
                    "record_replay": {
                        "type": "boolean",
                        "default": true
                    },
                    "fast_forward": {
                        "type": "boolean",
                        "default": true
                    },
                    "timeout_seconds": {
                        "type": "integer",
                        "default": 240
                    }
                }
            }
        }),
        json!({
            "name": "zeff_pair_stop",
            "description": "Stop the two Zeff Boy processes launched by zeff_pair_start.",
            "inputSchema": empty_schema()
        }),
        json!({
            "name": "zeff_stop",
            "description": "Stop the Zeff Boy process launched by zeff_start.",
            "inputSchema": empty_schema()
        }),
    ]
}

fn empty_schema() -> Value {
    json!({
        "type": "object",
        "properties": {}
    })
}

pub(crate) fn tool_result(result: anyhow::Result<Value>) -> Value {
    match result {
        Ok(value) => json!({
            "content": [{
                "type": "text",
                "text": serde_json::to_string_pretty(&value).unwrap_or_else(|_| value.to_string())
            }]
        }),
        Err(err) => json!({
            "isError": true,
            "content": [{
                "type": "text",
                "text": format!("{err:#}")
            }]
        }),
    }
}

pub(crate) fn jsonrpc_error(id: Value, code: i64, message: impl Into<String>) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": {
            "code": code,
            "message": message.into()
        }
    })
}
