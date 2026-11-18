use super::{OddSaveLayout, SAVE_START, SaveMemory};
use anyhow::{Result, ensure};
use sha2::{Digest, Sha256};

struct Identity {
    size: usize,
    product: &'static [u8],
    declaration: &'static [u8],
    digest: [u8; 32],
}

const IDENTITIES: [Identity; 1] = [Identity {
    size: 0x400000,
    product: b"GM T-274026-01",
    declaration: b"RA\xf8\x20\x00\x20\x00\x01\x00\x20\xff\xff",
    digest: [
        0x5b, 0xf6, 0xf7, 0x3f, 0xe1, 0xd7, 0xaf, 0x1d, 0x79, 0xd9, 0x95, 0x93, 0x6a, 0xdb, 0xcb,
        0x11, 0xb2, 0xcd, 0xfb, 0x4d, 0x66, 0x4e, 0x77, 0x61, 0x6a, 0x3c, 0x37, 0xa0, 0x10, 0xc1,
        0x2c, 0x14,
    ],
}];

fn identity(
    size: usize,
    product: Option<&[u8]>,
    declaration: Option<&[u8]>,
    digest: impl FnOnce() -> [u8; 32],
) -> bool {
    IDENTITIES
        .iter()
        .find(|expected| {
            size == expected.size
                && product == Some(expected.product)
                && declaration == Some(expected.declaration)
        })
        .is_some_and(|expected| digest() == expected.digest)
}

pub(super) fn detect(rom: &[u8]) -> bool {
    identity(
        rom.len(),
        rom.get(0x180..0x18e),
        rom.get(0x1b0..0x1bc),
        || Sha256::digest(rom).into(),
    )
}

impl SaveMemory {
    pub(super) fn new_reserved_switched() -> Self {
        Self {
            layout: OddSaveLayout::ReservedSwitchedRom,
            start: SAVE_START,
            switched: true,
            initially_enabled: false,
            time_latch: false,
            enabled: false,
            write_protected: false,
            bytes: Box::default(),
            serial: None,
        }
    }

    pub(crate) fn reserved_switched(&self) -> bool {
        self.layout == OddSaveLayout::ReservedSwitchedRom
    }

    pub(super) fn validate_reserved_switched(&self) -> Result<()> {
        ensure!(
            self.start == SAVE_START
                && self.switched
                && !self.initially_enabled
                && !self.time_latch
                && !self.enabled
                && !self.write_protected
                && self.serial.is_none()
                && self.bytes.is_empty(),
            "invalid MD reserved switched save mapping"
        );
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn reserved_switched_for_test() -> Self {
        Self::new_reserved_switched()
    }
}

#[cfg(test)]
pub(super) fn contract() {
    for expected in &IDENTITIES {
        assert_eq!(expected.product.len(), 14);
        assert_eq!(expected.declaration.len(), 12);
        assert!(identity(
            expected.size,
            Some(expected.product),
            Some(expected.declaration),
            || expected.digest
        ));
        for size in [
            0,
            0x280000,
            0x300000,
            expected.size - 1,
            expected.size + 1,
            0x800000,
        ] {
            assert!(!identity(
                size,
                Some(expected.product),
                Some(expected.declaration),
                || panic!()
            ));
        }
        for index in 0..26 {
            let mut product = expected.product.to_vec();
            let mut declaration = expected.declaration.to_vec();
            if index < 14 {
                product[index] ^= 1;
            } else {
                declaration[index - 14] ^= 1;
            }
            assert!(!identity(
                expected.size,
                Some(&product),
                Some(&declaration),
                || panic!()
            ));
        }
        for (product, declaration) in [
            (None, Some(expected.declaration)),
            (Some(expected.product), None),
            (None, None),
        ] {
            assert!(!identity(expected.size, product, declaration, || panic!()));
        }
        for index in 0..32 {
            let mut digest = expected.digest;
            digest[index] ^= 1;
            assert!(!identity(
                expected.size,
                Some(expected.product),
                Some(expected.declaration),
                || digest
            ));
        }
    }
    assert!(!identity(
        0x340000,
        Some(b"GM 00000000-00"),
        Some(b"RA\xe0\x20\x00\x20\x00\x00\x00\x20\x10\x00"),
        || panic!()
    ));
    let mut save = SaveMemory::new_reserved_switched();
    assert!(save.bytes.is_empty());
    assert!(save.rom_visible());
    assert!(!save.banked_readable());
    save.validate_state().unwrap();
    for value in 0..=u8::MAX {
        for word in [false, true] {
            let before = save.clone();
            assert!(!save.control(value, word));
            assert_eq!(save, before);
        }
    }
    for address in [
        0x200000, 0x200001, 0x203fff, 0x20ffff, 0x3fffff, 0xa13000, 0xa130f0, 0xa130fe,
    ] {
        let before = save.clone();
        assert!(!save.accepts_control_address(address));
        assert_eq!(save.read(address), None);
        save.write(address, 0xa5, 0, 1);
        assert_eq!(save, before);
    }
    for size in [0, 1, 8192, 32768] {
        let before = save.clone();
        assert!(save.load(&vec![0; size]).is_err());
        assert_eq!(save, before);
    }
    let before = save.clone();
    save.reset_mapping(0);
    assert_eq!(save, before);
    let mut writer = zeff_emu_common::save_state::StateWriter::new();
    save.write_state(&mut writer);
    let raw = writer.into_bytes();
    assert_eq!(raw, vec![0, 0]);
    let mut restored = SaveMemory::new_reserved_switched();
    restored
        .read_state(&mut zeff_emu_common::save_state::StateReader::new(&raw))
        .unwrap();
    assert_eq!(restored, save);
    for field in 0..8 {
        let mut invalid = save.clone();
        match field {
            0 => invalid.start += 2,
            1 => invalid.switched = false,
            2 => invalid.initially_enabled = true,
            3 => invalid.time_latch = true,
            4 => invalid.enabled = true,
            5 => invalid.write_protected = true,
            6 => invalid.bytes = vec![0; 1].into_boxed_slice(),
            _ => invalid.layout = OddSaveLayout::BoundedSwitchedOddKiB8,
        }
        assert!(invalid.validate_state().is_err());
    }
    assert!(save.validate_rom_profile(&vec![0; 0x400000]).is_err());
    crate::Emulator::reserved_switched_api_contract();
}
