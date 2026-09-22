use std::path::Path;
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU32, Ordering},
};

use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};

use super::formats::SongFormat;
use super::media::{ScanInput, ScanManifest, StandaloneFormat};
use super::vgm::{VgmEncoding, VgmLog};

#[cfg(test)]
pub(crate) mod tests;

pub(crate) struct VgmExportRequest {
    bytes: Arc<[u8]>,
    sha256: String,
    log: VgmLog,
    format: SongFormat,
    metadata: Value,
}

impl VgmExportRequest {
    pub(super) fn prepare(
        input: &ScanInput,
        manifest: &ScanManifest,
        log: &VgmLog,
        format: SongFormat,
    ) -> Result<Self> {
        ensure!(
            input.system.is_none()
                && input.cdda.is_none()
                && input.standalone_audio == Some(StandaloneFormat::Vgm),
            "VGM export requires a matching standalone register-log source"
        );
        ensure!(
            matches!(
                format,
                SongFormat::Vgm | SongFormat::Vgz | SongFormat::MappedAssets
            ) && super::catalog::SongRef::Vgm(log).supports(format),
            "this VGM source cannot be exported in the selected format"
        );
        super::extract::ExtractionRequest::prepare(
            input,
            manifest,
            log.source,
            "Original VGM/VGZ source file",
        )?;
        ensure!(
            log.source.offset == 0 && log.source.byte_len as usize == input.bytes.len(),
            "VGM source span must cover the complete input file"
        );
        Ok(Self {
            bytes: Arc::clone(&input.bytes),
            sha256: manifest
                .scan
                .media
                .sha256
                .clone()
                .context("scan has no media identity")?,
            log: log.clone(),
            format,
            metadata: json!({
                "classification": manifest.classification(super::catalog::SongRef::Vgm(log)),
                "schema": "zeff-vgm-export/1",
                "analysis_profile": manifest.analysis_profile,
                "source": manifest.source,
                "transforms": manifest.transforms,
                "media": manifest.scan.media,
                "detector": manifest.scan.detector,
                "detector_version": manifest.scan.detector_version,
                "applicable_detectors": manifest.scan.applicable_detectors,
                "detector_outcomes": manifest.scan.detector_outcomes,
                "song_detector": "vgm-register-log",
                "scan_status": manifest.scan.status,
                "log": log,
                "limitations": [
                    "This is an imported chip-register performance log, not native cartridge sequence discovery or a claim of playback fidelity.",
                    "VGM exports preserve the complete decoded source. VGZ exports preserve original compressed bytes. Trailing logical bytes are retained without interpretation.",
                    "Logical VGM spans are relative to the identified decoded byte stream. They do not map to compressed-file offsets."
                ],
            }),
        })
    }

    pub(super) fn write_new(
        mut self,
        path: &Path,
        cancel: &AtomicBool,
        progress: &AtomicU32,
    ) -> Result<()> {
        ensure!(!cancel.load(Ordering::Relaxed), "VGM export cancelled");
        ensure!(
            zeff_firmware::sha256_hex(&self.bytes) == self.sha256,
            "loaded media does not match the scan SHA-256 identity"
        );
        super::vgm::verify(&self.bytes, &self.log, cancel)?;
        let data = match self.format {
            SongFormat::Vgz => self.bytes.to_vec(),
            SongFormat::Vgm => super::vgm::decode(&self.bytes, cancel)?,
            SongFormat::MappedAssets => {
                let mut bundle = super::bundle::Bundle::new();
                let name = if self.log.encoding == VgmEncoding::Gzip {
                    "source.vgz"
                } else {
                    "source.vgm"
                };
                bundle.add(name, &self.bytes)?;
                self.metadata["source_path"] = json!(name);
                if self.log.encoding == VgmEncoding::Gzip {
                    bundle.add("decoded.vgm", &super::vgm::decode(&self.bytes, cancel)?)?;
                    self.metadata["logical_path"] = json!("decoded.vgm");
                } else {
                    self.metadata["logical_path"] = json!(name);
                }
                bundle.add("manifest.json", &serde_json::to_vec_pretty(&self.metadata)?)?;
                bundle.finish()?
            }
            _ => unreachable!("capabilities checked in prepare"),
        };
        super::assets::publish_bytes(path, &data, cancel, progress)
    }
}
