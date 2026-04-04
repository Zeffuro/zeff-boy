use anyhow::{bail, ensure};

const HEADER: &[u8; 4] = b"UPS1";
const FOOTER_SIZE: usize = 12;

pub(crate) fn apply_ups_patch(source: &[u8], patch: &[u8]) -> anyhow::Result<Vec<u8>> {
    ensure!(
        patch.len() >= HEADER.len() + FOOTER_SIZE,
        "UPS patch too short"
    );
    ensure!(&patch[..4] == HEADER, "UPS patch missing UPS1 header");

    let patch_body = &patch[..patch.len() - 4];
    let expected_patch_crc = read_u32_le(patch, patch.len() - 4);
    let actual_patch_crc = crc32fast::hash(patch_body);
    ensure!(
        actual_patch_crc == expected_patch_crc,
        "UPS patch CRC mismatch: expected {expected_patch_crc:08x}, got {actual_patch_crc:08x}"
    );

    let expected_source_crc = read_u32_le(patch, patch.len() - 12);
    let actual_source_crc = crc32fast::hash(source);
    ensure!(
        actual_source_crc == expected_source_crc,
        "UPS source CRC mismatch: expected {expected_source_crc:08x}, got {actual_source_crc:08x}"
    );

    let mut pos = 4;
    let source_size = decode_varint(patch, &mut pos)? as usize;
    let target_size = decode_varint(patch, &mut pos)? as usize;

    ensure!(
        source_size == source.len(),
        "UPS source size {source_size} doesn't match ROM size {}",
        source.len()
    );

    let mut output = source.to_vec();
    output.resize(target_size, 0);

    let data_end = patch.len() - FOOTER_SIZE;
    let mut write_offset: usize = 0;

    while pos < data_end {
        let skip = decode_varint(patch, &mut pos)? as usize;
        write_offset += skip;

        while pos < data_end && patch[pos] != 0x00 {
            ensure!(write_offset < target_size, "UPS target overflow");
            let xor_byte = patch[pos];
            pos += 1;
            let source_byte = if write_offset < source.len() {
                source[write_offset]
            } else {
                0
            };
            output[write_offset] = source_byte ^ xor_byte;
            write_offset += 1;
        }

        if pos >= data_end {
            bail!("UPS patch truncated: missing record terminator at offset {pos}");
        }
        pos += 1;
        write_offset += 1;
    }

    let expected_target_crc = read_u32_le(patch, patch.len() - 8);
    let actual_target_crc = crc32fast::hash(&output);
    ensure!(
        actual_target_crc == expected_target_crc,
        "UPS target CRC mismatch: expected {expected_target_crc:08x}, got {actual_target_crc:08x}"
    );

    Ok(output)
}

fn decode_varint(data: &[u8], pos: &mut usize) -> anyhow::Result<u64> {
    let mut result = 0u64;
    let mut shift = 1u64;
    loop {
        ensure!(*pos < data.len(), "UPS varint truncated");
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
mod tests {
    use super::*;

    fn make_ups(source: &[u8], target: &[u8]) -> Vec<u8> {
        let mut patch = Vec::new();
        patch.extend_from_slice(HEADER);
        patch.extend(encode_varint(source.len() as u64));
        patch.extend(encode_varint(target.len() as u64));

        let max_len = source.len().max(target.len());
        let mut write_offset: usize = 0;
        let mut i = 0;
        while i < max_len {
            let s = if i < source.len() { source[i] } else { 0 };
            let t = if i < target.len() { target[i] } else { 0 };
            if s != t {
                let skip = i - write_offset;
                patch.extend(encode_varint(skip as u64));
                while i < max_len {
                    let s = if i < source.len() { source[i] } else { 0 };
                    let t = if i < target.len() { target[i] } else { 0 };
                    let xor = s ^ t;
                    if xor == 0 {
                        break;
                    }
                    patch.push(xor);
                    i += 1;
                }
                patch.push(0x00);
                write_offset = i + 1;
            }
            i += 1;
        }

        let source_crc = crc32fast::hash(source);
        let target_crc = crc32fast::hash(target);
        patch.extend_from_slice(&source_crc.to_le_bytes());
        patch.extend_from_slice(&target_crc.to_le_bytes());
        let patch_crc = crc32fast::hash(&patch);
        patch.extend_from_slice(&patch_crc.to_le_bytes());
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
        let source = vec![0x00u8; 8];
        let target = vec![0xAA, 0xBB, 0xCC, 0xDD, 0x00, 0x00, 0x00, 0x00];
        let patch = make_ups(&source, &target);
        let result = apply_ups_patch(&source, &patch).unwrap();
        assert_eq!(result, target);
    }

    #[test]
    fn apply_identity_patch() {
        let source = vec![0x11, 0x22, 0x33, 0x44];
        let target = source.clone();
        let patch = make_ups(&source, &target);
        let result = apply_ups_patch(&source, &patch).unwrap();
        assert_eq!(result, target);
    }

    #[test]
    fn apply_full_replacement() {
        let source = vec![0x01, 0x02, 0x03, 0x04];
        let target = vec![0xF1, 0xF2, 0xF3, 0xF4];
        let patch = make_ups(&source, &target);
        let result = apply_ups_patch(&source, &patch).unwrap();
        assert_eq!(result, target);
    }

    #[test]
    fn apply_changes_size() {
        let source = vec![0x00u8; 4];
        let target = vec![0x11, 0x22, 0x33, 0x44, 0x55, 0x66];
        let patch = make_ups(&source, &target);
        let result = apply_ups_patch(&source, &patch).unwrap();
        assert_eq!(result, target);
    }

    #[test]
    fn apply_sparse_changes() {
        let source = vec![0x00u8; 16];
        let mut target = vec![0x00u8; 16];
        target[2] = 0xAA;
        target[10] = 0xBB;
        let patch = make_ups(&source, &target);
        let result = apply_ups_patch(&source, &patch).unwrap();
        assert_eq!(result, target);
    }

    #[test]
    fn apply_rejects_bad_header() {
        let source = vec![0u8; 4];
        let result = apply_ups_patch(&source, b"XXXX plus enough padding bytes here!");
        assert!(result.is_err());
    }

    #[test]
    fn apply_rejects_source_crc_mismatch() {
        let source = vec![0u8; 4];
        let target = vec![0xFFu8; 4];
        let patch = make_ups(&source, &target);
        let wrong_source = vec![0x11u8; 4];
        let result = apply_ups_patch(&wrong_source, &patch);
        assert!(result.is_err());
    }

    #[test]
    fn apply_rejects_short_patch() {
        let source = vec![0u8; 4];
        let result = apply_ups_patch(&source, b"UPS1");
        assert!(result.is_err());
    }
}
