use sha2::{Digest, Sha256};

struct Profile {
    bytes: usize,
    declaration: [u8; 12],
    digest: [u8; 32],
}

const PROFILES: [Profile; 3] = [
    Profile {
        bytes: 1048576,
        declaration: [
            0x52, 0x41, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20,
        ],
        digest: [
            0xe0, 0x5e, 0x03, 0xf0, 0xb3, 0x37, 0x75, 0xe5, 0xcc, 0x84, 0x4c, 0x5a, 0xdb, 0x8f,
            0xed, 0xd4, 0x1a, 0xbb, 0x72, 0x1f, 0xd3, 0x77, 0x8f, 0xf0, 0x72, 0x09, 0x21, 0x12,
            0xba, 0x5c, 0xbc, 0xb9,
        ],
    },
    Profile {
        bytes: 1048576,
        declaration: [
            0x52, 0x41, 0xe0, 0x20, 0x00, 0x20, 0x00, 0x01, 0x00, 0x20, 0x3f, 0xff,
        ],
        digest: [
            0x28, 0x63, 0x52, 0x07, 0x56, 0xf6, 0x9e, 0x87, 0x28, 0x52, 0x85, 0x59, 0xc8, 0x2f,
            0xed, 0x99, 0x11, 0x87, 0xed, 0x1f, 0x60, 0x83, 0xe7, 0x8a, 0xa4, 0xfe, 0x27, 0x2d,
            0xf1, 0x1a, 0x34, 0x02,
        ],
    },
    Profile {
        bytes: 524288,
        declaration: [
            0x52, 0x41, 0xf8, 0x20, 0x00, 0x20, 0x00, 0x00, 0x00, 0x20, 0x3f, 0xff,
        ],
        digest: [
            0x76, 0x1b, 0xbe, 0xa2, 0x8a, 0x5e, 0x4b, 0xbe, 0x12, 0xd0, 0x18, 0xb3, 0x44, 0x68,
            0x02, 0xca, 0xbf, 0xec, 0xbd, 0x3a, 0x0a, 0x0c, 0x48, 0xc3, 0xe3, 0x65, 0x09, 0x57,
            0x8c, 0x91, 0xdb, 0x6b,
        ],
    },
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct LinearRomOnly([u8; 32]);

impl LinearRomOnly {
    pub fn detect(rom: &[u8]) -> Option<Self> {
        Self::matching_identity(rom.len(), rom.get(0x1b0..0x1bc), || {
            Sha256::digest(rom).into()
        })
    }

    pub fn matches_digest(self, digest: [u8; 32]) -> bool {
        self.0 == digest
    }

    pub(crate) fn matching_identity(
        size: usize,
        declaration: Option<&[u8]>,
        digest: impl FnOnce() -> [u8; 32],
    ) -> Option<Self> {
        let eligible = |profile: &Profile| {
            profile.bytes == size && declaration == Some(profile.declaration.as_slice())
        };
        if !PROFILES.iter().any(eligible) {
            return None;
        }
        let actual = digest();
        PROFILES
            .iter()
            .any(|profile| eligible(profile) && profile.digest == actual)
            .then_some(Self(actual))
    }

    #[cfg(test)]
    pub(crate) fn identity(
        size: usize,
        declaration: Option<&[u8]>,
        digest: impl FnOnce() -> [u8; 32],
    ) -> bool {
        Self::matching_identity(size, declaration, digest).is_some()
    }
}

impl super::Cartridge {
    pub(crate) fn linear_rom_compatible(&self) -> bool {
        matches!(self.bytes.len(), 0x80000 | 0x100000)
            && self
                .linear_rom
                .is_some_and(|owner| owner.matches_digest(self.rom_digest()))
            && self.svp.is_none()
            && self.save.is_none()
            && self.save_latch.is_none()
            && self.download.is_none()
            && self.unprotected_rom.is_none()
            && self.parallel_adapter.is_none()
            && self.registers.is_none()
            && self.protection.is_none()
            && self.sf001.is_none()
            && self.sf002.is_none()
            && self.sf004.is_none()
            && self.extended_ssf.is_none()
            && self.realtec.is_none()
            && self.ssf2.is_none()
            && self.lock_on.is_none()
            && self.lock_on_save.is_none()
    }
}

#[cfg(test)]
pub(crate) fn fixture(words: &[u16]) -> super::Cartridge {
    fixture_with_len(words, 0x100000)
}

#[cfg(test)]
pub(crate) fn fixture_with_len(words: &[u16], size: usize) -> super::Cartridge {
    let mut bytes = vec![0x19; size];
    bytes[..8].copy_from_slice(&[0, 0xff, 0xf0, 0, 0, 0, 2, 0]);
    bytes[0x100..0x200].fill(b' ');
    for (index, word) in words.iter().copied().chain([0x4e71, 0x4e71]).enumerate() {
        bytes[0x200 + index * 2..0x202 + index * 2].copy_from_slice(&word.to_be_bytes());
    }
    bytes[size - 4..].copy_from_slice(&[0x12, 0x34, 0x56, 0x78]);
    let mut cart = super::Cartridge::new(bytes).unwrap();
    assert!(cart.linear_rom.is_none());
    cart.linear_rom = Some(LinearRomOnly(Sha256::digest(cart.bytes.as_ref()).into()));
    cart
}
