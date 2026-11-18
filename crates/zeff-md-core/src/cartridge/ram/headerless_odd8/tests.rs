use super::*;
use crate::cartridge::ram::{SAVE_START, SaveMemory};
use zeff_emu_common::save_state::{StateReader, StateWriter};

pub(in crate::cartridge::ram) fn contract() {
    for digest in DIGESTS {
        assert!(identity(
            0x200000,
            Some(VECTORS),
            Some(PRODUCT),
            Some(DECLARATION),
            || digest
        ));
        for index in 0..digest.len() {
            let mut changed = digest;
            changed[index] ^= 1;
            assert!(!identity(
                0x200000,
                Some(VECTORS),
                Some(PRODUCT),
                Some(DECLARATION),
                || changed
            ));
        }
    }
    for size in [0, 0x100000, 0x1fffff, 0x200001, 0x300000] {
        assert!(!identity(
            size,
            Some(VECTORS),
            Some(PRODUCT),
            Some(DECLARATION),
            || panic!("wrong size hashed")
        ));
    }
    for field in 0..3 {
        let original: &[u8] = [
            VECTORS.as_slice(),
            PRODUCT.as_slice(),
            DECLARATION.as_slice(),
        ][field];
        for index in 0..original.len() {
            let mut changed = original.to_vec();
            changed[index] ^= 1;
            let mut fields = [
                Some(VECTORS.as_slice()),
                Some(PRODUCT.as_slice()),
                Some(DECLARATION.as_slice()),
            ];
            fields[field] = Some(&changed);
            assert!(!identity(
                0x200000,
                fields[0],
                fields[1],
                fields[2],
                || panic!("wrong header hashed")
            ));
        }
        let mut fields = [
            Some(VECTORS.as_slice()),
            Some(PRODUCT.as_slice()),
            Some(DECLARATION.as_slice()),
        ];
        fields[field] = None;
        assert!(!identity(
            0x200000,
            fields[0],
            fields[1],
            fields[2],
            || panic!("missing header hashed")
        ));
    }
    let mut rom = vec![0; 0x200000];
    rom[..8].copy_from_slice(VECTORS);
    rom[0x180..0x18e].copy_from_slice(PRODUCT);
    rom[0x1b0..0x1bc].copy_from_slice(DECLARATION);
    assert!(!detect(&rom));
    assert_eq!(SaveMemory::from_rom(&rom), Ok(None));
    let mut save = fixture();
    assert!(save.unclaimed_parallel_8k());
    assert!(save.validate_rom_profile(&rom).is_err());
    save.validate_state().unwrap();
    for address in [SAVE_START - 1, SAVE_START + 16384] {
        assert!(!save.contains(address));
    }
    assert!(save.contains(SAVE_START) && save.contains(SAVE_START + 16383));
    for index in 0..8192 {
        let address = SAVE_START + 2 * index + 1;
        let value = ((index * 73 + index / 256) & 255) as u8;
        save.write(address, value, 0, 1);
        save.write(address - 1, 0, 0, 1);
        assert_eq!(save.read(address), Some(value));
        assert_eq!(save.read(address - 1), Some(0xff));
    }
    for value in 0..=255 {
        let before = save.clone();
        assert!(!save.control(value, false) && !save.control(value, true));
        assert!(!save.unchanged_upper_control(value));
        assert_eq!(save, before);
    }
    let backing = save.bytes.clone();
    save.enabled = false;
    save.write_protected = true;
    save.reset_mapping(0);
    assert!(save.enabled && !save.write_protected && !save.rom_visible());
    assert_eq!(save.bytes, backing);
    let mut writer = StateWriter::new();
    save.write_state(&mut writer);
    let bytes = writer.into_bytes();
    let mut restored = fixture();
    let mut reader = StateReader::new(&bytes);
    restored.read_state(&mut reader).unwrap();
    assert!(reader.is_exhausted());
    assert_eq!(save, restored);
    for field in 0..8 {
        let mut wrong = save.clone();
        match field {
            0 => wrong.layout = OddSaveLayout::KiB8,
            1 => wrong.start += 2,
            2 => wrong.switched = true,
            3 => wrong.initially_enabled = false,
            4 => wrong.enabled = false,
            5 => wrong.write_protected = true,
            6 => wrong.time_latch = true,
            _ => wrong.serial = Some(crate::cartridge::eeprom::Eeprom::default()),
        }
        assert!(
            wrong
                .validate_fixed_parallel(OddSaveLayout::HeaderlessOddKiB8)
                .is_err()
        );
    }
    for layout in [
        OddSaveLayout::FixedOddKiB8,
        OddSaveLayout::KiB32,
        OddSaveLayout::Bytes512,
    ] {
        let mut other = save.clone();
        other.layout = layout;
        assert!(!other.unclaimed_parallel_8k());
    }
    let before = save.clone();
    assert!(save.load(&[0; 8191]).is_err());
    assert_eq!(save, before);
    save.load(&[0x47; 8192]).unwrap();
    assert_eq!(save.bytes.as_ref(), &[0x47; 8192]);
}
