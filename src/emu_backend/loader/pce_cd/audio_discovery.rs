use std::path::Path;
use std::sync::Arc;

use crate::audio_discovery::cdda::{CdAudioInput, CdAudioProvenance};
use crate::audio_discovery::media::ScanInput;
use crate::emu_backend::pce_cd::LoadedPceCd;

pub(super) fn snapshot(loaded: &LoadedPceCd, source_path: &Path) -> anyhow::Result<Arc<ScanInput>> {
    let effective_hash = loaded.disc.content_hash();
    let modified = loaded.source_disc_sha256 != effective_hash;
    let audio = CdAudioInput::new(
        Arc::new(loaded.disc.clone()),
        const_hex::encode(loaded.source_disc_sha256),
        if modified {
            None
        } else {
            loaded.disc.payload_len()
        },
        const_hex::encode(effective_hash),
        CdAudioProvenance {
            // The prepared load authenticates the normalized disc, independently of TAS admission.
            source_kind: "loaded_disc",
            source_media_sha256: const_hex::encode(loaded.raw_source_media_sha256),
            source_media_len: loaded.raw_source_media_len,
            selected_member_path_sha256: None,
            transforms_applied: modified,
        },
    )?;
    let mut input = ScanInput::from_disc(audio, "loaded-effective-cd-v1");
    input.display_name = source_path
        .file_stem()
        .and_then(|name| name.to_str())
        .map(str::to_owned);
    Ok(Arc::new(input))
}

#[cfg(test)]
mod tests;
