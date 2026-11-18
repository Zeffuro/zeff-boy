use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{Result, ensure};
use serde_json::Value;
use zeff_emu_common::audio_trace::{MdAudioTrace, MdTraceRegion, MdTraceSource, MdTraceWrite};

pub(super) fn validate_context(
    trace: &MdAudioTrace,
    context: &Value,
    cancel: &AtomicBool,
) -> Result<()> {
    ensure!(!cancel.load(Ordering::Relaxed), "MD replay cancelled");
    let region = match trace.chip.region {
        MdTraceRegion::NtscUs => "NtscUs",
        MdTraceRegion::NtscJapan => "NtscJapan",
        MdTraceRegion::Pal => "Pal",
    };
    ensure!(
        context["system"] == "md"
            && context["firmware"].is_null()
            && context["settings"]["region"] == region,
        "MD replay requires a matching cartridge-reset capture context"
    );
    let identity = &context["source"]["effective_rom"];
    let length = identity["byte_len"].as_u64();
    ensure!(
        length.is_some_and(|length| {
            (8..=zeff_md_core::constants::MAX_CARTRIDGE_SIZE as u64).contains(&length)
        }) && identity["sha256"].as_str().is_some_and(super::valid_hash)
            && identity["address_space"] == "effective_core_rom",
        "MD replay requires a valid effective ROM identity"
    );
    let length = length.unwrap();
    for event in &trace.events {
        ensure!(!cancel.load(Ordering::Relaxed), "MD replay cancelled");
        let origin = match event.write {
            MdTraceWrite::Psg { origin, .. }
            | MdTraceWrite::Opn2Port { origin, .. }
            | MdTraceWrite::Opn2Reset { origin } => origin,
        };
        if let MdTraceSource::EffectiveRom { offset } = origin.source {
            ensure!(
                u64::from(offset) < length,
                "MD audio trace source exceeds effective ROM"
            );
        }
    }
    Ok(())
}
