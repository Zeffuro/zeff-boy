use anyhow::{bail, ensure};

const HEADER: &[u8; 4] = b"BPS1";
const FOOTER_SIZE: usize = 12;

pub(crate) fn apply_bps_patch(source: &[u8], patch: &[u8]) -> anyhow::Result<Vec<u8>> {
    ensure!(patch.len() >= HEADER.len() + FOOTER_SIZE, "BPS patch too short");
    ensure!(&patch[..4] == HEADER, "BPS patch missing BPS1 header");

    let patch_body = &patch[..patch.len() - 4];
    let expected_patch_crc = read_u32_le(patch, patch.len() - 4);
    let actual_patch_crc = crc32fast::hash(patch_body);
    ensure!(
        actual_patch_crc == expected_patch_crc,
        "BPS patch CRC mismatch: expected {expected_patch_crc:08x}, got {actual_patch_crc:08x}"
    );

    let expected_source_crc = read_u32_le(patch, patch.len() - 12);
    let actual_source_crc = crc32fast::hash(source);
    ensure!(
        actual_source_crc == expected_source_crc,
        "BPS source CRC mismatch: expected {expected_source_crc:08x}, got {actual_source_crc:08x}"
    );

    let mut pos = 4;
    let source_size = decode_varint(patch, &mut pos)? as usize;
    let target_size = decode_varint(patch, &mut pos)? as usize;
    let metadata_size = decode_varint(patch, &mut pos)? as usize;

    ensure!(
        source_size == source.len(),
        "BPS source size {source_size} doesn't match ROM size {}",
        source.len()
    );

    pos += metadata_size;

    let mut target = vec![0u8; target_size];
    let mut output_offset: usize = 0;
    let mut source_relative: usize = 0;
    let mut target_relative: usize = 0;

    let data_end = patch.len() - FOOTER_SIZE;

    while pos < data_end {
        let cmd = decode_varint(patch, &mut pos)? as usize;
        let action = cmd & 3;
        let length = (cmd >> 2) + 1;

        match action {
            0 => {
                for _ in 0..length {
                    ensure!(output_offset < target_size, "BPS target overflow (SourceRead)");
                    target[output_offset] = if output_offset < source.len() {
                        source[output_offset]
                    } else {
                        0
                    };
                    output_offset += 1;
                }
            }
            1 => {
                for _ in 0..length {
                    ensure!(pos < data_end, "BPS patch truncated (TargetRead)");
                    ensure!(output_offset < target_size, "BPS target overflow (TargetRead)");
                    target[output_offset] = patch[pos];
                    output_offset += 1;
                    pos += 1;
                }
            }
            2 => {
                let raw = decode_varint(patch, &mut pos)? as usize;
                let sign_negative = raw & 1 != 0;
                let delta = raw >> 1;
                if sign_negative {
                    source_relative = source_relative.wrapping_sub(delta);
                } else {
                    source_relative = source_relative.wrapping_add(delta);
                }
                for _ in 0..length {
                    ensure!(output_offset < target_size, "BPS target overflow (SourceCopy)");
                    ensure!(source_relative < source.len(), "BPS source read out of bounds");
                    target[output_offset] = source[source_relative];
                    output_offset += 1;
                    source_relative += 1;
                }
            }
            3 => {
                let raw = decode_varint(patch, &mut pos)? as usize;
                let sign_negative = raw & 1 != 0;
                let delta = raw >> 1;
                if sign_negative {
                    target_relative = target_relative.wrapping_sub(delta);
                } else {
                    target_relative = target_relative.wrapping_add(delta);
                }
                for _ in 0..length {
                    ensure!(output_offset < target_size, "BPS target overflow (TargetCopy)");
                    ensure!(target_relative < target_size, "BPS target copy out of bounds");
                    target[output_offset] = target[target_relative];
                    output_offset += 1;
                    target_relative += 1;
                }
            }
            _ => bail!("BPS invalid action {action}"),
        }
    }

    let expected_target_crc = read_u32_le(patch, patch.len() - 8);
    let actual_target_crc = crc32fast::hash(&target);
    ensure!(
        actual_target_crc == expected_target_crc,
        "BPS target CRC mismatch: expected {expected_target_crc:08x}, got {actual_target_crc:08x}"
    );

    Ok(target)
}

pub(crate) fn validate_bps(patch: &[u8]) -> bool {
    patch.len() >= HEADER.len() + FOOTER_SIZE && &patch[..4] == HEADER
}

fn decode_varint(data: &[u8], pos: &mut usize) -> anyhow::Result<u64> {
    let mut result = 0u64;
    let mut shift = 1u64;
    loop {
        ensure!(*pos < data.len(), "BPS varint truncated");
        let byte = data[*pos] as u64;
        *pos += 1;
        result += (byte & 0x7f) * shift;
        if byte & 0x80 != 0 {
            break;
        }
        shift <<= 7;
        result += shift;
    }
    Ok(result)
}

fn read_u32_le(data: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes([
        data[offset],
        data[offset + 1],
        data[offset + 2],
        data[offset + 3],
    ])
}

/// Encode a value as a BPS variable-length integer (for tests).
#[cfg(test)]
fn encode_varint(mut value: u64) -> Vec<u8> {
    let mut buf = Vec::new();
    loop {
        let mut byte = (value & 0x7f) as u8;
        value >>= 7;
        if value == 0 {
            byte |= 0x80;
            buf.push(byte);
            break;
        }
        buf.push(byte);
        value -= 1;
    }
    buf
}

#[cfg(test)]
fn write_u32_le(val: u32) -> [u8; 4] {
    val.to_le_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a minimal valid BPS patch from source to target using TargetRead actions.
    fn make_simple_bps(source: &[u8], target: &[u8]) -> Vec<u8> {
        let mut patch = Vec::new();
        patch.extend_from_slice(HEADER);
        patch.extend(encode_varint(source.len() as u64));
        patch.extend(encode_varint(target.len() as u64));
        patch.extend(encode_varint(0)); // no metadata

        // Emit all target bytes via TargetRead actions.
        // action = (length - 1) << 2 | 1
        let cmd = ((target.len() as u64 - 1) << 2) | 1;
        patch.extend(encode_varint(cmd));
        patch.extend_from_slice(target);

        // Footer: source CRC, target CRC, patch CRC (over everything before it).
        let source_crc = crc32fast::hash(source);
        let target_crc = crc32fast::hash(target);
        patch.extend_from_slice(&write_u32_le(source_crc));
        patch.extend_from_slice(&write_u32_le(target_crc));
        let patch_crc = crc32fast::hash(&patch);
        patch.extend_from_slice(&write_u32_le(patch_crc));
        patch
    }

    #[test]
    fn varint_roundtrip() {
        for value in [0, 1, 127, 128, 255, 1000, 65535, 0x1_0000] {
            let encoded = encode_varint(value);
            let mut pos = 0;
            let decoded = decode_varint(&encoded, &mut pos).unwrap();
            assert_eq!(decoded, value, "varint roundtrip failed for {value}");
            assert_eq!(pos, encoded.len());
        }
    }

    #[test]
    fn apply_simple_patch() {
        let source = vec![0x00; 8];
        let target_expected = vec![0xAA, 0xBB, 0xCC, 0xDD, 0x00, 0x00, 0x00, 0x00];
        let patch = make_simple_bps(&source, &target_expected);

        let result = apply_bps_patch(&source, &patch).unwrap();
        assert_eq!(result, target_expected);
    }

    #[test]
    fn apply_changes_size() {
        let source = vec![0x00; 4];
        let target_expected = vec![0x11, 0x22, 0x33, 0x44, 0x55, 0x66];
        let patch = make_simple_bps(&source, &target_expected);

        let result = apply_bps_patch(&source, &patch).unwrap();
        assert_eq!(result, target_expected);
    }

    #[test]
    fn apply_source_read() {
        // Build a patch that uses SourceRead to copy source bytes.
        let source = vec![0xAA, 0xBB, 0xCC, 0xDD];
        let target_expected = source.clone(); // identical copy via SourceRead

        let mut patch = Vec::new();
        patch.extend_from_slice(HEADER);
        patch.extend(encode_varint(source.len() as u64));
        patch.extend(encode_varint(target_expected.len() as u64));
        patch.extend(encode_varint(0)); // no metadata

        // SourceRead action: action=0, length=4 → cmd = (4-1)<<2 | 0 = 12
        let cmd = ((source.len() as u64 - 1) << 2) | 0;
        patch.extend(encode_varint(cmd));

        let source_crc = crc32fast::hash(&source);
        let target_crc = crc32fast::hash(&target_expected);
        patch.extend_from_slice(&write_u32_le(source_crc));
        patch.extend_from_slice(&write_u32_le(target_crc));
        let patch_crc = crc32fast::hash(&patch);
        patch.extend_from_slice(&write_u32_le(patch_crc));

        let result = apply_bps_patch(&source, &patch).unwrap();
        assert_eq!(result, target_expected);
    }

    #[test]
    fn validate_good_patch() {
        let source = vec![0u8; 4];
        let target = vec![0xFFu8; 4];
        let patch = make_simple_bps(&source, &target);
        assert!(validate_bps(&patch));
    }

    #[test]
    fn validate_bad_header() {
        assert!(!validate_bps(b"XXXX stuff here plus padding"));
    }

    #[test]
    fn validate_too_short() {
        assert!(!validate_bps(b"BPS1"));
    }

    #[test]
    fn apply_rejects_bad_header() {
        let source = vec![0u8; 4];
        let result = apply_bps_patch(&source, b"XXXX plus enough padding bytes here!");
        assert!(result.is_err());
    }

    #[test]
    fn apply_rejects_source_crc_mismatch() {
        let source = vec![0u8; 4];
        let target = vec![0xFFu8; 4];
        let mut patch = make_simple_bps(&source, &target);
        // Corrupt the source CRC (at patch.len() - 12).
        let off = patch.len() - 12;
        patch[off] ^= 0xFF;
        // Recompute patch CRC.
        let patch_crc = crc32fast::hash(&patch[..patch.len() - 4]);
        let len = patch.len();
        patch[len - 4..].copy_from_slice(&write_u32_le(patch_crc));

        let wrong_source = vec![0x11u8; 4]; // different source
        let result = apply_bps_patch(&wrong_source, &patch);
        assert!(result.is_err());
    }
}

