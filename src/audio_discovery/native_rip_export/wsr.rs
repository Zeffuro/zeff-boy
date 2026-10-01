use std::{
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU32},
    },
};

use anyhow::{Context, Result, ensure};
use zeff_audio_discovery::ws_tose::{WsToseSong, wsr};

use crate::audio_discovery::{
    catalog::SongRef,
    extract::ExtractionRequest,
    media::{ScanInput, ScanManifest},
};

#[cfg(test)]
mod tests;

pub(crate) struct WsrRequest {
    bytes: Arc<[u8]>,
    sha256: String,
    song: WsToseSong,
}

impl WsrRequest {
    pub(crate) fn prepare(
        input: &ScanInput,
        manifest: &ScanManifest,
        song: &WsToseSong,
    ) -> Result<Self> {
        ensure!(
            wsr::supported(song),
            "selection has no qualified WSR export"
        );
        ExtractionRequest::prepare(
            input,
            manifest,
            SongRef::WsTose(song)
                .span()
                .context("song has no source range")?,
            "WSR music rip",
        )?;
        Ok(Self {
            bytes: Arc::clone(&input.bytes),
            sha256: manifest
                .scan
                .media
                .sha256
                .clone()
                .context("scan has no identity")?,
            song: song.clone(),
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
            "WSR source identity changed"
        );
        let bytes = wsr::encode(&self.bytes, &self.song, cancel)?;
        crate::audio_discovery::assets::publish_bytes(path, &bytes, cancel, progress)
    }
}
