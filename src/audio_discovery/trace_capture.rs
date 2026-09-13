use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use super::vgm::capture::VgmCaptureSource;
use anyhow::{Result, ensure};
use serde_json::{Value, json};
use zeff_emu_common::audio_trace::{AudioTraceChip, AudioTraceSource, ChipAudioTrace};

mod game_boy;

pub(crate) use game_boy::write_game_boy_new;

pub(crate) fn write_new<C: AudioTraceChip, W>(
    path: &Path,
    trace: &ChipAudioTrace<C, W>,
    context: Value,
    cancel: &AtomicBool,
) -> Result<()>
where
    ChipAudioTrace<C, W>: VgmCaptureSource + serde::Serialize,
{
    check_cancel(cancel)?;
    validate_provenance(trace, &context)?;
    let encoded = trace.encode_vgm(cancel)?;
    let events = serde_json::to_vec(trace)?;
    ensure!(
        events.len() <= 96 * 1024 * 1024,
        "audio trace exceeds its JSON size limit"
    );
    check_cancel(cancel)?;
    let (kind, limitations) = if encoded.metadata.wonder_swan.is_some() {
        (
            "reset_to_end_hardware_register_and_wave_ram_capture",
            &[
                "Captures the executed interval, including game sound effects and silence; it does not discover song selectors or loop boundaries.",
                "Instruction-origin CPU and General DMA writes retain writer provenance; interrupt entries and autonomous Sound DMA have no attributed instruction. Sequence and sample source spans are not identified. ROM offsets address the exact loaded core input; they are not compressed archive offsets.",
                "VGM rounds absolute time down to 44,100 ticks per second. Trace JSON preserves original clocks, event order and timing precision.",
                "The VGM preamble reconstructs ordinary sound registers and 16 KiB wave RAM, but cannot reproduce every oscillator phase or emulator-specific APU behavior. External playback is not hardware-bit-exact PCM qualification.",
                "The existing Audio Explorer can inspect and preserve the captured VGM. Standalone VGM preview and arbitrary-state live capture are not implemented.",
            ][..],
        )
    } else {
        (
            "reset_to_end_hardware_register_capture",
            &[
                "Captures the executed interval, including game sound effects and silence; it does not discover song selectors or loop boundaries.",
                "Instruction provenance identifies the code writing sound ports, not the origin of sequence or sample data. ROM offsets address the exact loaded core input; they are not compressed archive offsets.",
                "VGM rounds absolute time down to 44,100 ticks per second. Trace JSON preserves original clocks, event order and timing precision.",
                "VGM resets registers but cannot reproduce every oscillator phase or emulator-specific PSG behavior. External playback is not hardware-bit-exact PCM qualification.",
                "The existing Audio Explorer can inspect and preserve the captured VGM. Standalone VGM preview and arbitrary-state live capture are not implemented.",
            ][..],
        )
    };
    let metadata = serde_json::to_vec_pretty(&json!({
        "schema": "zeff-audio-trace-capture/1",
        "kind": kind,
        "context": context,
        "vgm": encoded.metadata,
        "artifacts": [
            {"path": "capture.vgm", "byte_len": encoded.bytes.len(), "sha256": zeff_firmware::sha256_hex(&encoded.bytes)},
            {"path": "trace.json", "byte_len": events.len(), "sha256": zeff_firmware::sha256_hex(&events)},
        ],
        "limitations": limitations,
    }))?;
    let mut bundle = super::bundle::Bundle::new();
    bundle.add("capture.vgm", &encoded.bytes)?;
    bundle.add("trace.json", &events)?;
    bundle.add("manifest.json", &metadata)?;
    let bytes = bundle.finish()?;
    check_cancel(cancel)?;
    super::assets::publish_bytes(path, &bytes, cancel, &AtomicU32::new(0))
}

fn validate_provenance<C: AudioTraceChip, W>(
    trace: &ChipAudioTrace<C, W>,
    context: &Value,
) -> Result<()> {
    trace.validate_complete()?;
    let media_len = context["source"]["loaded_media"]["byte_len"]
        .as_u64()
        .ok_or_else(|| anyhow::anyhow!("audio trace requires a loaded-media identity"))?;
    let firmware_len = context["firmware"]["byte_len"].as_u64();
    for event in &trace.events {
        match event.instruction_source {
            AudioTraceSource::CartridgeRom { offset, .. } => ensure!(
                offset < media_len,
                "audio trace code is outside its loaded media"
            ),
            AudioTraceSource::BootRom { offset } => ensure!(
                firmware_len.is_some_and(|len| offset < len),
                "audio trace code has no matching firmware identity"
            ),
            _ => {}
        }
    }
    Ok(())
}

fn check_cancel(cancel: &AtomicBool) -> Result<()> {
    ensure!(
        !cancel.load(Ordering::Relaxed),
        "audio trace export cancelled"
    );
    Ok(())
}

#[cfg(test)]
pub(crate) mod tests;
