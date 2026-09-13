use std::io::Read;
use std::path::Path;

use anyhow::{Context, Result, ensure};

use super::{ScanInput, SourceIdentity, StandaloneFormat};
use crate::audio_discovery::{MAX_ROM_BYTES, tracker::EmbeddedFormat};

pub(crate) fn tracker_format(path: &Path) -> Option<EmbeddedFormat> {
    match path.extension()?.to_str()?.to_ascii_lowercase().as_str() {
        "xm" => Some(EmbeddedFormat::Xm),
        "mod" => Some(EmbeddedFormat::Mod),
        "s3m" => Some(EmbeddedFormat::S3m),
        "it" => Some(EmbeddedFormat::It),
        _ => None,
    }
}

pub(crate) fn audio_format(path: &Path) -> Option<StandaloneFormat> {
    tracker_format(path)
        .map(StandaloneFormat::Tracker)
        .or_else(
            || match path.extension()?.to_str()?.to_ascii_lowercase().as_str() {
                "vgm" | "vgz" => Some(StandaloneFormat::Vgm),
                "gbs" => Some(StandaloneFormat::Rip(
                    crate::audio_discovery::rips::RipFormat::Gbs,
                )),
                "nsf" => Some(StandaloneFormat::Rip(
                    crate::audio_discovery::rips::RipFormat::Nsf,
                )),
                _ => None,
            },
        )
}

pub(crate) fn load_audio(path: &Path) -> Result<ScanInput> {
    let format = audio_format(path)
        .context("select an XM, MOD, S3M, IT, VGM, VGZ, GBS or NSF audio file")?;
    let metadata = std::fs::metadata(path)
        .with_context(|| format!("failed to inspect audio file {}", path.display()))?;
    ensure!(metadata.is_file(), "audio input must be a regular file");
    ensure!(
        (1..=MAX_ROM_BYTES as u64).contains(&metadata.len()),
        "audio file must be between 1 and {MAX_ROM_BYTES} bytes"
    );
    let expected_len = usize::try_from(metadata.len()).context("audio file is too large")?;
    let mut bytes = Vec::with_capacity(expected_len);
    std::fs::File::open(path)
        .with_context(|| format!("failed to open audio file {}", path.display()))?
        .take(MAX_ROM_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .with_context(|| format!("failed to read audio file {}", path.display()))?;
    ensure!(
        bytes.len() == expected_len,
        "audio file changed while it was read"
    );
    let source = SourceIdentity {
        kind: match format {
            StandaloneFormat::Tracker(EmbeddedFormat::Xm) => "direct_xm_file",
            StandaloneFormat::Tracker(EmbeddedFormat::Mod) => "direct_mod_file",
            StandaloneFormat::Tracker(EmbeddedFormat::S3m) => "direct_s3m_file",
            StandaloneFormat::Tracker(EmbeddedFormat::It) => "direct_it_file",
            StandaloneFormat::Vgm => "direct_vgm_file",
            StandaloneFormat::Rip(crate::audio_discovery::rips::RipFormat::Gbs) => {
                "direct_gbs_file"
            }
            StandaloneFormat::Rip(crate::audio_discovery::rips::RipFormat::Nsf) => {
                "direct_nsf_file"
            }
        },
        sha256: zeff_firmware::sha256_hex(&bytes),
        len: bytes.len(),
        container: None,
        selected_member: None,
    };
    Ok(ScanInput::standalone(
        bytes,
        source,
        format,
        path.file_stem()
            .and_then(|name| name.to_str())
            .filter(|name| !name.is_empty())
            .map(str::to_owned),
    ))
}
