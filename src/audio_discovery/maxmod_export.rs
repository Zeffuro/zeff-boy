use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use anyhow::{Result, ensure};
use serde_json::json;

pub(crate) fn write_new(path: &Path, source: &[u8], cancel: &AtomicBool) -> Result<()> {
    ensure!(!cancel.load(Ordering::Relaxed), "Maxmod export cancelled");
    ensure!(!path.exists(), "Maxmod output already exists");
    let banks = zeff_audio_discovery::maxmod::discover(
        source,
        zeff_audio_discovery::ScanLimits::default(),
        cancel,
    )
    .map_err(|stop| anyhow::anyhow!("Maxmod inspection stopped: {stop:?}"))?;
    ensure!(
        banks.len() == 1,
        "expected one supported Maxmod sample bank"
    );
    let bank = &banks[0];
    let bank_bytes = span_bytes(source, bank.bank)?;
    let single = bank.samples.len() == 1;
    let mut bundle = super::bundle::Bundle::new();
    let mut artifacts = vec![json!({
        "path": "bank.bin", "byte_len": bank_bytes.len(), "sha256": zeff_firmware::sha256_hex(bank_bytes)
    })];
    bundle.add("bank.bin", bank_bytes)?;
    for (index, sample) in bank.samples.iter().enumerate() {
        ensure!(!cancel.load(Ordering::Relaxed), "Maxmod export cancelled");
        let bytes = span_bytes(source, sample.payload)?;
        let name = if single {
            "sample.u8".to_owned()
        } else {
            format!("samples/{index:04}.u8")
        };
        artifacts.push(json!({
            "path": name, "byte_len": bytes.len(), "sha256": zeff_firmware::sha256_hex(bytes)
        }));
        bundle.add(&name, bytes)?;
    }
    let (schema, profile, description) = if let [sample] = bank.samples.as_slice() {
        (
            "zeff-maxmod-sample-assets/1",
            "msl_gba_single_nonlooping_sfx_v18",
            json!({
                "bank": bank.bank, "record": sample.record, "header": sample.header,
                "payload": sample.payload, "guard": sample.guard, "frequency_code": sample.frequency_code,
            }),
        )
    } else {
        (
            "zeff-maxmod-sample-assets/2",
            "msl_gba_nonlooping_sfx_v18",
            serde_json::to_value(bank)?,
        )
    };
    let manifest = serde_json::to_vec_pretty(&json!({
        "schema": schema,
        "profile": profile,
        "source": {"byte_len": source.len(), "sha256": zeff_firmware::sha256_hex(source)},
        "bank": description,
        "sample_encoding": "unsigned_8_bit",
        "artifacts": artifacts,
        "limitations": [
            "Structural sample preservation only; no active driver, song, playback or runtime address is inferred.",
            "The frequency code is preserved without assigning a playback sample rate."
        ],
    }))?;
    bundle.add("manifest.json", &manifest)?;
    super::assets::publish_bytes(path, &bundle.finish()?, cancel, &AtomicU32::new(0))
}

fn span_bytes(source: &[u8], span: zeff_audio_discovery::tracker::FileSpan) -> Result<&[u8]> {
    let start = usize::try_from(span.offset)?;
    let end = start
        .checked_add(usize::try_from(span.byte_len)?)
        .ok_or_else(|| anyhow::anyhow!("Maxmod span overflow"))?;
    source
        .get(start..end)
        .ok_or_else(|| anyhow::anyhow!("Maxmod span is outside the source"))
}

#[cfg(test)]
mod tests;
