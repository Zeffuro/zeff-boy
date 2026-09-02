use std::fs::File;
use std::io::Read;
use std::path::Path;

use sha2::{Digest, Sha256};
use zeff_pce_core::hardware::{CdDisc, CdTrackMode};

use super::{HASH_BUFFER_BYTES, PCE_CD_DATA_BYTES_LIMIT, PceCdLoadError};

pub(crate) fn direct_file_sha256(path: &Path) -> Result<([u8; 32], usize), PceCdLoadError> {
    let metadata =
        std::fs::metadata(path).map_err(|_| PceCdLoadError::BinUnreadable(path.to_path_buf()))?;
    if metadata.len() > PCE_CD_DATA_BYTES_LIMIT as u64 {
        return Err(PceCdLoadError::DataTooLarge(metadata.len()));
    }
    let bytes = usize::try_from(metadata.len())
        .map_err(|_| PceCdLoadError::DataTooLarge(metadata.len()))?;
    let mut file = File::open(path).map_err(|_| PceCdLoadError::BinUnreadable(path.into()))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0; HASH_BUFFER_BYTES];
    let mut remaining = bytes;
    while remaining != 0 {
        let count = remaining.min(buffer.len());
        file.read_exact(&mut buffer[..count])
            .map_err(|_| PceCdLoadError::BinUnreadable(path.into()))?;
        hasher.update(&buffer[..count]);
        remaining -= count;
    }
    Ok((hasher.finalize().into(), bytes))
}

pub(super) fn disc_payload_len(disc: &CdDisc) -> Result<usize, PceCdLoadError> {
    disc.tracks().iter().try_fold(0_usize, |total, track| {
        let sector_bytes = match track.mode() {
            CdTrackMode::Mode1_2048 => zeff_pce_core::hardware::CD_USER_SECTOR_BYTES,
            CdTrackMode::Mode1_2352 | CdTrackMode::Audio => {
                zeff_pce_core::hardware::CD_RAW_SECTOR_BYTES
            }
        };
        let bytes = usize::try_from(track.sector_count())
            .map_err(|_| PceCdLoadError::DataTooLarge(u64::MAX))?
            .checked_mul(sector_bytes)
            .ok_or(PceCdLoadError::DataTooLarge(u64::MAX))?;
        total
            .checked_add(bytes)
            .ok_or(PceCdLoadError::DataTooLarge(u64::MAX))
    })
}
