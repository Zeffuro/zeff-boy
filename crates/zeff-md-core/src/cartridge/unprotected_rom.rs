use super::*;

const IDENTITY: [u8; 32] = [
    0x66, 0xfa, 0x05, 0x1f, 0xb8, 0xaf, 0x8e, 0x5c, 0x7e, 0x8d, 0xca, 0x49, 0x68, 0xef, 0xe5, 0xc7,
    0xce, 0x1b, 0xd9, 0x64, 0xa2, 0x99, 0x0c, 0xc7, 0xcb, 0xab, 0x19, 0x06, 0xef, 0x95, 0x79, 0xd0,
];
const TIME_IDENTITY: [u8; 32] = [
    0xc4, 0x33, 0x7c, 0x1e, 0x5d, 0xc0, 0x7d, 0x87, 0xee, 0x1a, 0xfe, 0x7f, 0xa2, 0xf2, 0x4d, 0xb9,
    0x7d, 0x37, 0x3e, 0x88, 0xcf, 0xb8, 0xb8, 0x9f, 0x13, 0x16, 0x53, 0x9f, 0x28, 0x05, 0x46, 0x88,
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct UnprotectedRom {
    size: usize,
    digest: [u8; 32],
    time_reads: bool,
}

impl UnprotectedRom {
    pub(crate) fn detect(
        rom: &[u8],
        header: Option<&CartridgeHeader>,
        digest: impl FnOnce() -> [u8; 32],
    ) -> Option<Self> {
        #[cfg(test)]
        if matches!(
            rom.get(0x80..0x90),
            Some(b"UNPROTECTED_TEST" | b"UNPROTECTED_TIME")
        ) {
            return Self::from_rom_identity(rom, digest());
        }
        let header = header?;
        Self::identity(
            rom.len(),
            header.checksum,
            header.calculated_checksum,
            digest,
        )
    }

    fn identity(
        size: usize,
        stored: u16,
        calculated: u16,
        digest: impl FnOnce() -> [u8; 32],
    ) -> Option<Self> {
        let expected = match (size, stored, calculated) {
            (0x100000, 0x0c44, 0xba81) => IDENTITY,
            (0x200000, 0, 0x96b0) => TIME_IDENTITY,
            _ => return None,
        };
        let digest = digest();
        (digest == expected).then_some(Self {
            size,
            digest,
            time_reads: expected == TIME_IDENTITY,
        })
    }

    fn from_rom_identity(rom: &[u8], digest: [u8; 32]) -> Option<Self> {
        let size = rom.len();
        if (size == 0x100000 && digest == IDENTITY) || (size == 0x200000 && digest == TIME_IDENTITY)
        {
            return Some(Self {
                size,
                digest,
                time_reads: digest == TIME_IDENTITY,
            });
        }
        #[cfg(test)]
        if matches!(
            rom.get(0x80..0x90),
            Some(b"UNPROTECTED_TEST" | b"UNPROTECTED_TIME")
        ) {
            return Some(Self {
                size,
                digest,
                time_reads: rom.get(0x80..0x90) == Some(b"UNPROTECTED_TIME"),
            });
        }
        None
    }

    pub(crate) fn unclaimed_time(self, address: u32) -> bool {
        self.time_reads && (0xa13000..=0xa130ff).contains(&address)
    }
}

impl Cartridge {
    pub(crate) fn validate_unprotected_rom(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.unprotected_rom
                == UnprotectedRom::from_rom_identity(&self.bytes, self.rom_digest()),
            "MD unprotected ROM identity mismatch"
        );
        if self.unprotected_rom.is_some() {
            anyhow::ensure!(
                self.unprotected_rom_compatible(),
                "MD unprotected ROM owner conflict"
            );
        }
        Ok(())
    }

    pub(crate) fn unprotected_rom_compatible(&self) -> bool {
        self.bytes.len() <= 0x400000
            && self.save.is_none()
            && self.linear_rom.is_none()
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
pub(crate) fn check_identity() {
    let detect = |size, stored, calculated, digest| {
        UnprotectedRom::identity(size, stored, calculated, || digest)
    };
    assert!(detect(0x100000, 0x0c44, 0xba81, IDENTITY).is_some());
    let time = detect(0x200000, 0, 0x96b0, TIME_IDENTITY).unwrap();
    assert!(time.unclaimed_time(0xa13000));
    assert!(!time.unclaimed_time(0xa13100));
    assert!(detect(0x200000, 0x7f7f, 0x2aad, TIME_IDENTITY).is_none());
    assert!(detect(0x200000, 0, 0x96b0, IDENTITY).is_none());
    for (size, stored, calculated, identity) in [
        (0x100000, 0x0c44, 0xba81, IDENTITY),
        (0x200000, 0, 0x96b0, TIME_IDENTITY),
    ] {
        for index in 0..32 {
            let mut bad = identity;
            bad[index] ^= 1;
            assert!(detect(size, stored, calculated, bad).is_none());
        }
    }
    for (size, stored, calculated) in [
        (0x80000, 0x0c44, 0xba81),
        (0x100001, 0x0c44, 0xba81),
        (0x100000, 0x0c45, 0xba81),
        (0x100000, 0x0c44, 0xba80),
    ] {
        assert!(
            UnprotectedRom::identity(size, stored, calculated, || panic!(
                "unmatched profile hashed"
            ))
            .is_none()
        );
    }
}
