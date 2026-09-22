use std::{
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU32, Ordering},
    },
};

use anyhow::{Context, Result, ensure};
use zeff_audio_discovery::{catalog::SongRef, huge::catalog::HugeSong};

use super::{
    extract::ExtractionRequest,
    media::{ScanInput, ScanManifest},
};

pub(crate) struct HugeGbsRequest {
    bytes: Arc<[u8]>,
    song: HugeSong,
}

impl HugeGbsRequest {
    pub(crate) fn prepare(
        input: &ScanInput,
        manifest: &ScanManifest,
        song: &HugeSong,
    ) -> Result<Self> {
        ExtractionRequest::prepare(
            input,
            manifest,
            SongRef::Huge(song)
                .span()
                .context("hUGE song has no source range")?,
            "hUGEDriver GBS",
        )?;
        ensure!(
            manifest.scan.media.sha256.as_deref() == Some(song.source_sha256.as_str()),
            "hUGE selection identity does not match the scan"
        );
        Ok(Self {
            bytes: Arc::clone(&input.bytes),
            song: song.clone(),
        })
    }

    pub(crate) fn write_new(
        self,
        path: &Path,
        cancel: &AtomicBool,
        progress: &AtomicU32,
    ) -> Result<()> {
        ensure!(!cancel.load(Ordering::Relaxed), "hUGE GBS export cancelled");
        ensure!(
            !path.exists(),
            "hUGE GBS output already exists: {}",
            path.display()
        );
        let (artifact, _proof) =
            super::huge_gbs_validation::validate(&self.bytes, &self.song, cancel)?;
        super::assets::publish_bytes(path, &artifact.bytes, cancel, progress)
    }
}
