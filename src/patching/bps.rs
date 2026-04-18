use anyhow::{bail, ensure};

const HEADER: &[u8; 4] = b"BPS1";
const FOOTER_SIZE: usize = 12;

pub(crate) fn apply_bps_patch(source: &[u8], patch: &[u8]) -> anyhow::Result<Vec<u8>> {
    ensure!(
        patch.len() >= HEADER.len() + FOOTER_SIZE,
        "BPS patch too short"
    );
    ensure!(&patch[..4] == HEADER, "BPS patch missing BPS1 header");

    super::verify_patch_crcs(patch, source, "BPS")?;

    let mut pos = 4;
    let source_size = super::decode_varint(patch, &mut pos, "BPS")? as usize;
    let target_size = super::decode_varint(patch, &mut pos, "BPS")? as usize;
    let metadata_size = super::decode_varint(patch, &mut pos, "BPS")? as usize;

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
        let cmd = super::decode_varint(patch, &mut pos, "BPS")? as usize;
        let action = cmd & 3;
        let length = (cmd >> 2) + 1;

        match action {
            0 => {
                for _ in 0..length {
                    ensure!(
                        output_offset < target_size,
                        "BPS target overflow (SourceRead)"
                    );
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
                    ensure!(
                        output_offset < target_size,
                        "BPS target overflow (TargetRead)"
                    );
                    target[output_offset] = patch[pos];
                    output_offset += 1;
                    pos += 1;
                }
            }
            2 => {
                let raw = super::decode_varint(patch, &mut pos, "BPS")? as usize;
                let sign_negative = raw & 1 != 0;
                let delta = raw >> 1;
                if sign_negative {
                    source_relative = source_relative.wrapping_sub(delta);
                } else {
                    source_relative = source_relative.wrapping_add(delta);
                }
                for _ in 0..length {
                    ensure!(
                        output_offset < target_size,
                        "BPS target overflow (SourceCopy)"
                    );
                    ensure!(
                        source_relative < source.len(),
                        "BPS source read out of bounds"
                    );
                    target[output_offset] = source[source_relative];
                    output_offset += 1;
                    source_relative += 1;
                }
            }
            3 => {
                let raw = super::decode_varint(patch, &mut pos, "BPS")? as usize;
                let sign_negative = raw & 1 != 0;
                let delta = raw >> 1;
                if sign_negative {
                    target_relative = target_relative.wrapping_sub(delta);
                } else {
                    target_relative = target_relative.wrapping_add(delta);
                }
                for _ in 0..length {
                    ensure!(
                        output_offset < target_size,
                        "BPS target overflow (TargetCopy)"
                    );
                    ensure!(
                        target_relative < target_size,
                        "BPS target copy out of bounds"
                    );
                    target[output_offset] = target[target_relative];
                    output_offset += 1;
                    target_relative += 1;
                }
            }
            _ => bail!("BPS invalid action {action}"),
        }
    }

    super::verify_target_crc(patch, &target, "BPS")?;

    Ok(target)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn make_simple_bps(source: &[u8], target: &[u8]) -> Vec<u8> {
        let mut patch = Vec::new();
        patch.extend_from_slice(HEADER);
        patch.extend(super::super::encode_varint(source.len() as u64));
        patch.extend(super::super::encode_varint(target.len() as u64));
        patch.extend(super::super::encode_varint(0));

        let cmd = ((target.len() as u64 - 1) << 2) | 1;
        patch.extend(super::super::encode_varint(cmd));
        patch.extend_from_slice(target);

        super::super::append_patch_crcs(&mut patch, source, target);
        patch
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
        let source = vec![0xAA, 0xBB, 0xCC, 0xDD];
        let target_expected = source.clone();

        let mut patch = Vec::new();
        patch.extend_from_slice(HEADER);
        patch.extend(super::super::encode_varint(source.len() as u64));
        patch.extend(super::super::encode_varint(target_expected.len() as u64));
        patch.extend(super::super::encode_varint(0));

        let cmd = (source.len() as u64 - 1) << 2;
        patch.extend(super::super::encode_varint(cmd));

        super::super::append_patch_crcs(&mut patch, &source, &target_expected);

        let result = apply_bps_patch(&source, &patch).unwrap();
        assert_eq!(result, target_expected);
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

        let off = patch.len() - 12;
        patch[off] ^= 0xFF;
        let patch_crc = crc32fast::hash(&patch[..patch.len() - 4]);
        let len = patch.len();
        patch[len - 4..].copy_from_slice(&super::super::write_u32_le(patch_crc));

        let wrong_source = vec![0x11u8; 4];
        let result = apply_bps_patch(&wrong_source, &patch);
        assert!(result.is_err());
    }
}
