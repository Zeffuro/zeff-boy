use std::path::Path;
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU32},
};

use super::super::{check_cancel, validate_options, write_new};
use super::PcmSong;
use crate::audio_discovery::{
    catalog::SongRef,
    extract::ExtractionRequest,
    formats::SongFormat,
    media::{ScanInput, ScanManifest},
    render::RenderOptions,
};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};

pub(crate) struct PcmExportRequest {
    bytes: Arc<[u8]>,
    sha256: String,
    song: PcmSong,
    format: SongFormat,
    options: RenderOptions,
    metadata: Value,
}

impl PcmExportRequest {
    pub(crate) fn prepare(
        input: &ScanInput,
        manifest: &ScanManifest,
        song: SongRef<'_>,
        format: SongFormat,
        options: RenderOptions,
    ) -> Result<Self> {
        ensure!(
            matches!(format, SongFormat::Audio(_) | SongFormat::MappedAssets)
                || (format == SongFormat::Midi && matches!(song, SongRef::DescriptorMidi(_))),
            "unsupported PCM export format"
        );
        ensure!(
            !matches!(song, SongRef::Gb(_) | SongRef::Nes(_))
                || matches!(format, SongFormat::Audio(_)),
            "Sequence MIDI and mapped assets require the sequence export path"
        );
        if matches!(format, SongFormat::Audio(_)) {
            validate_options(options)?;
        }
        if matches!(song, SongRef::Vgm(_)) {
            ensure!(
                matches!(format, SongFormat::Audio(_))
                    && input.system.is_none()
                    && input.cdda.is_none()
                    && input.standalone_audio
                        == Some(crate::audio_discovery::media::StandaloneFormat::Vgm),
                "VGM playback requires a standalone register log and an audio output"
            );
        }
        let span = song.span().context("song has no mapped source range")?;
        ExtractionRequest::prepare(input, manifest, span, "Audio recording")?;
        let owned = PcmSong::from_ref(song).context("this song has no PCM player")?;
        Ok(Self {
            bytes: Arc::clone(&input.bytes),
            sha256: manifest
                .scan
                .media
                .sha256
                .clone()
                .context("scan has no media identity")?,
            metadata: json!({
                "classification": manifest.classification(song),
                "schema": "zeff-engine-audio-export/1",
                "analysis_profile": manifest.analysis_profile,
                "source": manifest.source, "transforms": manifest.transforms,
                "media": manifest.scan.media, "song_detector": song.detector_id(),
                "detector_version": manifest.scan.detector_version,
                "scan_status": manifest.scan.status, "selection": owned,
            }),
            song: owned,
            format,
            options,
        })
    }

    pub(crate) fn write_new(
        self,
        path: &Path,
        cancel: &AtomicBool,
        progress: &AtomicU32,
    ) -> Result<()> {
        check_cancel(cancel)?;
        ensure!(
            zeff_firmware::sha256_hex(&self.bytes) == self.sha256,
            "loaded media does not match the scan SHA-256 identity"
        );
        if self.format == SongFormat::Midi {
            let PcmSong::DescriptorMidi(song) = &self.song else {
                unreachable!("validated native MIDI export")
            };
            let midi =
                zeff_audio_discovery::descriptor_midi::midi_bytes(&self.bytes, song, cancel)?;
            return crate::audio_discovery::assets::publish_bytes(path, &midi, cancel, progress);
        }
        if self.format == SongFormat::MappedAssets {
            if let PcmSong::GbNative(song) = &self.song {
                zeff_audio_discovery::gb_native::prepare_rom(&self.bytes, song, cancel)?;
            }
            if let PcmSong::NesNative(song) = &self.song {
                zeff_audio_discovery::nes_native::prepare_rom(&self.bytes, song, cancel)?;
            }
            if let PcmSong::GbGhx(song) = &self.song {
                zeff_audio_discovery::gb_ghx::validate_song(&self.bytes, song, cancel)?;
            }
            if let PcmSong::GbSoundSystem(song) = &self.song {
                zeff_audio_discovery::gb_sound_system::validate_song(&self.bytes, song, cancel)?;
            }
            if let PcmSong::GbCarillon(song) = &self.song {
                zeff_audio_discovery::gb_carillon::validate_song(&self.bytes, song, cancel)?;
            }
            if let PcmSong::WsTose(song) = &self.song {
                zeff_audio_discovery::ws_tose::validate_song(&self.bytes, song, cancel)?;
            }
            if let PcmSong::GbTose(song) = &self.song {
                zeff_audio_discovery::gb_tose::validate_song(&self.bytes, song, cancel)?;
            }
            if let PcmSong::GbQuickThunder(song) = &self.song {
                zeff_audio_discovery::gb_quickthunder::validate_song(&self.bytes, song, cancel)?;
            }
            if let PcmSong::NesTose(song) = &self.song {
                zeff_audio_discovery::nes_tose::validate_song(&self.bytes, song, cancel)?;
            }
            if let PcmSong::GbMusyx(song) = &self.song {
                zeff_audio_discovery::gb_musyx::validate_song(&self.bytes, song, cancel)?;
            }
            let mut bundle = crate::audio_discovery::bundle::Bundle::new();
            for span in self
                .song
                .mapped_spans()
                .context("song has a separate mapped-asset exporter")?
            {
                check_cancel(cancel)?;
                let start = span.effective_offset as usize;
                let data = self
                    .bytes
                    .get(start..start + span.byte_len as usize)
                    .context("mapped source range is outside source media")?;
                bundle.add(
                    &format!("source/{start:08x}-{:08x}.bin", span.byte_len),
                    data,
                )?;
            }
            bundle.add("manifest.json", &serde_json::to_vec_pretty(&self.metadata)?)?;
            return crate::audio_discovery::assets::publish_bytes(
                path,
                &bundle.finish()?,
                cancel,
                progress,
            );
        }
        let SongFormat::Audio(format) = self.format else {
            unreachable!("validated export format")
        };
        let session = self.song.session(&self.bytes, self.options, cancel)?;
        write_new(
            session,
            format,
            self.options,
            self.metadata,
            path,
            cancel,
            progress,
        )
    }
}
