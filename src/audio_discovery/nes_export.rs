use std::path::Path;
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU32, Ordering},
};

use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use zeff_emu_common::system::System;

use super::formats::SongFormat;
use super::media::{ScanInput, ScanManifest};
use super::nes_music::NesSong;
use super::render::{MAX_DURATION_SECONDS, MAX_LOOP_PASSES, RenderOptions};

#[cfg(test)]
mod tests;

pub(crate) struct NesExportRequest {
    bytes: Arc<[u8]>,
    sha256: String,
    song: NesSong,
    format: SongFormat,
    options: RenderOptions,
    metadata: Value,
}

impl NesExportRequest {
    pub(super) fn prepare(
        input: &ScanInput,
        manifest: &ScanManifest,
        song: &NesSong,
        format: SongFormat,
        options: RenderOptions,
    ) -> Result<Self> {
        ensure!(
            input.system == Some(System::Nes)
                && input.standalone_audio.is_none()
                && input.cdda.is_none(),
            "NES song export requires a matching cartridge source"
        );
        ensure!(
            format == SongFormat::MappedAssets
                || (format == SongFormat::Midi && song.midi_exportable),
            "this NES song cannot be exported in the selected format"
        );
        ensure!(
            (1..=MAX_LOOP_PASSES).contains(&options.loops)
                && (1..=MAX_DURATION_SECONDS).contains(&options.max_seconds),
            "NES MIDI requires 1 to {MAX_LOOP_PASSES} total passes and a duration limit of 1 to {MAX_DURATION_SECONDS} seconds"
        );
        super::extract::ExtractionRequest::prepare(
            input,
            manifest,
            song.table_entry,
            "NES song selector",
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
                "classification": manifest.classification(super::catalog::SongRef::Nes(song)),
                "schema": "zeff-nes-music-export/1",
                "analysis_profile": manifest.analysis_profile,
                "source": manifest.source,
                "transforms": manifest.transforms,
                "media": manifest.scan.media,
                "detector": manifest.scan.detector,
                "detector_version": manifest.scan.detector_version,
                "applicable_detectors": manifest.scan.applicable_detectors,
                "detector_outcomes": manifest.scan.detector_outcomes,
                "song_detector": "nes-queue-driver",
                "scan_status": manifest.scan.status,
                "song": song,
                "limitations": [
                    "Mapped original song structures are not a standalone NES driver or complete sound bank.",
                    "MIDI projects the verified NTSC driver's note timing to approximate General MIDI instruments. APU envelopes, duty, noise, hardware sweep and runtime SFX are not reproduced.",
                    "The song selector entry is the stable file-offset anchor. Aliases may share physical sequence headers."
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
        ensure!(!cancel.load(Ordering::Relaxed), "NES song export cancelled");
        ensure!(
            zeff_firmware::sha256_hex(&self.bytes) == self.sha256,
            "loaded media does not match the scan SHA-256 identity"
        );
        super::nes_music::validate_song(&self.bytes, &self.song, cancel)?;
        let data = if self.format == SongFormat::Midi {
            super::nes_music::midi(
                &self.bytes,
                &self.song,
                self.options.loops,
                self.options.max_seconds,
                cancel,
            )?
            .bytes
        } else {
            let mut bundle = super::bundle::Bundle::new();
            let mut spans = self.song.mapped_spans.clone();
            spans.sort_by_key(|span| (span.offset, span.byte_len));
            spans.dedup();
            let mut assets = Vec::with_capacity(spans.len());
            for span in spans {
                ensure!(!cancel.load(Ordering::Relaxed), "NES song export cancelled");
                let start = span.offset as usize;
                let end = start
                    .checked_add(span.byte_len as usize)
                    .context("NES source range overflows")?;
                let bytes = self
                    .bytes
                    .get(start..end)
                    .context("NES song range is outside its source media")?;
                let name = format!("source/{start:08x}-{:08x}.bin", span.byte_len);
                bundle.add(&name, bytes)?;
                assets.push(
                    json!({"path": name, "span": span, "sha256": zeff_firmware::sha256_hex(bytes)}),
                );
            }
            self.metadata["assets"] = Value::Array(assets);
            bundle.add("manifest.json", &serde_json::to_vec_pretty(&self.metadata)?)?;
            bundle.finish()?
        };
        super::assets::publish_bytes(path, &data, cancel, progress)
    }
}
