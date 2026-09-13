use std::{
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU32},
    },
};

use anyhow::{Context, Result, ensure};
use zeff_audio_discovery::{
    catalog::{SongId, SongRef},
    native_rips,
};

use super::{
    extract::ExtractionRequest,
    formats::SongFormat,
    media::{ScanInput, ScanManifest},
};

enum Selection {
    GbNative(Box<zeff_audio_discovery::gb_native::GbNativeSong>),
    Nes(Box<zeff_audio_discovery::nes_native::NesNativeSong>),
    Sega(Box<zeff_audio_discovery::sega_psg::SegaPsgSong>),
}

impl Selection {
    fn as_ref(&self) -> SongRef<'_> {
        match self {
            Self::GbNative(song) => SongRef::GbNative(song),
            Self::Nes(song) => SongRef::NesNative(song),
            Self::Sega(song) => SongRef::SegaPsg(song),
        }
    }
}

pub(crate) struct NativeRipRequest {
    bytes: Arc<[u8]>,
    sha256: String,
    selection: Selection,
}

impl NativeRipRequest {
    pub(crate) fn prepare(
        input: &ScanInput,
        manifest: &ScanManifest,
        id: SongId,
        format: SongFormat,
    ) -> Result<Self> {
        let song = manifest
            .scan
            .song(id)
            .context("select a song before exporting")?;
        let native =
            native_rips::supported_format(song).context("selection has no native music rip")?;
        ensure!(
            native.extension() == format.info().extension,
            "native music rip format does not match selection"
        );
        ExtractionRequest::prepare(
            input,
            manifest,
            song.span().context("song has no source range")?,
            "Native music rip",
        )?;
        let selection = match song {
            SongRef::GbNative(song) => Selection::GbNative(Box::new(song.clone())),
            SongRef::NesNative(song) => Selection::Nes(Box::new(song.clone())),
            SongRef::SegaPsg(song) => Selection::Sega(Box::new(song.clone())),
            _ => anyhow::bail!("selection has no native music rip"),
        };
        Ok(Self {
            bytes: Arc::clone(&input.bytes),
            sha256: manifest
                .scan
                .media
                .sha256
                .clone()
                .context("scan has no identity")?,
            selection,
        })
    }

    pub(crate) fn write_new(
        self,
        path: &Path,
        cancel: &AtomicBool,
        progress: &AtomicU32,
    ) -> Result<()> {
        ensure!(
            zeff_firmware::sha256_hex(&self.bytes) == self.sha256,
            "native music rip source identity changed"
        );
        let rip = native_rips::encode(&self.bytes, self.selection.as_ref(), cancel)?;
        super::assets::publish_bytes(path, &rip.bytes, cancel, progress)
    }
}
