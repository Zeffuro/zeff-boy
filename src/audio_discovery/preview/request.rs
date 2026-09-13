use std::sync::{Arc, atomic::AtomicBool};

use anyhow::{Context, Result, ensure};

use crate::audio_discovery::{
    SongCandidate, assets,
    catalog::{SongId, SongRef},
    cdda::{self, CdAudioTrack, preview::NativeRenderSession as CddaRenderSession},
    extract::ExtractionRequest,
    media::{ScanInput, ScanManifest},
    natsume::{NatsumeSong, NatsumeSongKind, preview::NativeRenderSession},
    pcm::{PcmSession, song::PcmSong},
    projection,
    render::{RenderOptions, RenderSession},
    sf2,
};

pub(crate) struct PreviewRequest {
    source: Arc<ScanInput>,
    song: PreviewSong,
    sha256: String,
    mixer_rate: Option<u32>,
    options: RenderOptions,
}

enum PreviewSong {
    Mp2k(SongCandidate),
    Natsume(NatsumeSong),
    Cdda(CdAudioTrack),
    Pcm(PcmSong),
}

impl PreviewRequest {
    pub(crate) fn can_preview(manifest: &ScanManifest, selection: SongId) -> bool {
        manifest.scan.song(selection).is_some_and(|song| {
            PcmSong::can_play(song)
                || matches!(song, SongRef::Mp2k(_) | SongRef::Cdda(_))
                || matches!(song, SongRef::Natsume(song) if song.kind == NatsumeSongKind::Music)
        })
    }

    pub(super) fn seek_step(&self) -> usize {
        if matches!(self.song, PreviewSong::Cdda(_)) {
            1
        } else {
            8
        }
    }

    pub(crate) fn prepare(
        source: &Arc<ScanInput>,
        manifest: &ScanManifest,
        index: usize,
        options: RenderOptions,
    ) -> Result<Self> {
        let song = manifest
            .scan
            .candidates
            .get(index)
            .context("select an MP2k song to preview")?;
        ExtractionRequest::prepare(source, manifest, song.header, "Song preview")?;
        Ok(Self {
            source: Arc::clone(source),
            song: PreviewSong::Mp2k(song.clone()),
            sha256: manifest
                .scan
                .media
                .sha256
                .clone()
                .context("scan has no ROM identity")?,
            mixer_rate: assets::infer_mixer_rate(&source.bytes, manifest, song).0,
            options,
        })
    }

    pub(crate) fn prepare_song(
        source: &Arc<ScanInput>,
        manifest: &ScanManifest,
        selection: SongId,
        options: RenderOptions,
    ) -> Result<Self> {
        if let Some(song) = manifest.scan.song(selection)
            && let Some(owned) = PcmSong::from_ref(song)
        {
            ExtractionRequest::prepare(
                source,
                manifest,
                song.span().context("song has no mapped source range")?,
                "Song preview",
            )?;
            return Ok(Self {
                source: Arc::clone(source),
                song: PreviewSong::Pcm(owned),
                sha256: manifest
                    .scan
                    .media
                    .sha256
                    .clone()
                    .context("scan has no media identity")?,
                mixer_rate: None,
                options,
            });
        }
        match selection {
            SongId::Mp2k(index) => Self::prepare(source, manifest, index, options),
            SongId::Natsume(index) => {
                let song = manifest
                    .scan
                    .natsume_songs
                    .get(index)
                    .context("select a Natsume song to preview")?;
                ensure!(
                    song.kind == NatsumeSongKind::Music,
                    "this driver control entry produces no music; select a music entry to preview"
                );
                ExtractionRequest::prepare(source, manifest, song.header, "Song preview")?;
                Ok(Self {
                    source: Arc::clone(source),
                    song: PreviewSong::Natsume(song.clone()),
                    sha256: manifest
                        .scan
                        .media
                        .sha256
                        .clone()
                        .context("scan has no ROM identity")?,
                    mixer_rate: None,
                    options,
                })
            }
            SongId::Cdda(index) => {
                let track = *manifest
                    .scan
                    .cdda_tracks
                    .get(index)
                    .context("select a CD audio track to preview")?;
                let disc = cdda::input_for_song(source, manifest, track)?;
                Ok(Self {
                    source: Arc::clone(source),
                    song: PreviewSong::Cdda(track),
                    sha256: disc.effective_disc_sha256.clone(),
                    mixer_rate: None,
                    options,
                })
            }
            _ => anyhow::bail!("preview is unavailable for this audio format"),
        }
    }

    pub(super) fn renderer(self, rate: u32, cancel: &AtomicBool) -> Result<PreviewRenderer> {
        if let PreviewSong::Cdda(track) = self.song {
            return CddaRenderSession::new(
                self.source
                    .cdda
                    .as_ref()
                    .context("CD audio source disappeared")?,
                track,
                rate,
                cancel,
            )
            .map(|renderer| PreviewRenderer::Cdda(Box::new(renderer)));
        }
        ensure!(
            zeff_firmware::sha256_hex(&self.source.bytes) == self.sha256,
            "loaded ROM does not match the scan SHA-256 identity"
        );
        let song = match self.song {
            PreviewSong::Pcm(song) => {
                return song
                    .session(
                        &self.source.bytes,
                        RenderOptions {
                            sample_rate: rate,
                            ..self.options
                        },
                        cancel,
                    )
                    .map(PreviewRenderer::Pcm);
            }
            PreviewSong::Natsume(song) => {
                return NativeRenderSession::new(
                    &self.source.bytes,
                    &song,
                    rate,
                    u32::from(self.options.max_seconds),
                    cancel,
                )
                .map(|renderer| PreviewRenderer::Natsume(Box::new(renderer)));
            }
            PreviewSong::Mp2k(song) => song,
            PreviewSong::Cdda(_) => unreachable!("CD audio handled above"),
        };
        let bank = projection::instrument_bank_with_cancel(
            &self.source.bytes,
            &song,
            "Independent MP2k preview".to_owned(),
            self.mixer_rate,
            cancel,
        )?;
        let soundfont = sf2::encode(&bank)?;
        drop(bank);
        RenderSession::new(
            &song,
            &self.source.bytes,
            &soundfont,
            RenderOptions {
                sample_rate: rate,
                ..self.options
            },
            cancel,
        )
        .map(|renderer| PreviewRenderer::Mp2k(Box::new(renderer)))
    }
}

pub(super) enum PreviewRenderer {
    Mp2k(Box<RenderSession>),
    Natsume(Box<NativeRenderSession>),
    Cdda(Box<CddaRenderSession>),
    Pcm(Box<dyn PcmSession>),
}

impl PreviewRenderer {
    pub(super) fn duration_frames(&self) -> usize {
        match self {
            Self::Mp2k(renderer) => renderer.duration_frames(),
            Self::Natsume(renderer) => renderer.duration_frames(),
            Self::Cdda(renderer) => renderer.duration_frames(),
            Self::Pcm(renderer) => renderer.duration_frames(),
        }
    }
    pub(super) fn position_frames(&self) -> usize {
        match self {
            Self::Mp2k(renderer) => renderer.position_frames(),
            Self::Natsume(renderer) => renderer.position_frames(),
            Self::Cdda(renderer) => renderer.position_frames(),
            Self::Pcm(renderer) => renderer.position_frames(),
        }
    }
    pub(super) fn sample_rate(&self) -> u32 {
        match self {
            Self::Mp2k(renderer) => renderer.sample_rate(),
            Self::Natsume(renderer) => renderer.sample_rate(),
            Self::Cdda(renderer) => renderer.sample_rate(),
            Self::Pcm(renderer) => renderer.sample_rate(),
        }
    }
    pub(super) fn track_count(&self) -> usize {
        match self {
            Self::Mp2k(renderer) => renderer.track_count(),
            Self::Natsume(renderer) => renderer.track_count(),
            Self::Cdda(renderer) => renderer.track_count(),
            Self::Pcm(renderer) => renderer.track_count(),
        }
    }
    pub(super) fn warnings(&self) -> &[String] {
        match self {
            Self::Mp2k(renderer) => renderer.warnings(),
            Self::Natsume(renderer) => renderer.warnings(),
            Self::Cdda(renderer) => renderer.warnings(),
            Self::Pcm(renderer) => renderer.warnings(),
        }
    }
    pub(super) fn reset(&mut self) -> Result<()> {
        match self {
            Self::Mp2k(renderer) => renderer.reset(),
            Self::Natsume(renderer) => renderer.reset(),
            Self::Cdda(renderer) => renderer.reset(),
            Self::Pcm(renderer) => renderer.reset(),
        }
    }
    pub(super) fn set_track_mask(&mut self, mask: u16) -> Result<()> {
        match self {
            Self::Mp2k(renderer) => renderer.set_track_mask(mask),
            Self::Natsume(renderer) => renderer.set_track_mask(mask),
            Self::Cdda(renderer) => renderer.set_track_mask(mask),
            Self::Pcm(renderer) => renderer.set_track_mask(mask),
        }
    }
    pub(super) fn read(&mut self, output: &mut [i16], cancel: &AtomicBool) -> Result<usize> {
        match self {
            Self::Mp2k(renderer) => renderer.read(output, cancel),
            Self::Natsume(renderer) => renderer.read(output, cancel),
            Self::Cdda(renderer) => renderer.read(output, cancel),
            Self::Pcm(renderer) => renderer.read(output, cancel),
        }
    }

    pub(super) fn seek_direct(&mut self, frame: usize) -> Result<bool> {
        if let Self::Cdda(renderer) = self {
            renderer.seek_frames(frame)?;
            Ok(true)
        } else {
            Ok(false)
        }
    }
}
