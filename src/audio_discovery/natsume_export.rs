use crate::audio_discovery::formats::FormatAvailability;
use std::path::Path;
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU32, Ordering},
};

use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use zeff_emu_common::system::System;

use super::formats::{AudioFormat, SongFormat};
use super::media::{ScanInput, ScanManifest};
use super::natsume::NatsumeSong;
use super::render::RenderOptions;

mod audio;

pub(crate) struct NatsumeExportRequest {
    bytes: Arc<[u8]>,
    sha256: String,
    song: NatsumeSong,
    metadata: Value,
    audio: Option<(AudioFormat, audio::Options)>,
}

impl NatsumeExportRequest {
    pub(super) fn prepare(
        input: &ScanInput,
        manifest: &ScanManifest,
        song: &NatsumeSong,
        format: SongFormat,
        options: RenderOptions,
    ) -> Result<Self> {
        ensure!(
            input.system == Some(System::Gba)
                && input.standalone_audio.is_none()
                && input.cdda.is_none(),
            "Natsume sequence export requires a matching GBA cartridge source"
        );
        let audio = match format {
            SongFormat::MappedAssets => None,
            SongFormat::Audio(format) => {
                ensure!(format.available(), "selected audio format is unavailable");
                Some((format, audio::Options::from_render(options)?))
            }
            _ => anyhow::bail!("Natsume supports recorded audio and mapped sequence export"),
        };
        super::extract::ExtractionRequest::prepare(
            input,
            manifest,
            song.header,
            "Natsume song header",
        )?;
        Ok(Self {
            bytes: Arc::clone(&input.bytes),
            sha256: manifest
                .scan
                .media
                .sha256
                .clone()
                .context("scan has no media identity")?,
            song: song.clone(),
            audio,
            metadata: json!({
                "classification": manifest.classification(super::catalog::SongRef::Natsume(song)),
                "schema": "zeff-natsume-sequence-export/1",
                "analysis_profile": manifest.analysis_profile,
                "source": manifest.source,
                "transforms": manifest.transforms,
                "media": manifest.scan.media,
                "detector": manifest.scan.detector,
                "detector_version": manifest.scan.detector_version,
                "applicable_detectors": manifest.scan.applicable_detectors,
                "detector_outcomes": manifest.scan.detector_outcomes,
                "song_detector": "gba-natsume-driver",
                "scan_status": manifest.scan.status,
                "song": song,
                "limitations": [
                    "Mapped table, header and visited sequence bytes only. This is not a complete sound bank, standalone music file or executable rip.",
                    "Wait units describe sequence structure, not musical duration. Instrument/percussion graphs and MIDI conversion are unavailable. Original-driver audio rendering is separate from this mapped-data archive."
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
        ensure!(!cancel.load(Ordering::Relaxed), "Natsume export cancelled");
        ensure!(
            zeff_firmware::sha256_hex(&self.bytes) == self.sha256,
            "loaded media does not match the scan SHA-256 identity"
        );
        super::natsume::validate_song(&self.bytes, &self.song, cancel)?;
        if let Some((format, options)) = self.audio {
            return audio::write_new(self, format, options, path, cancel, progress);
        }
        let mut bundle = super::bundle::Bundle::new();
        let mut spans = self.song.mapped_spans.clone();
        spans.sort();
        spans.dedup();
        let mut assets = Vec::with_capacity(spans.len());
        for span in spans {
            ensure!(!cancel.load(Ordering::Relaxed), "Natsume export cancelled");
            let start = span.effective_offset as usize;
            let end = start
                .checked_add(span.byte_len as usize)
                .context("Natsume source range overflows")?;
            let bytes = self
                .bytes
                .get(start..end)
                .context("Natsume sequence range is outside its source media")?;
            let name = format!("source/{start:08x}-{:08x}.bin", span.byte_len);
            bundle.add(&name, bytes)?;
            assets.push(json!({
                "path": name,
                "span": span,
                "sha256": zeff_firmware::sha256_hex(bytes),
            }));
        }
        self.metadata["assets"] = Value::Array(assets);
        bundle.add("manifest.json", &serde_json::to_vec_pretty(&self.metadata)?)?;
        super::assets::publish_bytes(path, &bundle.finish()?, cancel, progress)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio_discovery::natsume::NatsumeSongKind;

    #[test]
    fn forged_inventory_is_rejected_before_mapped_or_audio_export_is_published() -> Result<()> {
        let input = ScanInput {
            bytes: vec![0; 512].into(),
            system: Some(System::Gba),
            standalone_audio: None,
            cdda: None,
            provenance: None,
            analysis_profile: "natsume-export-test",
            display_name: None,
        };
        let manifest = input.analyze(Default::default(), &AtomicBool::new(false));
        let song = NatsumeSong {
            profile: "unrecognized",
            index: 0,
            title: "Fixture".to_owned(),
            kind: NatsumeSongKind::Music,
            table_entry: crate::audio_discovery::test_support::rom_span(0x80, 4),
            header: crate::audio_discovery::test_support::rom_span(0x100, 4),
            channel_mask: 0,
            priority: 0,
            channels: Vec::new(),
            mapped_spans: vec![crate::audio_discovery::test_support::rom_span(0x100, 4)],
            warnings: Vec::new(),
        };
        let directory = tempfile::tempdir()?;
        for (format, extension) in [
            (SongFormat::MappedAssets, "zip"),
            (SongFormat::Audio(AudioFormat::Wav), "wav"),
        ] {
            let path = directory.path().join(format!("song.{extension}"));
            let request = || {
                NatsumeExportRequest::prepare(
                    &input,
                    &manifest,
                    &song,
                    format,
                    RenderOptions::default(),
                )
            };
            assert!(
                request()?
                    .write_new(&path, &AtomicBool::new(false), &AtomicU32::new(0))
                    .is_err()
            );
            assert!(!path.exists());
            std::fs::write(&path, b"existing export")?;
            assert!(
                request()?
                    .write_new(&path, &AtomicBool::new(false), &AtomicU32::new(0))
                    .is_err()
            );
            assert_eq!(std::fs::read(&path)?, b"existing export");
            let cancelled = directory.path().join(format!("cancelled.{extension}"));
            assert!(
                request()?
                    .write_new(&cancelled, &AtomicBool::new(true), &AtomicU32::new(0))
                    .unwrap_err()
                    .to_string()
                    .contains("cancelled")
            );
            assert!(!cancelled.exists());
        }
        assert!(
            NatsumeExportRequest::prepare(
                &input,
                &manifest,
                &song,
                SongFormat::Midi,
                RenderOptions::default()
            )
            .is_err()
        );
        Ok(())
    }
}
