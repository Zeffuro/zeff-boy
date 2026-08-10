use std::net::SocketAddr;

use serde_json::json;
use zeff_gb_core::hardware::joypad::JoypadKey;

use super::parse::parse_wire_request;
use super::types::LiveCommand;

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
    let parsed =
        parse_wire_request(r#"{"command":"memory","space":"vram","start":"0x1800","length":32}"#)
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

#[test]
fn parses_zapper_command_with_screen_position() {
    let parsed = parse_wire_request(
        r#"{"command":"zapper","enabled":true,"trigger":true,"hit":true,"screen_x":128,"screen_y":96}"#,
    )
    .unwrap();
    assert!(matches!(
        parsed.command,
        LiveCommand::Zapper {
            enabled: true,
            trigger: true,
            hit: true,
            screen_pos: Some((128, 96))
        }
    ));
}
