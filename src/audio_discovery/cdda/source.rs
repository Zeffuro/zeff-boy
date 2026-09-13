use std::sync::Arc;

use anyhow::{Context, Result, ensure};

use super::{CdAudioInput, CdAudioTrack};
use crate::audio_discovery::media::{ScanInput, ScanManifest};

pub(crate) fn input_for_song(
    input: &ScanInput,
    manifest: &ScanManifest,
    track: CdAudioTrack,
) -> Result<Arc<CdAudioInput>> {
    let disc = input
        .cdda
        .as_ref()
        .context("loaded media has no CD audio source")?;
    let identity = manifest
        .disc
        .as_ref()
        .context("scan has no disc identity")?;
    ensure!(
        input.system == Some(zeff_emu_common::system::System::Pce)
            && input.standalone_audio.is_none()
            && input.bytes.is_empty()
            && manifest.analysis_profile == input.analysis_profile
            && manifest.scan.media.system == input.media_system_id()
            && manifest.scan.media.byte_len == disc.effective_disc_len as u64
            && manifest.scan.media.sha256.as_ref() == Some(&disc.effective_disc_sha256)
            && identity.original_disc_sha256 == disc.original_disc_sha256
            && identity.original_disc_len == disc.original_disc_len
            && identity.effective_disc_sha256 == disc.effective_disc_sha256
            && identity.effective_disc_len == disc.effective_disc_len
            && identity.provenance == disc.provenance
            && disc.track(track.number)? == track,
        "CD audio scan does not match the loaded disc"
    );
    Ok(Arc::clone(disc))
}
