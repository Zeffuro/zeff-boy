use std::path::Path;

use crate::audio_discovery::media::{ScanInput, ScanManifest};

use super::*;

pub(super) fn prepare(
    request: &AudioDiscoveryRequest,
    input: &ScanInput,
    manifest: &ScanManifest,
    limits: ScanLimits,
    cancel: &AtomicBool,
) -> anyhow::Result<Option<Vec<u8>>> {
    let Some(path) = &request.driver_evidence else {
        return Ok(None);
    };
    ensure_distinct_output_path(path, &request.input_path)?;
    ensure_distinct_output_path(path, &request.output_path)?;
    if let Some(export) = &request.export {
        ensure_distinct_output_path(path, &export.output_path)?;
    }
    if let Some(relations) = &request.relations {
        ensure_distinct_output_path(path, &relations.output_path)?;
    }
    ensure!(
        !path.exists(),
        "driver evidence output already exists: {}",
        path.display()
    );
    ensure!(
        input.cdda.is_none() && input.standalone_audio.is_none(),
        "--audio-driver-evidence requires a cartridge input"
    );
    let system = input
        .system
        .context("--audio-driver-evidence requires a cartridge input")?;
    let evidence = crate::audio_discovery::drivers::scan(system, &input.bytes, limits, cancel);
    let value = serde_json::json!({
        "schema": "zeff-audio-driver-evidence-export/1",
        "analysis_profile": manifest.analysis_profile,
        "source": manifest.source,
        "transforms": manifest.transforms,
        "driver_evidence": evidence,
    });
    serde_json::to_vec_pretty(&value)
        .context("could not serialize audio driver evidence")
        .map(Some)
}

pub(super) fn write_new(path: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    use std::io::Seek;

    crate::platform::write_new_file_atomically_validated(path, bytes, |file| {
        file.rewind()?;
        let _: serde_json::Value = serde_json::from_reader(file)?;
        Ok(())
    })?;
    println!("[audio-driver-evidence] wrote={}", path.display());
    Ok(())
}

#[cfg(test)]
#[path = "audio_discovery_driver_tests.rs"]
mod tests;
