use crate::cartridge::Cartridge;

pub(crate) fn rom(profile: usize) -> Vec<u8> {
    let (size, header, actual) = [
        (0x200000, 0x30b9_u16, 0x1c2a_u16),
        (0x200000, 0xffff, 0x5d98),
        (0x200000, 0x6001, 0x0211),
        (0x100000, 0x0000, 0x8ec8),
    ][profile];
    let mut bytes = vec![0; size];
    bytes[..8].copy_from_slice(&[0, 0xff, 0xff, 0xfc, 0, 0, 2, 0]);
    bytes[0x18e..0x190].copy_from_slice(&header.to_be_bytes());
    bytes[0x200..0x204].copy_from_slice(&[0x60, 0xfe, 0x4e, 0x71]);
    let sum = bytes[0x200..]
        .as_chunks::<2>()
        .0
        .iter()
        .fold(0_u16, |sum, pair| {
            sum.wrapping_add(u16::from_be_bytes([pair[0], pair[1]]))
        });
    let tail = bytes.len() - 2;
    bytes[tail..].copy_from_slice(&actual.wrapping_sub(sum).to_be_bytes());
    bytes
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn exact_checksum_pairs_and_size_select_only_two_latch_board() {
    super::bank_tests::assert_identity_mapping_and_codec();
    normal_slot_identities_and_projection();
    let mut inserted = inserted_save_fixture();
    assert_eq!(
        inserted.lock_on_save.as_ref().unwrap().bytes(),
        &[0xff; 512]
    );
    assert!(inserted.save.is_none());
    let pattern: Vec<_> = (0..512).map(|i| (i ^ (i >> 8)) as u8).collect();
    inserted
        .lock_on_save
        .as_mut()
        .unwrap()
        .load(&pattern)
        .unwrap();
    for (i, value) in pattern.into_iter().enumerate() {
        assert_eq!(
            inserted
                .lock_on_save
                .as_ref()
                .unwrap()
                .read(0x200001 + 2 * i as u32),
            Some(value)
        );
    }
    inserted
        .lock_on_save
        .as_mut()
        .unwrap()
        .load(&[0xff; 512])
        .unwrap();
    for enabled in [false, true, false] {
        inserted.lock_on_save.as_mut().unwrap().enabled = enabled;
        for address in [0x200000, 0x200001, 0x2003fe, 0x2003ff, 0x300000, 0x3fffff] {
            assert!(inserted.mode_sensitive_read(address));
            if enabled {
                assert_eq!(inserted.rom_source_offset(address), None);
                assert_eq!(
                    inserted.read(address),
                    (address < 0x300000 && address & 1 != 0).then_some(0xff)
                );
            } else {
                assert_eq!(
                    inserted.read(address),
                    Some(inserted.bytes[address as usize])
                );
            }
        }
        for address in [0x1fffff, 0x200400, 0x202000, 0x2fffff] {
            assert!(!inserted.mode_sensitive_read(address));
            assert_eq!(
                inserted.read(address),
                Some(inserted.bytes[address as usize])
            );
        }
    }
    for profile in 0..4 {
        let bytes = rom(profile);
        let cartridge = Cartridge::new(bytes.clone()).unwrap();
        let header = crate::CartridgeHeader::parse(&bytes).unwrap();
        let wrong_size = if bytes.len() == 0x100000 {
            0x200000
        } else {
            0x100000
        };
        assert!(super::CartridgeRegisters::detect(wrong_size, Some(&header)).is_none());
        let mut registers = cartridge.registers.unwrap();
        for (index, aliases) in [(0, [0x400000, 0x400002]), (1, [0x400004, 0x400006])] {
            for value in [0, 1, 0x42, 0xff] {
                registers.write(index, value);
                for address in aliases {
                    assert_eq!(registers.read(address), Some(value));
                }
            }
        }
        for address in [
            0x3ffffe, 0x400001, 0x400003, 0x400005, 0x400007, 0x400008, 0x40fffe, 0x410000,
            0xc00000,
        ] {
            assert_eq!(registers.read(address), None);
        }
        for change in 0..3 {
            let mut mismatch = bytes.clone();
            match change {
                0 => mismatch[0x18e] ^= 1,
                1 => mismatch[0x200] ^= 1,
                _ => {
                    mismatch.extend([0, 0]);
                }
            }
            assert!(Cartridge::new(mismatch).unwrap().registers.is_none());
        }
    }
}

pub(crate) fn inserted_save_fixture() -> Cartridge {
    inserted_save_fixture_with_rom(normal_rom_fixture(super::LockOn::TwoMiB))
}

pub(crate) fn inserted_save_fixture_with_rom(bytes: Vec<u8>) -> Cartridge {
    assert_eq!(bytes.len(), 0x400000);
    let mut cartridge = Cartridge::new(bytes).unwrap();
    assert!(cartridge.lock_on.is_none());
    cartridge.lock_on = Some(super::LockOn::TwoMiB);
    cartridge.lock_on_save = Some(super::LockOnSave::new());
    assert!(!cartridge.supports_rom_overlay());
    cartridge
}

pub(crate) fn normal_rom_fixture(mapping: super::LockOn) -> Vec<u8> {
    use super::LockOn::*;
    let size = match mapping {
        Empty => 0x200000,
        HalfMiB => 0x280000,
        MiB | Patch => 0x340000,
        TwoMiB => 0x400000,
    };
    let mut bytes = vec![0; size];
    for (index, chunk) in bytes.chunks_mut(0x40000).enumerate() {
        chunk.fill(0x20 + index as u8);
    }
    bytes[..0x200].fill(0);
    bytes[..8].copy_from_slice(&[0, 0xff, 0xff, 0xfc, 0, 0, 2, 0]);
    bytes[0x180..0x18e].copy_from_slice(b"GM MK-1563 -00");
    bytes[0x200..0x204].copy_from_slice(&[0x60, 0xfe, 0x4e, 0x71]);
    bytes
}

fn normal_slot_identities_and_projection() {
    use super::LockOn;
    for (size, hash, mapping) in LockOn::IDENTITIES {
        assert_eq!(LockOn::from_identity(size, hash), Some(mapping));
        for index in 0..32 {
            let mut wrong = hash;
            wrong[index] ^= 1;
            assert_eq!(LockOn::from_identity(size, wrong), None);
        }
        for other_size in [0x200000, 0x280000, 0x340000, 0x400000, size + 1] {
            if other_size != size {
                assert_eq!(LockOn::from_identity(other_size, hash), None);
            }
        }
        let bytes = normal_rom_fixture(mapping);
        assert_eq!(LockOn::detect(&bytes), None);
        let mut cartridge = Cartridge::new(bytes).unwrap();
        assert_eq!(cartridge.lock_on, None);
        cartridge.lock_on = Some(mapping);
        for address in [
            0x1ffffe, 0x1fffff, 0x200000, 0x200001, 0x27ffff, 0x280000, 0x280001, 0x2fffff,
            0x300000, 0x300001, 0x33ffff, 0x340000, 0x380000, 0x3fffff,
        ] {
            let index = match (address < 0x200000, mapping) {
                (true, _) => Some(address as usize),
                (_, LockOn::Empty) => None,
                (_, LockOn::HalfMiB) => Some(0x200000 + (address as usize & 0x7ffff)),
                (_, LockOn::MiB | LockOn::Patch) => Some(0x200000 + (address as usize & 0xfffff)),
                (_, LockOn::TwoMiB) => Some(address as usize),
            };
            assert_eq!(
                cartridge.read(address),
                Some(index.map_or(0xff, |i| cartridge.bytes[i]))
            );
        }
        if mapping == LockOn::MiB {
            assert_ne!(cartridge.bytes[0x200000], cartridge.bytes[0x300000]);
            assert_eq!(cartridge.read(0x300000), Some(cartridge.bytes[0x200000]));
            for value in [1, 1, 0] {
                assert!(cartridge.lock_on.as_mut().unwrap().write_control(value));
                for address in [0x2fffff, 0x300000, 0x33ffff, 0x340000, 0x380000, 0x3fffff] {
                    let index = if address < 0x300000 {
                        address as usize
                    } else if value == 1 {
                        0x300000 + (address as usize & 0x3ffff)
                    } else {
                        0x200000 + (address as usize & 0xfffff)
                    };
                    assert_eq!(cartridge.read(address), Some(cartridge.bytes[index]));
                    assert_eq!(cartridge.rom_source_offset(address), Some(index));
                }
            }
        } else {
            let before = cartridge.clone();
            assert!(!cartridge.lock_on.as_mut().unwrap().write_control(1));
            assert_eq!(cartridge, before);
        }
        assert!(cartridge.save.is_none());
    }
}
