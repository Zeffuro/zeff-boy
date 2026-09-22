use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU32};

use anyhow::{Result, ensure};
use serde_json::{Value, json};
use zeff_emu_common::audio_trace::{AudioTraceChip, ChipAudioTrace};

use super::{check_cancel, validate_provenance};
use crate::audio_discovery::vgm::capture::VgmCapture;

pub(super) struct CaptureEncoding<'a> {
    pub kind: &'a str,
    pub limitations: &'a [&'a str],
    pub capture: Option<&'a VgmCapture>,
    pub unavailable: Value,
}

pub(super) fn write_native_new<C: AudioTraceChip, W>(
    path: &Path,
    trace: &ChipAudioTrace<C, W>,
    context: Value,
    encoding: CaptureEncoding<'_>,
    cancel: &AtomicBool,
) -> Result<()>
where
    ChipAudioTrace<C, W>: serde::Serialize,
{
    check_cancel(cancel)?;
    validate_provenance(trace, &context)?;
    let events = serde_json::to_vec(trace)?;
    ensure!(
        events.len() <= 96 * 1024 * 1024,
        "audio trace exceeds its JSON size limit"
    );
    let mut artifacts = vec![json!({"path": "trace.json", "byte_len": events.len(),
        "sha256": zeff_firmware::sha256_hex(&events)})];
    let mut vgm = json!({"status": "unavailable", "unavailable": encoding.unavailable});
    if let Some(capture) = encoding.capture {
        artifacts.push(
            json!({"path": "capture.vgm", "byte_len": capture.bytes.len(),
            "sha256": zeff_firmware::sha256_hex(&capture.bytes)}),
        );
        vgm["status"] = json!("available");
        vgm["capture"] = serde_json::to_value(&capture.metadata)?;
    }
    let manifest = serde_json::to_vec_pretty(&json!({
        "schema": "zeff-audio-trace-capture/1", "kind": encoding.kind, "context": context,
        "vgm": vgm,
        "artifacts": artifacts,
        "limitations": encoding.limitations,
    }))?;
    let mut bundle = super::super::bundle::Bundle::new();
    bundle.add("trace.json", &events)?;
    if let Some(capture) = encoding.capture {
        bundle.add("capture.vgm", &capture.bytes)?;
    }
    bundle.add("manifest.json", &manifest)?;
    let bytes = bundle.finish()?;
    check_cancel(cancel)?;
    super::super::assets::publish_bytes(path, &bytes, cancel, &AtomicU32::new(0))
}
