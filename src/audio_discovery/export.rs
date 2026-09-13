use crate::audio_discovery::formats::FormatAvailability;
use std::path::Path;
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU32, Ordering},
};

use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};

use super::assets::AudioExportRequest;
use super::catalog::{SongId, SongRef};
use super::formats::{ExportKind, SongFormat};
use super::media::{ScanInput, ScanManifest};
use super::render::RenderOptions;

#[cfg(test)]
mod tests;

pub(crate) enum SongExportRequest {
    Mp2k(AudioExportRequest),
    Gsf(super::gsf::GsfExportRequest),
    NativeGsf(super::gsf::native::NativeGsfRequest),
    NativeRip(super::native_rip_export::NativeRipRequest),
    Gb(super::gb_export::GbExportRequest),
    Nes(super::nes_export::NesExportRequest),
    Natsume(super::natsume_export::NatsumeExportRequest),
    Vgm(super::vgm_export::VgmExportRequest),
    Rip(super::container_export::ContainerExportRequest),
    Tracker(TrackerExportRequest),
    Pcm(super::pcm::song::PcmExportRequest),
    Cdda {
        input: Arc<super::cdda::CdAudioInput>,
        number: u8,
        format: super::formats::AudioFormat,
    },
}

pub(crate) struct TrackerExportRequest {
    bytes: Arc<[u8]>,
    sha256: String,
    song: TrackerSong,
    format: SongFormat,
    metadata: Value,
}

enum TrackerSong {
    Gax(Box<super::gax::GaxSong>),
    EngineSoftware(Box<super::engine_software::EngineSoftwareSong>),
    Embedded(super::tracker::EmbeddedModule),
}

impl SongExportRequest {
    pub(crate) fn prepare(
        input: &ScanInput,
        manifest: &ScanManifest,
        id: SongId,
        format: SongFormat,
        options: RenderOptions,
    ) -> Result<Self> {
        let song = manifest
            .scan
            .song(id)
            .context("select a song before exporting")?;
        ensure!(
            format.available() && song.supports(format),
            "{} does not support {} for the selected song",
            song.engine(),
            format.info().label
        );
        if format.is_gsf() && !matches!(id, SongId::Mp2k(_)) {
            return super::gsf::native::NativeGsfRequest::prepare(
                input, manifest, id, format, options,
            )
            .map(Self::NativeGsf);
        }
        if matches!(format, SongFormat::Gbs | SongFormat::Nsf | SongFormat::Sgc)
            && !matches!(song, SongRef::Rip(_))
        {
            return super::native_rip_export::NativeRipRequest::prepare(
                input, manifest, id, format,
            )
            .map(Self::NativeRip);
        }
        if (matches!(format, SongFormat::Audio(_)) && super::pcm::song::PcmSong::can_play(song))
            || (super::pcm::song::PcmSong::is_native(song) && !matches!(song, SongRef::Gb(_)))
        {
            return super::pcm::song::PcmExportRequest::prepare(
                input, manifest, song, format, options,
            )
            .map(Self::Pcm);
        }
        if let SongId::Mp2k(index) = id {
            if format.is_gsf() {
                return super::gsf::GsfExportRequest::prepare(
                    input, manifest, index, format, options,
                )
                .map(Self::Gsf);
            }
            return AudioExportRequest::prepare(
                input,
                manifest,
                index,
                ExportKind::Song { format, options },
            )
            .map(Self::Mp2k);
        }
        if let SongRef::Cdda(track) = song {
            let disc = super::cdda::input_for_song(input, manifest, *track)?;
            let SongFormat::Audio(format) = format else {
                unreachable!("capabilities checked above")
            };
            return Ok(Self::Cdda {
                input: disc,
                number: track.number,
                format,
            });
        }
        if let SongRef::Gb(song) = song {
            return super::gb_export::GbExportRequest::prepare(
                input, manifest, song, format, options,
            )
            .map(Self::Gb);
        }
        if let SongRef::Nes(song) = song {
            return super::nes_export::NesExportRequest::prepare(
                input, manifest, song, format, options,
            )
            .map(Self::Nes);
        }
        if let SongRef::Natsume(song) = song {
            return super::natsume_export::NatsumeExportRequest::prepare(
                input, manifest, song, format, options,
            )
            .map(Self::Natsume);
        }
        if let SongRef::Vgm(log) = song {
            return super::vgm_export::VgmExportRequest::prepare(input, manifest, log, format)
                .map(Self::Vgm);
        }
        if let SongRef::Rip(rip) = song {
            return super::container_export::ContainerExportRequest::prepare(
                input, manifest, rip, format,
            )
            .map(Self::Rip);
        }
        let span = song
            .span()
            .context("selected song has no mapped source range")?;
        super::extract::ExtractionRequest::prepare(input, manifest, span, "Tracker song")?;
        let sha256 = manifest
            .scan
            .media
            .sha256
            .clone()
            .context("scan has no media identity")?;
        let mut metadata = json!({
            "schema": "zeff-tracker-export/1",
            "analysis_profile": manifest.analysis_profile,
            "source": manifest.source,
            "transforms": manifest.transforms,
            "media": manifest.scan.media,
            "detector": manifest.scan.detector,
            "detector_version": manifest.scan.detector_version,
            "applicable_detectors": manifest.scan.applicable_detectors,
            "detector_outcomes": manifest.scan.detector_outcomes,
            "song_detector": song.detector_id(),
            "scan_status": manifest.scan.status,
            "engine": song.engine(),
            "song_offset": span.effective_offset,
        });
        let song = match song {
            SongRef::EngineSoftware(song) => {
                metadata["song"] = serde_json::to_value(song)?;
                metadata["limitations"] = serde_json::to_value(&song.warnings)?;
                TrackerSong::EngineSoftware(Box::new(song.clone()))
            }
            SongRef::Gax(song) => {
                metadata["song"] = serde_json::to_value(song)?;
                metadata["limitations"] = json!([
                    "GAX 3 graph inventory preserves referenced source structures and original unsigned sample points.",
                    "XM is a projection to FastTracker II playback. Envelope rounding and tracker/player behavior can differ; original engine execution is not reproduced.",
                    "XM export is unavailable for unresolved instrument modulation, pattern effects or unsupported tracker limits."
                ]);
                TrackerSong::Gax(Box::new(song.clone()))
            }
            SongRef::Module(song) => {
                match song.source {
                    super::tracker::ModuleSource::Embedded => ensure!(
                        input.system.is_some() && input.standalone_audio.is_none(),
                        "embedded tracker export requires a cartridge scan input"
                    ),
                    super::tracker::ModuleSource::Standalone { trailing_bytes } => ensure!(
                        input.system.is_none()
                            && input.standalone_audio
                                == Some(super::media::StandaloneFormat::Tracker(song.format))
                            && song.span.offset == 0
                            && song.span.byte_len as usize + trailing_bytes as usize
                                == input.bytes.len(),
                        "standalone tracker export does not match its source input"
                    ),
                }
                metadata["song"] = serde_json::to_value(song)?;
                metadata["limitations"] = match song.source {
                    super::tracker::ModuleSource::Embedded => json!([
                        "The validated embedded module extent is preserved byte for byte, including referenced patterns, samples and supported metadata. Trailing nonstandard chunks and unrelated containing-file bytes are not included.",
                        "Module recognition does not identify the console's native driver or prove the game plays this module."
                    ]),
                    super::tracker::ModuleSource::Standalone { trailing_bytes } => json!([
                        "The complete standalone source file is preserved byte for byte, including bytes after the validated module extent.",
                        format!(
                            "{trailing_bytes} trailing source bytes are outside the validated module structure and are retained without interpretation."
                        ),
                        "Standalone module recognition does not attribute the file to a console driver or reproduce tracker playback."
                    ]),
                };
                TrackerSong::Embedded(song.clone())
            }
            SongRef::Mp2k(_)
            | SongRef::Cdda(_)
            | SongRef::Gb(_)
            | SongRef::Nes(_)
            | SongRef::Natsume(_)
            | SongRef::Vgm(_)
            | SongRef::Rip(_) => unreachable!("handled above"),
            SongRef::Krawall(_)
            | SongRef::GaxNative(_)
            | SongRef::Musyx(_)
            | SongRef::Aas(_)
            | SongRef::DescriptorMidi(_)
            | SongRef::Nsq(_)
            | SongRef::Radriver(_)
            | SongRef::Gbass(_)
            | SongRef::AasStream(_)
            | SongRef::AasPcm(_)
            | SongRef::NesNative(_)
            | SongRef::GbNative(_)
            | SongRef::SegaPsg(_) => {
                unreachable!("handled above")
            }
        };
        ensure!(
            serde_json::to_vec(&metadata)?.len() <= 32 * 1024 * 1024,
            "tracker metadata exceeds its size limit"
        );
        Ok(Self::Tracker(TrackerExportRequest {
            bytes: Arc::clone(&input.bytes),
            sha256,
            song,
            format,
            metadata,
        }))
    }

    pub(crate) fn write_new(
        self,
        path: &Path,
        cancel: &AtomicBool,
        progress: &AtomicU32,
    ) -> Result<()> {
        match self {
            Self::Mp2k(request) => request.write_new(path, cancel, progress),
            Self::Gsf(request) => request.write_new(path, cancel, progress),
            Self::NativeGsf(request) => request.write_new(path, cancel, progress),
            Self::NativeRip(request) => request.write_new(path, cancel, progress),
            Self::Gb(request) => request.write_new(path, cancel, progress),
            Self::Nes(request) => request.write_new(path, cancel, progress),
            Self::Natsume(request) => request.write_new(path, cancel, progress),
            Self::Vgm(request) => request.write_new(path, cancel, progress),
            Self::Rip(request) => request.write_new(path, cancel, progress),
            Self::Tracker(request) => request.write_new(path, cancel, progress),
            Self::Pcm(request) => request.write_new(path, cancel, progress),
            Self::Cdda {
                input,
                number,
                format,
            } => {
                super::cdda::write_new(&input, number, format, path, cancel)?;
                progress.store(100, Ordering::Relaxed);
                Ok(())
            }
        }
    }
}

impl TrackerExportRequest {
    fn write_new(mut self, path: &Path, cancel: &AtomicBool, progress: &AtomicU32) -> Result<()> {
        ensure!(!cancel.load(Ordering::Relaxed), "tracker export cancelled");
        ensure!(
            zeff_firmware::sha256_hex(&self.bytes) == self.sha256,
            "loaded media does not match the scan SHA-256 identity"
        );
        let (data, extension) = match &self.song {
            TrackerSong::EngineSoftware(song) => {
                if self.format == SongFormat::MappedAssets {
                    let mut bundle = super::bundle::Bundle::new();
                    for span in &song.mapped_spans {
                        ensure!(!cancel.load(Ordering::Relaxed), "tracker export cancelled");
                        let start = span.effective_offset as usize;
                        let data = self
                            .bytes
                            .get(start..start + span.byte_len as usize)
                            .context("mapped Engine Software range is outside source media")?;
                        bundle.add(
                            &format!("source/{start:08x}-{:08x}.bin", span.byte_len),
                            data,
                        )?;
                    }
                    bundle.add("manifest.json", &serde_json::to_vec_pretty(&self.metadata)?)?;
                    return super::assets::publish_bytes(path, &bundle.finish()?, cancel, progress);
                }
                (
                    super::engine_software::to_xm(&self.bytes, song, cancel)?,
                    "xm",
                )
            }
            TrackerSong::Embedded(song) => {
                super::tracker::verify_original(&self.bytes, song, cancel)?;
                let data = match song.source {
                    super::tracker::ModuleSource::Embedded => {
                        let start = song.span.offset as usize;
                        let end = start + song.span.byte_len as usize;
                        self.bytes
                            .get(start..end)
                            .context("module is outside source media")?
                            .to_vec()
                    }
                    super::tracker::ModuleSource::Standalone { .. } => self.bytes.to_vec(),
                };
                (data, song.format.extension())
            }
            TrackerSong::Gax(song) if self.format == SongFormat::MappedAssets => {
                self.metadata["kind"] = json!("mapped_original_gax_assets");
                let mut bundle = super::bundle::Bundle::new();
                for span in &song.mapped_spans {
                    ensure!(!cancel.load(Ordering::Relaxed), "tracker export cancelled");
                    let start = span.effective_offset as usize;
                    let end = start + span.byte_len as usize;
                    let data = self
                        .bytes
                        .get(start..end)
                        .context("mapped GAX range is outside source media")?;
                    bundle.add(
                        &format!("source/{start:08x}-{:08x}.bin", span.byte_len),
                        data,
                    )?;
                }
                bundle.add("manifest.json", &serde_json::to_vec_pretty(&self.metadata)?)?;
                let data = bundle.finish()?;
                return super::assets::publish_bytes(path, &data, cancel, progress);
            }
            TrackerSong::Gax(song) => {
                let module = super::gax::project(&self.bytes, song, cancel)?;
                (super::tracker::xm::encode(&module, cancel)?, "xm")
            }
        };
        progress.store(90, Ordering::Relaxed);
        self.metadata["module_sha256"] = json!(zeff_firmware::sha256_hex(&data));
        let data = if matches!(
            self.format,
            SongFormat::TrackerPack | SongFormat::MappedAssets
        ) {
            let mut bundle = super::bundle::Bundle::new();
            bundle.add(&format!("song.{extension}"), &data)?;
            bundle.add("manifest.json", &serde_json::to_vec_pretty(&self.metadata)?)?;
            bundle.finish()?
        } else {
            data
        };
        super::assets::publish_bytes(path, &data, cancel, progress)
    }
}
