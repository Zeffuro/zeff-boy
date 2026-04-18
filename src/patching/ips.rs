use anyhow::{bail, ensure};

const HEADER: &[u8; 5] = b"PATCH";
const FOOTER: &[u8; 3] = b"EOF";

pub(crate) fn apply_ips_patch(rom: &mut Vec<u8>, patch: &[u8]) -> anyhow::Result<()> {
    ensure!(patch.len() >= 8, "IPS patch too short");
    ensure!(&patch[..5] == HEADER, "IPS patch missing PATCH header");

    let mut pos = 5;
    loop {
        if pos + 3 > patch.len() {
            bail!("IPS patch truncated: unexpected end at offset {pos}");
        }
        if &patch[pos..pos + 3] == FOOTER {
            break;
        }

        let offset = read_u24_be(patch, pos);
        pos += 3;

        if pos + 2 > patch.len() {
            bail!("IPS patch truncated: missing size at offset {pos}");
        }
        let size = read_u16_be(patch, pos) as usize;
        pos += 2;

        if size == 0 {
            if pos + 3 > patch.len() {
                bail!("IPS RLE record truncated at offset {pos}");
            }
            let rle_count = read_u16_be(patch, pos) as usize;
            pos += 2;
            let rle_value = patch[pos];
            pos += 1;

            let end = offset + rle_count;
            if end > rom.len() {
                rom.resize(end, 0);
            }
            rom[offset..end].fill(rle_value);
        } else {
            if pos + size > patch.len() {
                bail!("IPS record data truncated at offset {pos}");
            }
            let end = offset + size;
            if end > rom.len() {
                rom.resize(end, 0);
            }
            rom[offset..end].copy_from_slice(&patch[pos..pos + size]);
            pos += size;
        }
    }

    Ok(())
}

fn read_u24_be(data: &[u8], offset: usize) -> usize {
    ((data[offset] as usize) << 16)
        | ((data[offset + 1] as usize) << 8)
        | (data[offset + 2] as usize)
}

fn read_u16_be(data: &[u8], offset: usize) -> u16 {
    ((data[offset] as u16) << 8) | (data[offset + 1] as u16)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_patch(records: &[u8]) -> Vec<u8> {
        let mut patch = Vec::new();
        patch.extend_from_slice(HEADER);
        patch.extend_from_slice(records);
        patch.extend_from_slice(FOOTER);
        patch
    }

    #[test]
    fn apply_simple_record() {
        let patch = make_patch(&[0x00, 0x00, 0x02, 0x00, 0x03, 0xAA, 0xBB, 0xCC]);
        let mut rom = vec![0u8; 16];
        apply_ips_patch(&mut rom, &patch).unwrap();
        assert_eq!(rom[2], 0xAA);
        assert_eq!(rom[3], 0xBB);
        assert_eq!(rom[4], 0xCC);
    }

    #[test]
    fn apply_rle_record() {
        let patch = make_patch(&[0x00, 0x00, 0x04, 0x00, 0x00, 0x00, 0x05, 0xFF]);
        let mut rom = vec![0u8; 16];
        apply_ips_patch(&mut rom, &patch).unwrap();
        assert_eq!(&rom[4..9], &[0xFF; 5]);
        assert_eq!(rom[3], 0x00);
        assert_eq!(rom[9], 0x00);
    }

    #[test]
    fn apply_extends_rom() {
        let patch = make_patch(&[0x00, 0x00, 0x08, 0x00, 0x02, 0x11, 0x22]);
        let mut rom = vec![0u8; 4];
        apply_ips_patch(&mut rom, &patch).unwrap();
        assert_eq!(rom.len(), 10);
        assert_eq!(rom[8], 0x11);
        assert_eq!(rom[9], 0x22);
    }

    #[test]
    fn apply_rejects_bad_header() {
        let mut rom = vec![0u8; 16];
        let result = apply_ips_patch(&mut rom, b"XXXXX stuff EOF");
        assert!(result.is_err());
    }

    #[test]
    fn apply_multiple_records() {
        let patch = make_patch(&[
            0x00, 0x00, 0x00, 0x00, 0x02, 0xAA, 0xBB, 0x00, 0x00, 0x04, 0x00, 0x01, 0xCC,
        ]);
        let mut rom = vec![0u8; 16];
        apply_ips_patch(&mut rom, &patch).unwrap();
        assert_eq!(rom[0], 0xAA);
        assert_eq!(rom[1], 0xBB);
        assert_eq!(rom[4], 0xCC);
    }
}
