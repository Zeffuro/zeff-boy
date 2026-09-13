use std::{
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU32},
    },
};

use anyhow::{Context, Result, ensure};
use serde::Serialize;
use serde_json::{Value, json};

use super::{PcmSession, tracker::TrackerSession};
use crate::audio_discovery::{
    RomSpan,
    catalog::SongRef,
    extract::ExtractionRequest,
    formats::SongFormat,
    media::{ScanInput, ScanManifest},
    render::RenderOptions,
};

#[derive(Clone, Serialize)]
#[serde(tag = "engine", content = "song", rename_all = "snake_case")]
pub(crate) enum PcmSong {
    EngineSoftware(Box<zeff_audio_discovery::engine_software::EngineSoftwareSong>),
    Gax(Box<zeff_audio_discovery::gax::GaxSong>),
    Krawall(Box<zeff_audio_discovery::krawall::KrawallSong>),
    GaxNative(Box<zeff_audio_discovery::gax_native::GaxNativeSong>),
    Musyx(Box<zeff_audio_discovery::musyx::MusyxSong>),
    Aas(Box<zeff_audio_discovery::aas::AasSong>),
    DescriptorMidi(Box<zeff_audio_discovery::descriptor_midi::DescriptorMidiSong>),
    Nsq(Box<zeff_audio_discovery::nsq::NsqSong>),
    Radriver(Box<zeff_audio_discovery::radriver::RadriverSong>),
    Gbass(Box<zeff_audio_discovery::gbass::GbassSong>),
    AasStream(Box<zeff_audio_discovery::aas_stream::AasStreamSong>),
    AasPcm(Box<zeff_audio_discovery::aas_pcm::AasPcmSong>),
    NesNative(Box<zeff_audio_discovery::nes_native::NesNativeSong>),
    GbNative(Box<zeff_audio_discovery::gb_native::GbNativeSong>),
    GbBanked(Box<zeff_audio_discovery::gb_music::GbSong>),
    SegaPsg(Box<zeff_audio_discovery::sega_psg::SegaPsgSong>),
}

impl PcmSong {
    pub(crate) fn can_play(song: SongRef<'_>) -> bool {
        matches!(
            song,
            SongRef::EngineSoftware(_)
                | SongRef::Krawall(_)
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
                | SongRef::SegaPsg(_)
        ) || matches!(song, SongRef::Gax(song) if song.xm_exportable)
            || matches!(song, SongRef::Gb(song) if zeff_audio_discovery::gb_music::native::supports_native(song))
    }
    pub(crate) fn is_native(song: SongRef<'_>) -> bool {
        matches!(
            song,
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
                | SongRef::SegaPsg(_)
        ) || matches!(song, SongRef::Gb(song) if zeff_audio_discovery::gb_music::native::supports_native(song))
    }
    pub(crate) fn from_ref(song: SongRef<'_>) -> Option<Self> {
        match song {
            SongRef::EngineSoftware(song) => Some(Self::EngineSoftware(Box::new(song.clone()))),
            SongRef::Gax(song) if song.xm_exportable => Some(Self::Gax(Box::new(song.clone()))),
            SongRef::Krawall(song) => Some(Self::Krawall(Box::new(song.clone()))),
            SongRef::GaxNative(song) => Some(Self::GaxNative(Box::new(song.clone()))),
            SongRef::Musyx(song) => Some(Self::Musyx(Box::new(song.clone()))),
            SongRef::Aas(song) => Some(Self::Aas(Box::new(song.clone()))),
            SongRef::DescriptorMidi(song) => Some(Self::DescriptorMidi(Box::new(song.clone()))),
            SongRef::Nsq(song) => Some(Self::Nsq(Box::new(song.clone()))),
            SongRef::Radriver(song) => Some(Self::Radriver(Box::new(song.clone()))),
            SongRef::Gbass(song) => Some(Self::Gbass(Box::new(song.clone()))),
            SongRef::AasStream(song) => Some(Self::AasStream(Box::new(song.clone()))),
            SongRef::AasPcm(song) => Some(Self::AasPcm(Box::new(song.clone()))),
            SongRef::NesNative(song) => Some(Self::NesNative(Box::new(song.clone()))),
            SongRef::GbNative(song) => Some(Self::GbNative(Box::new(song.clone()))),
            SongRef::Gb(song) if zeff_audio_discovery::gb_music::native::supports_native(song) => {
                Some(Self::GbBanked(Box::new(song.clone())))
            }
            SongRef::SegaPsg(song) => Some(Self::SegaPsg(Box::new(song.clone()))),
            _ => None,
        }
    }

    pub(crate) fn session(
        &self,
        bytes: &[u8],
        options: RenderOptions,
        cancel: &AtomicBool,
    ) -> Result<Box<dyn PcmSession>> {
        super::validate_options(options)?;
        super::check_cancel(cancel)?;
        let (xm, warnings) = match self {
            Self::GbBanked(song) => {
                let prepared =
                    zeff_audio_discovery::gb_music::native::prepare_rom(bytes, song, cancel)?;
                return Ok(Box::new(super::gb_banked::GbBankedSession::new(
                    prepared,
                    options,
                    Vec::new(),
                    cancel,
                )?));
            }
            Self::GbNative(song) => {
                let prepared = zeff_audio_discovery::gb_native::prepare_rom(bytes, song, cancel)?;
                return Ok(Box::new(super::gb::GbSession::new(
                    prepared,
                    options,
                    song.warnings.clone(),
                    cancel,
                )?));
            }
            Self::NesNative(song) => {
                let prepared = zeff_audio_discovery::nes_native::prepare_rom(bytes, song, cancel)?;
                return Ok(Box::new(super::nes::NesSession::new(
                    prepared,
                    options,
                    song.warnings.clone(),
                    cancel,
                )?));
            }
            Self::SegaPsg(song) => {
                let prepared = zeff_audio_discovery::sega_psg::prepare_rom(bytes, song, cancel)?;
                return Ok(Box::new(super::sega::SegaSession::new(
                    prepared,
                    options,
                    song.warnings.clone(),
                    cancel,
                )?));
            }
            Self::Nsq(song) => {
                let prepared = zeff_audio_discovery::nsq::prepare_rom(bytes, song, cancel)?;
                return Ok(Box::new(super::gba::GbaSession::new_ready(
                    prepared.bytes,
                    prepared.wait_loop,
                    options,
                    song.warnings.clone(),
                    cancel,
                )?));
            }
            Self::DescriptorMidi(song) => {
                let prepared =
                    zeff_audio_discovery::descriptor_midi::prepare_rom(bytes, song, cancel)?;
                return Ok(Box::new(super::gba::GbaSession::new_ready(
                    prepared.bytes,
                    prepared.wait_loop,
                    options,
                    song.warnings.clone(),
                    cancel,
                )?));
            }
            Self::Aas(song) => {
                let prepared = zeff_audio_discovery::aas::prepare_rom(bytes, song, cancel)?;
                return Ok(Box::new(super::gba::GbaSession::new_ready(
                    prepared.bytes,
                    prepared.wait_loop,
                    options,
                    song.warnings.clone(),
                    cancel,
                )?));
            }
            Self::AasPcm(song) => {
                let prepared = zeff_audio_discovery::aas_pcm::prepare_rom(bytes, song, cancel)?;
                return Ok(Box::new(super::gba::GbaSession::new_ready(
                    prepared.bytes,
                    prepared.wait_loop,
                    options,
                    song.warnings.clone(),
                    cancel,
                )?));
            }
            Self::AasStream(song) => {
                let prepared = zeff_audio_discovery::aas_stream::prepare_rom(bytes, song, cancel)?;
                return Ok(Box::new(super::gba::GbaSession::new_ready(
                    prepared.bytes,
                    prepared.wait_loop,
                    options,
                    song.warnings.clone(),
                    cancel,
                )?));
            }
            Self::Gbass(song) => {
                let prepared = zeff_audio_discovery::gbass::prepare_rom(bytes, song, cancel)?;
                return Ok(Box::new(super::gba::GbaSession::new_ready(
                    prepared.bytes,
                    prepared.wait_loop,
                    options,
                    song.warnings.clone(),
                    cancel,
                )?));
            }
            Self::Radriver(song) => {
                let prepared = zeff_audio_discovery::radriver::prepare_rom(bytes, song, cancel)?;
                return Ok(Box::new(super::gba::GbaSession::new_ready(
                    prepared.bytes,
                    prepared.wait_loop,
                    options,
                    song.warnings.clone(),
                    cancel,
                )?));
            }
            Self::Musyx(song) => {
                let prepared = zeff_audio_discovery::musyx::prepare_rom(bytes, song, cancel)?;
                return Ok(Box::new(super::gba::GbaSession::new_ready(
                    prepared.bytes,
                    prepared.wait_loop,
                    options,
                    song.warnings.clone(),
                    cancel,
                )?));
            }
            Self::Krawall(song) => {
                let rom = zeff_audio_discovery::krawall::prepare_rom(bytes, song, cancel)?;
                return Ok(Box::new(super::gba::GbaSession::new(
                    rom,
                    options,
                    song.warnings.clone(),
                    cancel,
                )?));
            }
            Self::GaxNative(song) => {
                let rom = zeff_audio_discovery::gax_native::prepare_rom(bytes, song, cancel)?;
                return Ok(Box::new(super::gba::GbaSession::new(
                    rom,
                    options,
                    song.warnings.clone(),
                    cancel,
                )?));
            }
            Self::EngineSoftware(song) => (
                zeff_audio_discovery::engine_software::to_xm(bytes, song, cancel)?,
                song.warnings.clone(),
            ),
            Self::Gax(song) => {
                let module = zeff_audio_discovery::gax::project(bytes, song, cancel)?;
                (
                    zeff_audio_discovery::tracker::xm::encode(&module, cancel)?,
                    song.warnings
                        .iter()
                        .map(|w| format!("+{:06X}: {}", w.offset, w.reason))
                        .collect(),
                )
            }
        };
        Ok(Box::new(TrackerSession::from_xm(
            &xm, options, warnings, cancel,
        )?))
    }

    fn mapped_spans(&self) -> Option<&[RomSpan]> {
        Some(match self {
            Self::GbBanked(_) => return None,
            Self::EngineSoftware(song) => &song.mapped_spans,
            Self::Gax(song) => &song.mapped_spans,
            Self::Krawall(song) => &song.mapped_spans,
            Self::GaxNative(song) => &song.mapped_spans,
            Self::Musyx(song) => &song.mapped_spans,
            Self::Aas(song) => &song.mapped_spans,
            Self::DescriptorMidi(song) => &song.mapped_spans,
            Self::Nsq(song) => &song.mapped_spans,
            Self::Radriver(song) => &song.mapped_spans,
            Self::Gbass(song) => &song.mapped_spans,
            Self::AasStream(song) => &song.mapped_spans,
            Self::AasPcm(song) => &song.mapped_spans,
            Self::NesNative(song) => &song.mapped_spans,
            Self::GbNative(song) => &song.mapped_spans,
            Self::SegaPsg(song) => &song.mapped_spans,
        })
    }
}

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
            !matches!(song, SongRef::Gb(_)) || matches!(format, SongFormat::Audio(_)),
            "Game Boy MIDI and mapped assets require the sequence export path"
        );
        if matches!(format, SongFormat::Audio(_)) {
            super::validate_options(options)?;
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
        super::check_cancel(cancel)?;
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
            let mut bundle = crate::audio_discovery::bundle::Bundle::new();
            for span in self
                .song
                .mapped_spans()
                .context("song has a separate mapped-asset exporter")?
            {
                super::check_cancel(cancel)?;
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
        super::write_new(
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
