mod bps;
mod ips;
pub(crate) mod ups;

pub(crate) use bps::apply_bps_patch;
pub(crate) use ips::apply_ips_patch;
pub(crate) use ups::apply_ups_patch;

fn decode_varint(data: &[u8], pos: &mut usize, label: &str) -> anyhow::Result<u64> {
    let mut result = 0u64;
    let mut shift = 1u64;
    loop {
        anyhow::ensure!(*pos < data.len(), "{label} varint truncated");
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

fn verify_patch_crcs(patch: &[u8], source: &[u8], label: &str) -> anyhow::Result<()> {
    let patch_body = &patch[..patch.len() - 4];
    let expected_patch_crc = read_u32_le(patch, patch.len() - 4);
    let actual_patch_crc = crc32fast::hash(patch_body);
    anyhow::ensure!(
        actual_patch_crc == expected_patch_crc,
        "{label} patch CRC mismatch: expected {expected_patch_crc:08x}, got {actual_patch_crc:08x}"
    );

    let expected_source_crc = read_u32_le(patch, patch.len() - 12);
    let actual_source_crc = crc32fast::hash(source);
    anyhow::ensure!(
        actual_source_crc == expected_source_crc,
        "{label} source CRC mismatch: expected {expected_source_crc:08x}, got {actual_source_crc:08x}"
    );

    Ok(())
}

fn verify_target_crc(patch: &[u8], target: &[u8], label: &str) -> anyhow::Result<()> {
    let expected = read_u32_le(patch, patch.len() - 8);
    let actual = crc32fast::hash(target);
    anyhow::ensure!(
        actual == expected,
        "{label} target CRC mismatch: expected {expected:08x}, got {actual:08x}"
    );
    Ok(())
}

#[cfg(test)]
pub(crate) fn write_u32_le(val: u32) -> [u8; 4] {
    val.to_le_bytes()
}

#[cfg(test)]
pub(crate) fn append_patch_crcs(patch: &mut Vec<u8>, source: &[u8], target: &[u8]) {
    let source_crc = crc32fast::hash(source);
    let target_crc = crc32fast::hash(target);
    patch.extend_from_slice(&source_crc.to_le_bytes());
    patch.extend_from_slice(&target_crc.to_le_bytes());
    let patch_crc = crc32fast::hash(patch);
    patch.extend_from_slice(&patch_crc.to_le_bytes());
}

#[cfg(test)]
pub(crate) fn encode_varint(mut value: u64) -> Vec<u8> {
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

    #[test]
    fn varint_roundtrip() {
        for value in [0, 1, 127, 128, 255, 1000, 65535, 0x1_0000] {
            let encoded = encode_varint(value);
            let mut pos = 0;
            let decoded = decode_varint(&encoded, &mut pos, "test").unwrap();
            assert_eq!(decoded, value, "varint roundtrip failed for {value}");
            assert_eq!(pos, encoded.len());
        }
    }
}
