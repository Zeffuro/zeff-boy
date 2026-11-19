use super::{OddSaveLayout, SAVE_START, SaveMemory};
use anyhow::{Result, ensure};
use sha2::{Digest, Sha256};

const CELLS: usize = 8192;
const END: u32 = SAVE_START + CELLS as u32 * 2;
const PRODUCT: &[u8] = b"GM T-68063 -00";
const DECLARATION: &[u8] = b"RA\xf8\x20\x00\x20\x00\x01\x00\x20\x3f\xff";
const DIGEST: [u8; 32] = [
    0x1d, 0x3f, 0x08, 0xe2, 0x35, 0xfa, 0x8e, 0x51, 0xb9, 0x65, 0x68, 0x20, 0xfa, 0xbf, 0xb5, 0xb7,
    0x03, 0xac, 0xaf, 0x36, 0xbc, 0x34, 0xd5, 0x37, 0x8a, 0x3f, 0x96, 0x13, 0xd4, 0x53, 0xbb, 0x76,
];

pub(super) fn domain(address: u32) -> bool {
    (SAVE_START..0x400000).contains(&address)
}

pub(super) fn byte(address: u32) -> bool {
    (SAVE_START..END).contains(&address) && address & 1 != 0
}

fn identity(
    size: usize,
    product: Option<&[u8]>,
    declaration: Option<&[u8]>,
    digest: impl FnOnce() -> [u8; 32],
) -> bool {
    size == 0x280000
        && product == Some(PRODUCT)
        && declaration == Some(DECLARATION)
        && digest() == DIGEST
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
    pub(super) fn new_bounded_switched() -> Self {
        Self {
            layout: OddSaveLayout::BoundedSwitchedOddKiB8,
            start: SAVE_START,
            switched: true,
            initially_enabled: false,
            time_latch: false,
            enabled: false,
            write_protected: false,
            bytes: vec![0xff; CELLS].into_boxed_slice(),
            serial: None,
        }
    }

    pub(crate) fn bounded_switched(&self) -> bool {
        self.layout == OddSaveLayout::BoundedSwitchedOddKiB8
    }

    pub(crate) fn bounded_switched_byte(&self, address: u32) -> bool {
        self.bounded_switched() && byte(address)
    }

    pub(super) fn validate_bounded_switched(&self) -> Result<()> {
        ensure!(
            self.start == SAVE_START
                && self.switched
                && !self.initially_enabled
                && !self.time_latch
                && self.serial.is_none()
                && self.bytes.len() == CELLS
                && (!self.write_protected || self.enabled),
            "invalid MD bounded switched save mapping"
        );
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn bounded_switched_for_test() -> Self {
        Self::new_bounded_switched()
    }
}

#[cfg(test)]
pub(super) fn contract() {
    assert!(identity(0x280000, Some(PRODUCT), Some(DECLARATION), || {
        DIGEST
    }));
    for size in [0, 0x200000, 0x280001, 0x300000, 0x400000] {
        assert!(!identity(
            size,
            Some(PRODUCT),
            Some(DECLARATION),
            || panic!()
        ));
    }
    for index in 0..26 {
        let mut product = PRODUCT.to_vec();
        let mut declaration = DECLARATION.to_vec();
        if index < 14 {
            product[index] ^= 1;
        } else {
            declaration[index - 14] ^= 1;
        }
        assert!(!identity(
            0x280000,
            Some(&product),
            Some(&declaration),
            || panic!()
        ));
    }
    for index in 0..32 {
        let mut digest = DIGEST;
        digest[index] ^= 1;
        assert!(!identity(
            0x280000,
            Some(PRODUCT),
            Some(DECLARATION),
            || digest
        ));
    }
    for (product, declaration) in [
        (None, Some(DECLARATION)),
        (Some(PRODUCT), None),
        (None, None),
    ] {
        assert!(!identity(0x280000, product, declaration, || panic!()));
    }
    let mut save = SaveMemory::new_bounded_switched();
    assert!(save.rom_visible());
    assert!(!save.banked_readable());
    save.control(1, false);
    for index in 0..CELLS {
        save.write(
            SAVE_START + index as u32 * 2 + 1,
            (index ^ (index >> 8)) as u8,
            0,
            1,
        );
    }
    for index in 0..CELLS {
        assert_eq!(
            save.read(SAVE_START + index as u32 * 2 + 1),
            Some((index ^ (index >> 8)) as u8)
        );
    }
    for address in [SAVE_START, END, END + 1, 0x204001, 0x300001, 0x3fffff] {
        let before = save.clone();
        assert_eq!(save.read(address), None);
        save.write(address, 0xa5, 0, 1);
        assert_eq!(save, before);
    }
    for value in 0..=u8::MAX {
        for word in [false, true] {
            let mut candidate = save.clone();
            let before = candidate.clone();
            let accepted = candidate.control(value, word);
            assert_eq!(accepted, !word && value <= 3);
            if accepted {
                assert_eq!(candidate.enabled, value & 1 != 0);
                assert_eq!(candidate.write_protected, value == 3);
                candidate.validate_state().unwrap();
            } else {
                assert_eq!(candidate, before);
            }
        }
    }
    save.control(3, false);
    let cells = save.bytes.clone();
    save.write(SAVE_START + 5, 0x12, 0, 1);
    assert_eq!(save.bytes, cells);
    let mut writer = zeff_emu_common::save_state::StateWriter::new();
    save.write_state(&mut writer);
    let raw = writer.into_bytes();
    let mut restored = SaveMemory::new_bounded_switched();
    restored
        .read_state(&mut zeff_emu_common::save_state::StateReader::new(&raw))
        .unwrap();
    assert_eq!(restored, save);
    save.reset_mapping(0);
    assert!(save.rom_visible());
    assert_eq!(save.bytes, cells);
    for size in [0, CELLS - 1, CELLS + 1, 32768] {
        let before = save.clone();
        assert!(save.load(&vec![0; size]).is_err());
        assert_eq!(save, before);
    }
    save.load(&vec![0x47; CELLS]).unwrap();
    assert_eq!(save.bytes.as_ref(), &[0x47; CELLS]);
    for field in 0..6 {
        let mut invalid = save.clone();
        match field {
            0 => invalid.start += 2,
            1 => invalid.switched = false,
            2 => invalid.initially_enabled = true,
            3 => invalid.time_latch = true,
            4 => invalid.bytes = vec![0; CELLS + 1].into_boxed_slice(),
            _ => invalid.write_protected = true,
        }
        assert!(invalid.validate_state().is_err());
    }
    assert!(save.validate_rom_profile(&vec![0; 0x280000]).is_err());
}
