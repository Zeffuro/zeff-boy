use md5::Md5;
use sha1::Sha1;
use sha2::{Digest, Sha256};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DigestSet {
    pub md5: Option<String>,
    pub sha1: Option<String>,
    pub sha256: [u8; 32],
}

impl DigestSet {
    pub fn from_bytes(bytes: &[u8]) -> Self {
        Self {
            md5: Some(hex_bytes(&Md5::digest(bytes))),
            sha1: Some(hex_bytes(&Sha1::digest(bytes))),
            sha256: sha256_bytes(bytes),
        }
    }
}

pub fn sha256_bytes(bytes: &[u8]) -> [u8; 32] {
    let digest = Sha256::digest(bytes);
    let mut out = [0u8; 32];
    out.copy_from_slice(&digest);
    out
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    hex_bytes(&sha256_bytes(bytes))
}

pub(crate) fn hex_bytes(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0F) as usize] as char);
    }
    out
}

pub(crate) fn hex_eq_digest(hex: &str, digest: &[u8]) -> bool {
    if hex.len() != digest.len() * 2 {
        return false;
    }
    hex.as_bytes()
        .chunks_exact(2)
        .zip(digest.iter().copied())
        .all(|(pair, byte)| decode_hex_pair(pair) == Some(byte))
}

fn decode_hex_pair(pair: &[u8]) -> Option<u8> {
    let hi = decode_hex_nibble(pair[0])?;
    let lo = decode_hex_nibble(pair[1])?;
    Some((hi << 4) | lo)
}

fn decode_hex_nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_hex_matches_known_empty_digest() {
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn digest_set_computes_legacy_hashes_for_catalog_recognition() {
        let digests = DigestSet::from_bytes(b"abc");
        assert_eq!(
            digests.md5.as_deref(),
            Some("900150983cd24fb0d6963f7d28e17f72")
        );
        assert_eq!(
            digests.sha1.as_deref(),
            Some("a9993e364706816aba3e25717850c26c9cd0d89d")
        );
    }

    #[test]
    fn hex_eq_digest_accepts_mixed_case() {
        assert!(hex_eq_digest("E3B0", &[0xE3, 0xB0]));
    }

    #[test]
    fn hex_eq_digest_rejects_wrong_length() {
        assert!(!hex_eq_digest("e3", &[0xE3, 0xB0]));
    }
}
