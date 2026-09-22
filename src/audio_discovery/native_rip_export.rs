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

#[cfg(test)]
mod tests;

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
    format: native_rips::NativeRipFormat,
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
        let native = match format {
            SongFormat::Gbs => native_rips::NativeRipFormat::Gbs,
            SongFormat::Nsf => native_rips::NativeRipFormat::Nsf,
            SongFormat::Nsfe => native_rips::NativeRipFormat::Nsfe,
            SongFormat::Sgc => native_rips::NativeRipFormat::Sgc,
            _ => anyhow::bail!("selection has no native music rip in this format"),
        };
        ensure!(
            native_rips::supports_format(song, native),
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
            format: native,
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
        let rip =
            native_rips::encode_as(&self.bytes, self.selection.as_ref(), self.format, cancel)?;
        super::assets::publish_bytes(path, &rip.bytes, cancel, progress)
    }
}
