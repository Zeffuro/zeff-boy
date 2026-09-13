use std::path::Path;
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU32, Ordering},
};

use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use zeff_emu_common::system::System;

use super::formats::SongFormat;
use super::gb_music::GbSong;
use super::media::{ScanInput, ScanManifest};
use super::render::{MAX_DURATION_SECONDS, MAX_LOOP_PASSES, RenderOptions};

pub(crate) struct GbExportRequest {
    bytes: Arc<[u8]>,
    sha256: String,
    song: GbSong,
    format: SongFormat,
    options: RenderOptions,
    metadata: Value,
}

impl GbExportRequest {
    pub(super) fn prepare(
        input: &ScanInput,
        manifest: &ScanManifest,
        song: &GbSong,
        format: SongFormat,
        options: RenderOptions,
    ) -> Result<Self> {
        ensure!(
            input.system == Some(System::Gb)
                && input.standalone_audio.is_none()
                && input.cdda.is_none(),
            "Game Boy song export requires a matching cartridge source"
        );
        ensure!(
            format == SongFormat::MappedAssets
                || (format == SongFormat::Midi && song.midi_exportable),
            "this Game Boy song cannot be exported in the selected format"
        );
        ensure!(
            (1..=MAX_LOOP_PASSES).contains(&options.loops)
                && (1..=MAX_DURATION_SECONDS).contains(&options.max_seconds),
            "Game Boy MIDI requires 1 to {MAX_LOOP_PASSES} total passes and a duration limit of 1 to {MAX_DURATION_SECONDS} seconds"
        );
        super::extract::ExtractionRequest::prepare(
            input,
            manifest,
            song.header,
            "Game Boy song header",
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
            format,
            options,
            metadata: json!({
                "classification": manifest.classification(super::catalog::SongRef::Gb(song)),
                "schema": "zeff-gb-music-export/1",
                "analysis_profile": manifest.analysis_profile,
                "source": manifest.source,
                "transforms": manifest.transforms,
                "media": manifest.scan.media,
                "detector": manifest.scan.detector,
                "detector_version": manifest.scan.detector_version,
                "applicable_detectors": manifest.scan.applicable_detectors,
                "detector_outcomes": manifest.scan.detector_outcomes,
                "song_detector": "gb-banked-driver",
                "scan_status": manifest.scan.status,
                "song": song,
                "limitations": [
                    "These are the mapped original song structures visited by the supported interpreter, not a standalone driver or a complete Game Boy sound bank.",
                    "MIDI is an approximate note projection. Hardware waveforms, envelopes and modulation are not reproduced by a General MIDI player.",
                    "Bank and CPU address are recorded separately; a banked Game Boy address alone does not identify a unique file offset."
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
            "Game Boy song export cancelled"
        );
        ensure!(
            zeff_firmware::sha256_hex(&self.bytes) == self.sha256,
            "loaded media does not match the scan SHA-256 identity"
        );
        super::gb_music::validate_song(&self.bytes, &self.song, cancel)?;
        let data = if self.format == SongFormat::Midi {
            let midi = super::gb_music::midi(
                &self.bytes,
                &self.song,
                self.options.loops,
                self.options.max_seconds,
                cancel,
            )?;
            midi.bytes
        } else {
            let mut bundle = super::bundle::Bundle::new();
            let mut spans = self.song.mapped_spans.clone();
            spans.sort_by_key(|span| (span.offset, span.byte_len));
            spans.dedup();
            let mut assets = Vec::with_capacity(spans.len());
            for span in spans {
                ensure!(
                    !cancel.load(Ordering::Relaxed),
                    "Game Boy song export cancelled"
                );
                let start = span.offset as usize;
                let end = start
                    .checked_add(span.byte_len as usize)
                    .context("Game Boy source range overflows")?;
                let bytes = self
                    .bytes
                    .get(start..end)
                    .context("Game Boy song range is outside its source media")?;
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
            bundle.finish()?
        };
        super::assets::publish_bytes(path, &data, cancel, progress)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forged_gb_inventory_cannot_publish_or_replace_an_existing_file() -> Result<()> {
        let mut bytes = vec![0; 0x8000];
        bytes[0x4000..0x4003].copy_from_slice(&[0, 0, 0x41]);
        bytes[0x4100..0x4107].copy_from_slice(&[0xd8, 1, 0xf0, 0xd4, 0x13, 0xff, 0]);
        let song = super::super::test_support::gb_music::synthetic_song(&bytes, 1, 0x4000);
        assert!(song.midi_exportable);
        let input = ScanInput {
            bytes: bytes.into(),
            system: Some(System::Gb),
            standalone_audio: None,
            cdda: None,
            provenance: None,
            analysis_profile: "gb-export-boundary-test",
            display_name: None,
        };
        let mut manifest = input.analyze(Default::default(), &AtomicBool::new(false));
        assert!(manifest.scan.gb_songs.is_empty());
        manifest.scan.gb_songs.push(song.clone());
        let directory = tempfile::tempdir()?;
        for format in [SongFormat::Midi, SongFormat::MappedAssets] {
            let path = directory.path().join(format.info().filename);
            let request =
                || GbExportRequest::prepare(&input, &manifest, &song, format, Default::default());
            let failure = request()?
                .write_new(&path, &AtomicBool::new(false), &AtomicU32::new(0))
                .unwrap_err();
            assert!(
                failure
                    .to_string()
                    .contains("unrecognized Game Boy music profile")
            );
            assert!(!path.exists());
            std::fs::write(&path, b"existing export")?;
            assert!(
                request()?
                    .write_new(&path, &AtomicBool::new(false), &AtomicU32::new(0))
                    .is_err()
            );
            assert_eq!(std::fs::read(&path)?, b"existing export");
            let cancelled = directory
                .path()
                .join(format!("cancelled-{}", format.info().filename));
            let failure = request()?
                .write_new(&cancelled, &AtomicBool::new(true), &AtomicU32::new(0))
                .unwrap_err();
            assert!(failure.to_string().contains("cancelled"));
            assert!(!cancelled.exists());
        }
        assert!(
            GbExportRequest::prepare(
                &input,
                &manifest,
                &song,
                SongFormat::SoundFont,
                Default::default()
            )
            .is_err()
        );
        Ok(())
    }
}
