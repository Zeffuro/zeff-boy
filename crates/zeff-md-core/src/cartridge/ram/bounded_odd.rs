use super::{OddSaveLayout, SAVE_START, SaveMemory};
use sha2::{Digest, Sha256};

const CELLS: usize = 2400;
const END: u32 = SAVE_START + CELLS as u32 * 2;
const DIGEST: [u8; 32] = [
    0x7e, 0xdf, 0x7c, 0x45, 0x74, 0x7d, 0xf7, 0xab, 0x69, 0x61, 0xc3, 0x42, 0x34, 0x93, 0xc7, 0x1a,
    0x66, 0x5a, 0xd9, 0x58, 0x60, 0xc2, 0x71, 0x53, 0xb6, 0x35, 0x89, 0xca, 0x6b, 0x30, 0x2e, 0x51,
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
    size == 0x100000
        && product == Some(&[0xff; 14])
        && declaration == Some(&[0xff; 12])
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
    pub(super) fn new_bounded_odd() -> Self {
        Self {
            layout: OddSaveLayout::BoundedOdd2400,
            start: SAVE_START,
            switched: false,
            initially_enabled: true,
            time_latch: false,
            enabled: true,
            write_protected: false,
            bytes: vec![0xff; CELLS].into_boxed_slice(),
            serial: None,
        }
    }

    pub(crate) fn bounded_odd(&self) -> bool {
        self.layout == OddSaveLayout::BoundedOdd2400
    }

    pub(crate) fn bounded_odd_byte(&self, address: u32) -> bool {
        self.bounded_odd() && byte(address)
    }

    #[cfg(test)]
    pub(crate) fn bounded_odd_for_test() -> Self {
        Self::new_bounded_odd()
    }
}

#[cfg(test)]
pub(super) fn contract() {
    assert!(identity(
        0x100000,
        Some(&[0xff; 14]),
        Some(&[0xff; 12]),
        || DIGEST
    ));
    for index in 0..32 {
        let mut wrong = DIGEST;
        wrong[index] ^= 1;
        assert!(!identity(
            0x100000,
            Some(&[0xff; 14]),
            Some(&[0xff; 12]),
            || wrong
        ));
    }
    for size in [0, 0xfffff, 0x100001, 0x200000] {
        assert!(!identity(
            size,
            Some(&[0xff; 14]),
            Some(&[0xff; 12]),
            || panic!()
        ));
    }
    for index in 0..26 {
        let mut product = [0xff; 14];
        let mut declaration = [0xff; 12];
        if index < 14 {
            product[index] ^= 1;
        } else {
            declaration[index - 14] ^= 1;
        }
        assert!(!identity(
            0x100000,
            Some(&product),
            Some(&declaration),
            || panic!()
        ));
    }
    for (product, declaration) in [
        (None, Some(&[0xff; 12][..])),
        (Some(&[0xff; 14][..]), None),
        (None, None),
    ] {
        assert!(!identity(0x100000, product, declaration, || panic!()));
    }
    let mut save = SaveMemory::new_bounded_odd();
    for index in 0..CELLS {
        let address = SAVE_START + index as u32 * 2 + 1;
        save.write(address, (index ^ (index >> 8)) as u8, 0, 1);
    }
    for index in 0..CELLS {
        assert_eq!(
            save.read(SAVE_START + index as u32 * 2 + 1),
            Some((index ^ (index >> 8)) as u8)
        );
    }
    for address in [SAVE_START, END - 2, END, END + 1, 0x3fffff] {
        let before = save.clone();
        assert_eq!(save.read(address), None);
        save.write(address, 0xa5, 0, 1);
        assert_eq!(save, before);
    }
    let cells = save.bytes.clone();
    save.reset_mapping(0);
    assert_eq!(save.bytes, cells);
    save.validate_state().unwrap();
    let mut writer = zeff_emu_common::save_state::StateWriter::new();
    save.write_state(&mut writer);
    let raw = writer.into_bytes();
    let mut restored = SaveMemory::new_bounded_odd();
    restored
        .read_state(&mut zeff_emu_common::save_state::StateReader::new(&raw))
        .unwrap();
    assert_eq!(restored, save);
    for size in [0, CELLS - 1, CELLS + 1, 16384] {
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
            1 => invalid.switched = true,
            2 => invalid.initially_enabled = false,
            3 => invalid.enabled = false,
            4 => invalid.write_protected = true,
            _ => invalid.time_latch = true,
        }
        assert!(invalid.validate_state().is_err());
    }
    for value in 0..=u8::MAX {
        for word in [false, true] {
            let before = save.clone();
            assert!(!save.control(value, word));
            assert_eq!(save, before);
        }
    }
    assert!(save.validate_rom_profile(&vec![0xff; 0x100000]).is_err());
}
