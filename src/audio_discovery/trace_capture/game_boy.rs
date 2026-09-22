use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU32};

use anyhow::{Result, ensure};
use serde_json::{Value, json};
use zeff_emu_common::audio_trace::GameBoyAudioTrace;

use super::{check_cancel, validate_provenance};

pub(crate) fn write_game_boy_new(
    path: &Path,
    trace: &GameBoyAudioTrace,
    context: Value,
    cancel: &AtomicBool,
) -> Result<()> {
    check_cancel(cancel)?;
    validate_provenance(trace, &context)?;
    let encoded = crate::audio_discovery::vgm::capture::encode_game_boy(trace, cancel)?;
    let events = serde_json::to_vec(trace)?;
    ensure!(
        events.len() <= 96 * 1024 * 1024,
        "audio trace exceeds its JSON size limit"
    );
    check_cancel(cancel)?;

    let mut artifacts = vec![json!({
        "path": "trace.json",
        "byte_len": events.len(),
        "sha256": zeff_firmware::sha256_hex(&events),
    })];
    let vgm = encoded.capture.as_ref().map(|capture| &capture.bytes);
    if let Some(vgm) = vgm {
        artifacts.insert(
            0,
            json!({
                "path": "capture.vgm",
                "byte_len": vgm.len(),
                "sha256": zeff_firmware::sha256_hex(vgm),
            }),
        );
    }
    let vgm_metadata = match &encoded.capture {
        Some(capture) => json!({
            "status": "available",
            "capture": capture.metadata,
            "unavailable": encoded.unavailable,
        }),
        None => json!({
            "status": "unavailable",
            "unavailable": encoded.unavailable,
        }),
    };
    let mut metadata = json!({
        "schema": "zeff-audio-trace-capture/1",
        "kind": "reset_to_end_game_boy_hardware_register_capture",
        "context": context,
        "vgm": vgm_metadata,
        "artifacts": artifacts,
        "limitations": [
            "Captures the executed interval, including divider resets, STOP and CGB speed changes; it does not discover song selectors or loop boundaries.",
            "Instruction provenance identifies the code writing sound hardware, not sequence or sample-data origins. CPU-interrupt writes intentionally use an unknown instruction source and PC zero. ROM offsets address the exact loaded core input; they are not compressed archive offsets. Work-RAM offsets use physical WRAM 0x0000 through 0x7FFF, followed by HRAM at 0x8000 through 0x807E.",
            "Trace JSON preserves native clocks and event order. Sequencer-clock events occur before the native APU service batch, while register writes record completed CPU-write cycles; the trace does not claim physical cycle-exact PCM timing. VGM is emitted only when every represented event has a defined safe mapping; unavailable representations retain the trace and explicit reasons.",
            "The trace records its reset state. Headless CLI captures use an HLE post-boot state without firmware; API callers can start from power-on with firmware. It is not a hardware-bit-exact PCM claim.",
            "Standalone VGM preview and arbitrary-state live capture are not implemented.",
        ],
    });
    if let Some(contract) = trace.chip.native_replay {
        metadata["native_playback"] = json!({
            "contract": contract,
            "output_boundaries": "recorded_native_apu_batches_and_host_pcm_drains",
        });
        metadata["limitations"].as_array_mut().unwrap().push(json!(
            "Native replay preserves the recorded APU service batches and output-drain boundaries, including samples discarded by NR52 power-off. Output-setting changes or incomplete output timelines prevent exact native playback; source PCM comparisons require matching sample rates."
        ));
    }
    let metadata = serde_json::to_vec_pretty(&metadata)?;

    let mut bundle = super::super::bundle::Bundle::new();
    if let Some(vgm) = vgm {
        bundle.add("capture.vgm", vgm)?;
    }
    bundle.add("trace.json", &events)?;
    bundle.add("manifest.json", &metadata)?;
    let bytes = bundle.finish()?;
    check_cancel(cancel)?;
    super::super::assets::publish_bytes(path, &bytes, cancel, &AtomicU32::new(0))
}
