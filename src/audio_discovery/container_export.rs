use std::path::Path;
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU32, Ordering},
};

use anyhow::{Result, ensure};
use serde_json::{Value, json};

use super::formats::SongFormat;
use super::media::{ScanInput, ScanManifest, StandaloneFormat};
use super::rips::MusicRip;

#[cfg(test)]
mod tests;

pub(crate) struct ContainerExportRequest {
    bytes: Arc<[u8]>,
    rip: MusicRip,
    format: SongFormat,
    metadata: Value,
}

impl ContainerExportRequest {
    pub(super) fn prepare(
        input: &ScanInput,
        manifest: &ScanManifest,
        rip: &MusicRip,
        format: SongFormat,
    ) -> Result<Self> {
        ensure!(
            input.system.is_none()
                && input.cdda.is_none()
                && input.standalone_audio == Some(StandaloneFormat::Rip(rip.format)),
            "music-rip export requires a matching standalone source"
        );
        ensure!(
            super::catalog::SongRef::Rip(rip).supports(format),
            "selected format cannot preserve this music rip"
        );
        super::extract::ExtractionRequest::prepare(
            input,
            manifest,
            rip.source,
            "Original music-rip source",
        )?;
        ensure!(
            rip.source.offset == 0
                && rip.source.byte_len as usize == input.bytes.len()
                && manifest.scan.media.sha256.as_ref() == Some(&rip.sha256),
            "music-rip source span and identity must cover the complete input file"
        );
        Ok(Self {
            bytes: Arc::clone(&input.bytes),
            rip: rip.clone(),
            format,
            metadata: json!({
                "schema": "zeff-music-rip-export/1",
                "analysis_profile": manifest.analysis_profile,
                "display_name": manifest.display_name,
                "source": manifest.source,
                "transforms": manifest.transforms,
                "media": manifest.scan.media,
                "detector": manifest.scan.detector,
                "detector_version": manifest.scan.detector_version,
                "applicable_detectors": manifest.scan.applicable_detectors,
                "detector_outcomes": manifest.scan.detector_outcomes,
                "song_detector": rip.format.detector_id(),
                "scan_status": manifest.scan.status,
                "rip": rip,
                "limitations": [
                    "This imported executable-rip container is preserved without running its program or attributing a native cartridge driver.",
                    "The declared song count and first song are header metadata. No per-song byte extents, MIDI, sound banks or playback are inferred.",
                    "Initial entry offsets describe header-directed source mapping, not callable code or successful playback. Runtime banking and register changes are unobserved.",
                    "All source bytes are retained. A declared NSF program length separates any appended opaque metadata; metadata chunks are not interpreted."
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
        ensure!(
            !cancel.load(Ordering::Relaxed),
            "music-rip export cancelled"
        );
        super::rips::verify(&self.bytes, &self.rip, cancel)?;
        let bytes = if self.format == SongFormat::MappedAssets {
            let mut bundle = super::bundle::Bundle::new();
            let source_path = format!("source.{}", self.rip.format.extension());
            bundle.add(&source_path, &self.bytes)?;
            self.metadata["source_path"] = json!(source_path);
            let mut assets = Vec::new();
            for (name, span) in [
                ("header.bin", Some(self.rip.header)),
                ("program.bin", Some(self.rip.program)),
                ("metadata.bin", self.rip.opaque_metadata),
            ] {
                let Some(span) = span else { continue };
                ensure!(
                    !cancel.load(Ordering::Relaxed),
                    "music-rip export cancelled"
                );
                let data =
                    &self.bytes[span.offset as usize..(span.offset + span.byte_len) as usize];
                bundle.add(name, data)?;
                assets.push(json!({ "path": name, "span": span, "sha256": zeff_firmware::sha256_hex(data) }));
            }
            self.metadata["assets"] = json!(assets);
            bundle.add("manifest.json", &serde_json::to_vec_pretty(&self.metadata)?)?;
            bundle.finish()?
        } else {
            self.bytes.to_vec()
        };
        super::assets::publish_bytes(path, &bytes, cancel, progress)
    }
}
