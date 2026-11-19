use super::{OddSaveLayout, SAVE_START, SaveMemory};
use anyhow::{Result, ensure};
use sha2::{Digest, Sha256};
use zeff_emu_common::save_state::{StateReader, StateWriter};

const CELLS: usize = 2048;
const END: u32 = SAVE_START + CELLS as u32;
const STATE_TAG: u16 = 0x4232;
const PRODUCT: &[u8] = b"GM 00000000-00";
const DECLARATION: &[u8] = b"RA\xe0\x20\x00\x20\x00\x00\x00\x20\x10\x00";
const DIGEST: [u8; 32] = [
    0x76, 0x87, 0xe0, 0x7b, 0x3e, 0xb4, 0x8f, 0x9d, 0xea, 0xb0, 0x08, 0x88, 0x7f, 0xea, 0x45, 0x0b,
    0x3a, 0x88, 0x9c, 0xc2, 0x58, 0xd7, 0x09, 0x54, 0x1f, 0x1c, 0x92, 0xa8, 0x56, 0xe7, 0x36, 0x00,
];

pub(super) fn byte(address: u32) -> bool {
    (SAVE_START..END).contains(&address)
}

fn identity(
    size: usize,
    product: Option<&[u8]>,
    declaration: Option<&[u8]>,
    digest: impl FnOnce() -> [u8; 32],
) -> bool {
    size == 0x340000
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
    pub(super) fn new_bounded_bytes() -> Self {
        Self {
            layout: OddSaveLayout::BoundedSwitchedBytes2048,
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

    pub(crate) fn bounded_bytes(&self) -> bool {
        self.layout == OddSaveLayout::BoundedSwitchedBytes2048
    }

    pub(crate) fn bounded_bytes_byte(&self, address: u32) -> bool {
        self.bounded_bytes() && byte(address)
    }

    pub(super) fn validate_bounded_bytes(&self) -> Result<()> {
        ensure!(
            self.start == SAVE_START
                && self.switched
                && !self.initially_enabled
                && !self.time_latch
                && self.serial.is_none()
                && self.bytes.len() == CELLS
                && (!self.write_protected || self.enabled),
            "invalid MD bounded byte save mapping"
        );
        Ok(())
    }

    pub(super) fn write_bounded_bytes_tag(&self, writer: &mut StateWriter) {
        if self.bounded_bytes() {
            writer.write_u16(STATE_TAG);
        }
    }

    pub(super) fn read_bounded_bytes_tag(&self, reader: &mut StateReader<'_>) -> Result<()> {
        if self.bounded_bytes() {
            ensure!(
                reader.read_u16()? == STATE_TAG,
                "MD bounded byte save state has no matching owner"
            );
        }
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn bounded_bytes_for_test() -> Self {
        Self::new_bounded_bytes()
    }
}

#[cfg(test)]
pub(super) fn contract() {
    assert!(identity(0x340000, Some(PRODUCT), Some(DECLARATION), || {
        DIGEST
    }));
    for size in [0, 0x280000, 0x33ffff, 0x340001, 0x400000, 0x800000] {
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
            0x340000,
            Some(&product),
            Some(&declaration),
            || panic!()
        ));
    }
    for index in 0..32 {
        let mut digest = DIGEST;
        digest[index] ^= 1;
        assert!(!identity(
            0x340000,
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
        assert!(!identity(0x340000, product, declaration, || panic!()));
    }
    let mut save = SaveMemory::new_bounded_bytes();
    assert!(save.rom_visible());
    assert!(!save.banked_readable());
    assert_eq!(save.bytes.as_ref(), &[0xff; CELLS]);
    assert!(save.control(1, false));
    for index in 0..CELLS {
        save.write(
            SAVE_START + index as u32,
            (index ^ (index >> 8)) as u8,
            0,
            1,
        );
    }
    for index in 0..CELLS {
        assert_eq!(
            save.read(SAVE_START + index as u32),
            Some((index ^ (index >> 8)) as u8)
        );
    }
    for address in [SAVE_START - 1, END, END + 1, 0x201000, 0x300000, 0x3fffff] {
        let before = save.clone();
        assert_eq!(save.read(address), None);
        save.write(address, 0x47, 0, 1);
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
    for address in [SAVE_START, SAVE_START + 1, END - 2, END - 1] {
        save.write(address, 0xa5, 0, 1);
    }
    assert_eq!(save.bytes, cells);
    let mut writer = StateWriter::new();
    save.write_state(&mut writer);
    let raw = writer.into_bytes();
    assert_eq!(&raw[..2], &STATE_TAG.to_le_bytes());
    let mut restored = SaveMemory::new_bounded_bytes();
    restored.read_state(&mut StateReader::new(&raw)).unwrap();
    assert_eq!(restored, save);
    for old in [&[0, 0][..], &[1, 0][..], &[][..], &[0x32][..]] {
        let before = restored.clone();
        assert!(restored.read_state(&mut StateReader::new(old)).is_err());
        assert_eq!(restored, before);
    }
    for index in 0..2 {
        let mut wrong = raw.clone();
        wrong[index] ^= 1;
        let before = restored.clone();
        assert!(restored.read_state(&mut StateReader::new(&wrong)).is_err());
        assert_eq!(restored, before);
    }
    save.reset_mapping(0);
    assert!(save.rom_visible());
    assert_eq!(save.bytes, cells);
    for size in [0, 2047, 2049, 4097, 8192] {
        let before = save.clone();
        assert!(save.load(&vec![0; size]).is_err());
        assert_eq!(save, before);
    }
    save.load(&vec![0x47; CELLS]).unwrap();
    assert_eq!(save.bytes.as_ref(), &[0x47; CELLS]);
    for field in 0..8 {
        let mut invalid = save.clone();
        match field {
            0 => invalid.start += 1,
            1 => invalid.switched = false,
            2 => invalid.initially_enabled = true,
            3 => invalid.time_latch = true,
            4 => invalid.bytes = vec![0; CELLS + 1].into_boxed_slice(),
            5 => invalid.write_protected = true,
            6 => invalid.serial = Some(super::Eeprom::default()),
            _ => invalid.layout = OddSaveLayout::BoundedSwitchedOddKiB8,
        }
        assert!(invalid.validate_state().is_err());
    }
    assert!(save.validate_rom_profile(&vec![0; 0x340000]).is_err());
    crate::Emulator::bounded_bytes_api_contract();
}
