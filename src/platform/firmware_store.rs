const FIRMWARE_KEY_PREFIX: &str = "zeff-firmware-v1:";
const RECORD_MAGIC: &[u8; 4] = b"ZFW1";

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct FirmwareRecord {
    pub(crate) original_filename: String,
    pub(crate) bytes: Vec<u8>,
}

pub(crate) fn storage_key(bytes: &[u8]) -> String {
    format!("{FIRMWARE_KEY_PREFIX}{}", zeff_firmware::sha256_hex(bytes))
}

pub(crate) fn is_firmware_key(key: &str) -> bool {
    key.starts_with(FIRMWARE_KEY_PREFIX)
}

pub(crate) fn is_valid_firmware_key(key: &str) -> bool {
    let Some(digest) = key.strip_prefix(FIRMWARE_KEY_PREFIX) else {
        return false;
    };
    digest.len() == 64
        && digest
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

pub(crate) fn firmware_key_matches_bytes(key: &str, bytes: &[u8]) -> bool {
    key == storage_key(bytes)
}

pub(crate) fn encode_record(record: &FirmwareRecord) -> anyhow::Result<Vec<u8>> {
    let filename = record.original_filename.as_bytes();
    let filename_len = u32::try_from(filename.len())?;
    let mut encoded =
        Vec::with_capacity(RECORD_MAGIC.len() + 4 + filename.len() + record.bytes.len());
    encoded.extend_from_slice(RECORD_MAGIC);
    encoded.extend_from_slice(&filename_len.to_le_bytes());
    encoded.extend_from_slice(filename);
    encoded.extend_from_slice(&record.bytes);
    Ok(encoded)
}

pub(crate) fn decode_record(encoded: &[u8]) -> anyhow::Result<FirmwareRecord> {
    if encoded.len() < 8 || &encoded[..4] != RECORD_MAGIC {
        anyhow::bail!("invalid firmware record header");
    }
    let filename_len = u32::from_le_bytes(encoded[4..8].try_into()?) as usize;
    let payload_start = 8usize
        .checked_add(filename_len)
        .filter(|end| *end <= encoded.len())
        .ok_or_else(|| anyhow::anyhow!("invalid firmware filename length"))?;
    let original_filename = std::str::from_utf8(&encoded[8..payload_start])?.to_owned();
    if original_filename.is_empty() {
        anyhow::bail!("firmware filename is empty");
    }
    Ok(FirmwareRecord {
        original_filename,
        bytes: encoded[payload_start..].to_vec(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn firmware_key_is_content_addressed() {
        assert_eq!(storage_key(b"same"), storage_key(b"same"));
        assert_ne!(storage_key(b"same"), storage_key(b"different"));
        assert!(is_firmware_key(&storage_key(b"same")));
        assert!(is_valid_firmware_key(&storage_key(b"same")));
        assert!(!is_valid_firmware_key(FIRMWARE_KEY_PREFIX));
        assert!(!is_valid_firmware_key(&format!(
            "{FIRMWARE_KEY_PREFIX}{}",
            "G".repeat(64)
        )));
        assert!(!is_valid_firmware_key(&format!(
            "{FIRMWARE_KEY_PREFIX}{}",
            "a".repeat(63)
        )));
        assert!(firmware_key_matches_bytes(&storage_key(b"same"), b"same"));
        assert!(!firmware_key_matches_bytes(
            &storage_key(b"different"),
            b"same"
        ));
    }

    #[test]
    fn firmware_record_roundtrips_filename_and_bytes() {
        let record = FirmwareRecord {
            original_filename: "cgb_boot.bin".to_owned(),
            bytes: vec![0, 1, 2, 255],
        };
        assert_eq!(
            decode_record(&encode_record(&record).unwrap()).unwrap(),
            record
        );
    }

    #[test]
    fn malformed_firmware_records_are_rejected() {
        assert!(decode_record(b"bad").is_err());
        let mut encoded = b"ZFW1".to_vec();
        encoded.extend_from_slice(&10u32.to_le_bytes());
        encoded.push(b'x');
        assert!(decode_record(&encoded).is_err());
    }
}
