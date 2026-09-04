use std::path::Path;

use super::{CHD_HEADER_BYTES, CHD_UNIT_BYTES, PceCdLoadError};

pub(crate) fn write_synthetic_uncompressed_v5_chd(path: &Path) -> Result<(), PceCdLoadError> {
    let hunk_bytes = 4 * CHD_UNIT_BYTES;
    let first =
        b"TRACK:1 TYPE:MODE1 SUBTYPE:NONE FRAMES:4 PREGAP:0 PGTYPE:VMODE1 PGSUB:NONE POSTGAP:0";
    let second =
        b"TRACK:2 TYPE:AUDIO SUBTYPE:NONE FRAMES:4 PREGAP:0 PGTYPE:VAUDIO PGSUB:NONE POSTGAP:0";
    let metadata_offset = 132_usize;
    let second_offset = metadata_offset + 16 + first.len();
    let mut bytes = vec![0; 3 * hunk_bytes];
    bytes[..8].copy_from_slice(b"MComprHD");
    bytes[8..12].copy_from_slice(&124_u32.to_be_bytes());
    bytes[12..16].copy_from_slice(&5_u32.to_be_bytes());
    bytes[32..40].copy_from_slice(&(2 * hunk_bytes as u64).to_be_bytes());
    bytes[40..48].copy_from_slice(&(CHD_HEADER_BYTES as u64).to_be_bytes());
    bytes[48..56].copy_from_slice(&(metadata_offset as u64).to_be_bytes());
    bytes[56..60].copy_from_slice(&(hunk_bytes as u32).to_be_bytes());
    bytes[60..64].copy_from_slice(&(CHD_UNIT_BYTES as u32).to_be_bytes());
    bytes[124..128].copy_from_slice(&1_u32.to_be_bytes());
    bytes[128..132].copy_from_slice(&2_u32.to_be_bytes());
    bytes[metadata_offset..metadata_offset + 4].copy_from_slice(b"CHT2");
    bytes[metadata_offset + 4..metadata_offset + 8]
        .copy_from_slice(&(first.len() as u32).to_be_bytes());
    bytes[metadata_offset + 8..metadata_offset + 16]
        .copy_from_slice(&(second_offset as u64).to_be_bytes());
    bytes[metadata_offset + 16..metadata_offset + 16 + first.len()].copy_from_slice(first);
    bytes[second_offset..second_offset + 4].copy_from_slice(b"CHT2");
    bytes[second_offset + 4..second_offset + 8]
        .copy_from_slice(&(second.len() as u32).to_be_bytes());
    bytes[second_offset + 16..second_offset + 16 + second.len()].copy_from_slice(second);
    bytes[hunk_bytes] = 0x31;
    bytes[2 * hunk_bytes..2 * hunk_bytes + 4].copy_from_slice(&[0x12, 0x34, 0x56, 0x78]);
    std::fs::write(path, bytes).map_err(|_| PceCdLoadError::ChdUnreadable(path.into()))
}
