use std::path::Path;
use std::sync::atomic::AtomicBool;

use anyhow::{Result, ensure};
use serde_json::Value;
use zeff_emu_common::audio_trace::{AudioTraceSource, NesAudioTrace, NesTraceWrite};

pub(crate) fn write_nes_new(
    path: &Path,
    trace: &NesAudioTrace,
    context: Value,
    cancel: &AtomicBool,
) -> Result<()> {
    validate_sample_provenance(trace, &context)?;
    let encoded = crate::audio_discovery::vgm::capture::encode_nes(trace, cancel)?;
    super::native::write_native_new(
        path,
        trace,
        context,
        super::native::CaptureEncoding {
            kind: "fresh_to_end_nes_base_apu_capture",
            limitations: &[
                "Captures a freshly constructed base NES APU interval, including register writes, status reads and actual DMC fetched bytes. It does not identify songs, triggers or loops.",
                "Cycles start after the core's seven-cycle CPU construction origin, before each corresponding APU tick. The descriptor identifies the emulator reset contract and regional rational clock, not a hardware power-on measurement.",
                "DMC fetches retain their post-mapping byte and source. DMA-origin events have no halted-instruction attribution. ROM offsets include the original iNES header and trainer; unresolved cartridge RAM origins remain unknown.",
                "VGM is a quantized base-APU register projection when the trace satisfies its supported reset, region and event contract. DMC playback and unsupported regions retain native JSON with explicit VGM refusal reasons. External VGM PCM equivalence is not established.",
                "FDS, expansion audio, other cartridge boards, warm reset and arbitrary-state capture are unsupported. The native trace remains the timing-preserving artifact, including status-read side effects and DMC fetches.",
            ],
            capture: encoded.capture.as_ref(),
            unavailable: serde_json::to_value(&encoded.unavailable)?,
        },
        cancel,
    )
}

pub(in crate::audio_discovery) fn validate_sample_provenance(
    trace: &NesAudioTrace,
    context: &Value,
) -> Result<()> {
    let len = context["source"]["loaded_media"]["byte_len"]
        .as_u64()
        .ok_or_else(|| anyhow::anyhow!("NES audio trace requires loaded-media identity"))?;
    for event in &trace.events {
        if let NesTraceWrite::DmcFetch { source, .. } = event.write {
            match source {
                AudioTraceSource::CartridgeRom {
                    offset,
                    bit_reversed,
                } => ensure!(
                    offset < len && !bit_reversed,
                    "NES DMC fetch has an invalid loaded-media source"
                ),
                AudioTraceSource::WorkRam { offset } => {
                    ensure!(offset < 0x800, "NES DMC source exceeds internal RAM")
                }
                AudioTraceSource::Unknown | AudioTraceSource::Unmapped => {}
                _ => anyhow::bail!("unsupported NES DMC source provenance"),
            }
        }
    }
    Ok(())
}
