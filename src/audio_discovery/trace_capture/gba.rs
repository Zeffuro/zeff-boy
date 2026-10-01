use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU32};

use anyhow::{Result, ensure};
use serde_json::{Value, json};
use zeff_emu_common::audio_trace::GbaAudioTrace;

mod replay;

pub(crate) fn write_gba_new(
    path: &Path,
    trace: &GbaAudioTrace,
    source: &[u8],
    context: Value,
    cancel: &AtomicBool,
) -> Result<()> {
    super::check_cancel(cancel)?;
    super::validate_provenance(trace, &context)?;
    ensure!(
        context["system"] == "gba"
            && context["firmware"].is_null()
            && context["settings"]["system_start"] == "post_bios_core_reset"
            && context["source"]["loaded_media"]["sha256"] == zeff_firmware::sha256_hex(source)
            && context["source"]["loaded_media"]["byte_len"] == source.len(),
        "GBA FIFO capture requires its exact source and post-BIOS reset context"
    );
    let feed = replay::replay(trace, source, cancel)?;
    let events = serde_json::to_vec(trace)?;
    ensure!(
        events.len() <= 96 * 1024 * 1024,
        "GBA FIFO trace exceeds its JSON limit"
    );
    let provenance = serde_json::to_vec(&feed.metadata)?;
    let entries = [
        ("trace.json", events.as_slice()),
        ("feed.json", provenance.as_slice()),
        ("fifo-a.s8", feed.samples[0].as_slice()),
        ("fifo-b.s8", feed.samples[1].as_slice()),
    ];
    let artifacts: Vec<_> = entries
        .iter()
        .map(|(name, bytes)| {
            json!({
                "path": name, "byte_len": bytes.len(), "sha256": zeff_firmware::sha256_hex(bytes),
            })
        })
        .collect();
    let manifest = serde_json::to_vec_pretty(&json!({
        "schema": "zeff-gba-fifo-capture/1",
        "kind": "reset_to_end_direct_sound_feed",
        "context": context,
        "artifacts": artifacts,
        "sample_encoding": "signed_8_bit; one byte per observed FIFO pop, including zero on underflow",
        "timing": "core bus-service boundaries; equal-cycle events retain trace order",
        "limitations": [
            "Raw FIFO feeds include silence and sound effects; songs and loop boundaries are not identified.",
            "Pop cycles describe the core's applied timing. DMA words share a service boundary; hardware transfer timing and a fixed sample rate are not inferred.",
            "These bytes exclude PSG, routing, gain, DAC and output filtering. They are not a mixed WAV or VGM and cannot be previewed as a native audio capture.",
            "ROM reads are checked against the loaded source. RAM and unresolved sources are snapshots, with no inferred original sample asset."
        ],
    }))?;
    let mut bundle = super::super::bundle::Bundle::new();
    for (name, bytes) in entries {
        bundle.add(name, bytes)?;
    }
    bundle.add("manifest.json", &manifest)?;
    let bytes = bundle.finish()?;
    super::check_cancel(cancel)?;
    super::super::assets::publish_bytes(path, &bytes, cancel, &AtomicU32::new(0))
}

#[cfg(test)]
mod tests;
