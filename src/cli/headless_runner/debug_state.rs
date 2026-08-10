use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

use crate::cli::types::HeadlessOptions;

use super::{InputMasks, StuckReport};

mod gb;
mod gba;
mod nes;
mod sega8;

pub(super) use gb::gb_debug_state;
pub(super) use gba::{dump_gba_memory_snapshots, gba_debug_state, gba_wait_classification};
pub(super) use nes::nes_debug_state;
pub(super) use sega8::{Sega8DebugStateRequest, sega8_debug_state};
pub(super) fn emit_debug_state(
    opts: &HeadlessOptions,
    value: serde_json::Value,
) -> anyhow::Result<()> {
    if let Some(path) = &opts.debug_state_path {
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, serde_json::to_vec_pretty(&value)?)?;
        println!("[headless] debug-state={}", path.display());
    }
    if opts.print_debug_state {
        println!("[headless-debug] {}", serde_json::to_string(&value)?);
    }
    Ok(())
}

pub(super) fn write_audio_dump_f32le(
    path: &Path,
    samples: &[f32],
    sample_rate: u32,
) -> anyhow::Result<()> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        std::fs::create_dir_all(parent)?;
    }
    let file = File::create(path)?;
    let mut writer = BufWriter::new(file);
    for &sample in samples {
        writer.write_all(&sample.to_le_bytes())?;
    }
    writer.flush()?;
    println!(
        "[headless] audio-dump={} format=f32le channels=2 sample_rate={} samples={}",
        path.display(),
        sample_rate,
        samples.len()
    );
    Ok(())
}

fn stuck_report_json(report: Option<&StuckReport>) -> serde_json::Value {
    match report {
        Some(report) => serde_json::json!({
            "detected": true,
            "frame": report.frame,
            "window_frames": report.window_frames,
            "unique_pcs": report.unique_pcs,
            "framebuffer_changed": report.framebuffer_changed,
            "first_pc": report.first_pc,
            "last_pc": report.last_pc,
            "classification": report.classification,
            "expected_wait": report.expected_wait,
        }),
        None => serde_json::json!({ "detected": false }),
    }
}

fn input_json(input: InputMasks) -> serde_json::Value {
    serde_json::json!({
        "buttons": input.buttons,
        "dpad": input.dpad,
        "reset": input.reset,
        "buttons_hex": format!("{:02X}", input.buttons),
        "dpad_hex": format!("{:02X}", input.dpad),
        "zapper": {
            "enabled": input.zapper_enabled,
            "trigger": input.zapper_trigger,
            "hit": input.zapper_hit,
            "screen_pos": input.zapper_screen_pos.map(|(x, y)| serde_json::json!({ "x": x, "y": y })),
        },
    })
}

fn input_schedule_json(opts: &HeadlessOptions) -> serde_json::Value {
    let events = opts
        .input_events
        .iter()
        .map(|event| {
            serde_json::json!({
                "start_frame": event.start_frame,
                "end_frame": event.end_frame,
                "buttons": event.buttons,
                "dpad": event.dpad,
                "reset": event.reset,
                "buttons_hex": format!("{:02X}", event.buttons),
                "dpad_hex": format!("{:02X}", event.dpad),
            })
        })
        .collect::<Vec<_>>();
    let zapper_events = opts
        .zapper_events
        .iter()
        .map(|event| {
            serde_json::json!({
                "start_frame": event.start_frame,
                "end_frame": event.end_frame,
                "x": event.x,
                "y": event.y,
                "trigger": event.trigger,
                "hit": event.hit,
            })
        })
        .collect::<Vec<_>>();

    serde_json::json!({
        "event_count": events.len(),
        "events": events,
        "zapper_event_count": zapper_events.len(),
        "zapper_events": zapper_events,
    })
}

fn screenshot_json(path: Option<&PathBuf>) -> serde_json::Value {
    match path {
        Some(path) => serde_json::json!({ "written": true, "path": path.display().to_string() }),
        None => serde_json::json!({ "written": false }),
    }
}

fn hex_bytes(bytes: &[u8]) -> Vec<String> {
    bytes.iter().map(|byte| format!("{byte:02X}")).collect()
}

fn decode_printable_ascii(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|&byte| match byte {
            b'\n' | b'\r' | b'\t' => byte as char,
            0x20..=0x7E => byte as char,
            _ => '.',
        })
        .collect()
}
