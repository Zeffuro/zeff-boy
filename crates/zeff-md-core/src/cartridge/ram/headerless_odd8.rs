#[cfg(test)]
use super::OddSaveLayout;

const VECTORS: &[u8; 8] = &[0x00, 0xff, 0xff, 0xf0, 0x00, 0x00, 0x04, 0x3e];
const PRODUCT: &[u8; 14] = b"\x67\x00\x01\x98\x42\x39\x00\xff\x70\x01\x23\xfc\xc0\x00";
const DECLARATION: &[u8; 12] = b"\x00\x00\x33\xdb\x00\xc0\x00\x00\x33\xdb\x00\xc0";
const DIGESTS: [[u8; 32]; 2] = [
    [
        0xf3, 0xb7, 0xaa, 0xc9, 0x3f, 0xb6, 0x1c, 0x38, 0xf3, 0x8a, 0xb9, 0x03, 0xb2, 0x4c, 0x9c,
        0xef, 0xd7, 0xa9, 0x5f, 0x67, 0xb2, 0x0c, 0x05, 0xd0, 0xd2, 0x0e, 0xfe, 0xbc, 0xe4, 0x4d,
        0x68, 0x7e,
    ],
    [
        0x59, 0x22, 0x0f, 0x40, 0x3d, 0x03, 0x20, 0x32, 0xae, 0x5b, 0xbc, 0x10, 0x50, 0xf2, 0xa0,
        0x7b, 0x11, 0x0d, 0xde, 0xdb, 0x14, 0xb0, 0x20, 0xb3, 0x8d, 0xb6, 0x9b, 0x69, 0x64, 0x23,
        0xae, 0x35,
    ],
];

pub(super) fn detect(rom: &[u8]) -> bool {
    use sha2::{Digest, Sha256};
    identity(
        rom.len(),
        rom.get(..8),
        rom.get(0x180..0x18e),
        rom.get(0x1b0..0x1bc),
        || Sha256::digest(rom).into(),
    )
}

fn identity(
    len: usize,
    vectors: Option<&[u8]>,
    product: Option<&[u8]>,
    declaration: Option<&[u8]>,
    digest: impl FnOnce() -> [u8; 32],
) -> bool {
    len == 0x20_0000
        && vectors == Some(VECTORS.as_slice())
        && product == Some(PRODUCT.as_slice())
        && declaration == Some(DECLARATION.as_slice())
        && DIGESTS.contains(&digest())
}

#[cfg(test)]
pub(crate) fn fixture() -> super::SaveMemory {
    super::SaveMemory {
        layout: OddSaveLayout::HeaderlessOddKiB8,
        start: super::SAVE_START,
        switched: false,
        initially_enabled: true,
        time_latch: false,
        enabled: true,
        write_protected: false,
        bytes: vec![0xff; 8192].into_boxed_slice(),
        serial: None,
    }
}

#[cfg(test)]
mod tests;
#[cfg(test)]
pub(super) use tests::contract;
