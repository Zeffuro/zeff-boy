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
                    "frames": { "type": "integer", "default": 4 }
                },
                "required": ["button"]
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
                                    "description": "wait, tap, press, release, screenshot, save_state, load_state, memory, graphics, status, debug_info, pause, resume, frame_advance, speed"
                                },
                                "button": { "type": "string" },
                                "frames": { "type": "integer" },
                                "ms": { "type": "integer" },
                                "seconds": { "type": "integer" },
                                "path": { "type": "string" },
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
                "text": err.to_string()
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
