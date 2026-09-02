use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use sha2::{Digest, Sha256};

use super::{LoadedPceCd, PceCdLoadError};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) struct FileIdentity {
    pub(super) bytes: u64,
    modified: Option<std::time::SystemTime>,
}

impl FileIdentity {
    pub(super) fn from_metadata(metadata: &std::fs::Metadata) -> Self {
        Self {
            bytes: metadata.len(),
            modified: metadata.modified().ok(),
        }
    }
}

pub(super) fn hash_file(
    file: &mut File,
    bytes: u64,
    path: &Path,
) -> Result<[u8; 32], PceCdLoadError> {
    file.seek(SeekFrom::Start(0))
        .map_err(|_| PceCdLoadError::ChdUnreadable(path.into()))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0; 64 * 1024];
    let mut remaining = bytes;
    while remaining != 0 {
        let count = usize::try_from(remaining.min(buffer.len() as u64))
            .map_err(|_| PceCdLoadError::DataTooLarge(remaining))?;
        file.read_exact(&mut buffer[..count])
            .map_err(|_| PceCdLoadError::ChdUnreadable(path.into()))?;
        hasher.update(&buffer[..count]);
        remaining -= count as u64;
    }
    Ok(hasher.finalize().into())
}

pub(super) fn apply_raw_source_media(
    loaded: &mut LoadedPceCd,
    sha256: [u8; 32],
    bytes: u64,
) -> Result<(), PceCdLoadError> {
    loaded.raw_source_media_sha256 = sha256;
    loaded.raw_source_media_len =
        usize::try_from(bytes).map_err(|_| PceCdLoadError::DataTooLarge(bytes))?;
    Ok(())
}
