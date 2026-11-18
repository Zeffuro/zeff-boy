use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{Result, ensure};
use serde_json::Value;
use zeff_emu_common::audio_trace::{MdAudioTraceCapture, MdTraceSource, MdTraceWrite};

pub(crate) fn write_md_new(
    path: &Path,
    capture: &MdAudioTraceCapture,
    context: Value,
    cancel: &AtomicBool,
) -> Result<()> {
    validate_identity(capture, &context, cancel)?;
    let encoded = crate::audio_discovery::vgm::capture::encode_md(&capture.trace, cancel)?;
    super::native::write_native_new(
        path,
        &capture.trace,
        context,
        super::native::CaptureEncoding {
            kind: "reset_to_end_md_committed_sound_capture",
            limitations: &[
                "Records committed PSG writes, raw OPN2 address/data ports and OPN2 resets over a complete fresh interval. It does not identify songs, loops, sample-data origins or every attempted bus operation.",
                "Cycles are absolute regional master ticks at bus service boundaries, including the reset construction origin. Typed CPU origins identify architectural instruction/segment boundaries, not guaranteed opcode fetches. Generic instruction sources remain unknown.",
                "Effective ROM offsets address the separately identified effective core ROM, never ZIP archive offsets. Main and sound RAM sources remain typed; unresolved sources remain unknown.",
                "VGM is a quantized composite register projection with explicit refusal for OPN2 resets and unsupported register data. Native JSON retains all address latches, ignored inputs and original timing. External-player PCM equivalence is not established.",
                "Native rendering replays the captured composite hardware output from power-on. Arbitrary-state capture, individual-channel extraction and unrecorded host output changes are unsupported.",
            ],
            capture: encoded.capture.as_ref(),
            unavailable: serde_json::to_value(&encoded.unavailable)?,
        },
        cancel,
    )
}

fn validate_identity(
    capture: &MdAudioTraceCapture,
    context: &Value,
    cancel: &AtomicBool,
) -> Result<()> {
    crate::audio_discovery::vgm::capture::validate_md_trace(&capture.trace, cancel)?;
    let identity = &context["source"]["effective_rom"];
    ensure!(
        (8..=zeff_md_core::constants::MAX_CARTRIDGE_SIZE as u32)
            .contains(&capture.effective_rom_len)
            && identity["byte_len"].as_u64() == Some(u64::from(capture.effective_rom_len))
            && identity["sha256"].as_str()
                == Some(const_hex::encode(capture.effective_rom_sha256).as_str()),
        "MD audio trace effective ROM identity does not match its capture"
    );
    for event in &capture.trace.events {
        ensure!(
            !cancel.load(Ordering::Relaxed),
            "audio trace export cancelled"
        );
        let origin = match event.write {
            MdTraceWrite::Psg { origin, .. }
            | MdTraceWrite::Opn2Port { origin, .. }
            | MdTraceWrite::Opn2Reset { origin } => origin,
        };
        if let MdTraceSource::EffectiveRom { offset } = origin.source {
            ensure!(
                offset < capture.effective_rom_len,
                "MD audio trace source exceeds effective ROM"
            );
        }
    }
    Ok(())
}
