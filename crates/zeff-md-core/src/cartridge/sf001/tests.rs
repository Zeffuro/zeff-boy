use super::*;

pub(crate) fn rom(revision: Revision) -> Vec<u8> {
    let mut bytes = vec![0; 0x400000];
    for bank in 0..64 {
        bytes[bank * 0x10000..(bank + 1) * 0x10000].fill(bank as u8);
    }
    bytes[..8].copy_from_slice(&[0, 0xff, 0xff, 0, 0, 0, 2, 0]);
    bytes[0x100..0x110].copy_from_slice(b"SEGA MEGA DRIVE ");
    bytes[0x180..0x18e].copy_from_slice(PRODUCT);
    bytes[0x1b0..0x1bc].fill(b' ');
    let checksum: u16 = match revision {
        Revision::HiddenSave => 0x3e08,
        Revision::UpperSave => 0x0836,
    };
    bytes[0x18e..0x190].copy_from_slice(&checksum.to_be_bytes());
    bytes[0x3ffffe..].fill(0);
    let sum = bytes[0x200..]
        .as_chunks::<2>()
        .0
        .iter()
        .fold(0_u16, |sum, pair| {
            sum.wrapping_add(u16::from_be_bytes([pair[0], pair[1]]))
        });
    bytes[0x3ffffe..].copy_from_slice(&checksum.wrapping_sub(sum).to_be_bytes());
    bytes
}

fn detect(bytes: &[u8]) -> Option<Revision> {
    Sf001::detect_revision(bytes, CartridgeHeader::parse(bytes).as_ref())
}

fn encode(board: &Sf001) -> Vec<u8> {
    let mut writer = StateWriter::new();
    board.write_state(&mut writer);
    writer.into_bytes()
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn exact_profiles_require_product_size_and_both_checksums() {
    for revision in [Revision::HiddenSave, Revision::UpperSave] {
        let bytes = rom(revision);
        assert_eq!(detect(&bytes), Some(revision));
        assert_eq!(Sf001::detect_revision(&bytes, None), None);
        for index in 0x180..0x190 {
            let mut changed = bytes.clone();
            changed[index] ^= 1;
            assert_eq!(detect(&changed), None);
        }
        let mut changed = bytes.clone();
        changed[0x200] ^= 1;
        assert_eq!(detect(&changed), None);
        for size in [0x3fffff, 0x400001] {
            let mut changed = bytes.clone();
            changed.resize(size, 0);
            assert_eq!(detect(&changed), None);
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn all_control_values_modes_windows_and_byte_offsets_have_exact_priority_and_lock() {
    for revision in [Revision::HiddenSave, Revision::UpperSave] {
        let mut board = Sf001::new(revision);
        for mode in [Mapping::Linear, Mapping::Switched, Mapping::Disconnected] {
            for window in 0..16 {
                for offset in 0..256 {
                    let address = window * 0x1000 + 0xe00 + offset;
                    for value in 0..=255_u8 {
                        board.mapping = mode;
                        board.locked = false;
                        assert!(board.control(address, value));
                        let expected = if value & 0x40 != 0 {
                            Mapping::Disconnected
                        } else if value & 0x80 != 0 {
                            Mapping::Switched
                        } else {
                            Mapping::Linear
                        };
                        assert_eq!(board.mapping(), expected);
                        assert_eq!(board.locked(), value & 0x20 != 0);
                        board.mapping = mode;
                        board.locked = true;
                        assert!(board.control(address, value));
                        assert_eq!(board.mapping(), mode);
                        assert!(board.locked());
                    }
                }
            }
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn invalid_controls_do_not_alias_and_ordinary_rom_writes_are_noops() {
    for revision in [Revision::HiddenSave, Revision::UpperSave] {
        let mut board = Sf001::new(revision);
        for address in 0..0x20000 {
            let valid = address <= 0xffff && address & 0xf00 == 0xe00;
            board.power_on();
            assert_eq!(board.control(address, 0xe0), valid);
            if valid {
                assert_eq!(board.mapping(), Mapping::Disconnected);
                assert!(board.locked());
            } else {
                assert_eq!(board.mapping(), Mapping::Linear);
                assert!(!board.locked());
                assert!(board.write(address, 0xe0));
                assert_eq!(board.mapping(), Mapping::Linear);
            }
        }
        for address in [0x400000, 0x410000, 0xffffff, 0x1000e00, u32::MAX] {
            assert!(!board.control(address, 0xe0));
        }
        board.power_on();
        for address in [0, 0xdff, 0xf00, 0x10000, 0x3fffff, 0xa130f0, 0xa130ff] {
            assert!(board.write(address, 0xe0));
        }
        assert_eq!(board.mapping(), Mapping::Linear);
        assert!(!board.locked());
        assert!(!board.write(0x410000, 0xe0));
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn every_rom_bank_boundary_and_switched_source_are_decoded_without_upper_aliases() {
    for revision in [Revision::HiddenSave, Revision::UpperSave] {
        let bytes = rom(revision);
        let mut board = Sf001::new(revision);
        for mapping in [Mapping::Linear, Mapping::Switched, Mapping::Disconnected] {
            board.mapping = mapping;
            for bank in 0..64_u32 {
                for low in [0, 1, 0xfffe, 0xffff] {
                    let address = bank * 0x10000 + low;
                    let expected = if mapping == Mapping::Disconnected {
                        Location::Disconnected
                    } else if mapping == Mapping::Switched && bank < 4 {
                        Location::Rom(0x380000 + address)
                    } else if mapping == Mapping::Switched
                        && revision == Revision::HiddenSave
                        && bank >= 60
                    {
                        Location::Sram(low as u16)
                    } else {
                        Location::Rom(address)
                    };
                    assert_eq!(board.decode(address), expected);
                    let value = match expected {
                        Location::Rom(offset) => Some(bytes[offset as usize]),
                        Location::Sram(_) => Some(0xff),
                        _ => None,
                    };
                    assert_eq!(board.read(address, |at| bytes.get(at).copied()), value);
                }
            }
            for address in [0x400000, 0x400001, 0x40fffe, 0x40ffff] {
                let expected = if revision == Revision::UpperSave {
                    Location::Sram(address as u16)
                } else {
                    Location::Unclaimed
                };
                assert_eq!(board.decode(address), expected);
            }
            for address in [0x410000, 0x7fffff, 0xa130f0, 0x1000000, u32::MAX] {
                assert_eq!(board.decode(address), Location::Unclaimed);
                assert_eq!(board.read(address, |at| bytes.get(at).copied()), None);
            }
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn both_sram_lanes_and_all_cells_mirror_and_survive_power_on() {
    for revision in [Revision::HiddenSave, Revision::UpperSave] {
        let mut board = Sf001::new(revision);
        assert!(board.save_bytes().iter().all(|byte| *byte == 0xff));
        let base = if revision == Revision::HiddenSave {
            0x3c0000
        } else {
            0x400000
        };
        board.control(0xe00, 0x80);
        for cell in 0..0x10000_u32 {
            let value = (cell ^ (cell >> 8)) as u8;
            assert!(board.write(base + cell, value));
            assert_eq!(board.save_bytes()[cell as usize], value);
        }
        let save = board.save_bytes().to_vec();
        for cell in 0..0x10000_u32 {
            let value = (cell ^ (cell >> 8)) as u8;
            for bank in if revision == Revision::HiddenSave {
                0x3c..0x40
            } else {
                0x40..0x41
            } {
                assert_eq!(board.read(bank * 0x10000 + cell, |_| None), Some(value));
            }
        }
        for value in [0, 0x40, 0x80, 0xa0, 0xe0] {
            board.power_on();
            board.control(0xe00, value);
            assert_eq!(board.save_bytes(), save);
            if revision == Revision::UpperSave {
                assert_eq!(board.read(0x400001, |_| None), Some(1));
                assert!(board.write(0x400001, 1));
            }
        }
        board.power_on();
        assert_eq!(board.revision(), revision);
        assert_eq!(board.mapping(), Mapping::Linear);
        assert!(!board.locked());
        assert_eq!(board.save_bytes(), save);
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn save_load_and_state_decode_reject_malformed_input_transactionally() {
    for revision in [Revision::HiddenSave, Revision::UpperSave] {
        let mut board = Sf001::new(revision);
        let save: Vec<_> = (0..SAVE_BYTES)
            .map(|index| (index ^ (index >> 8)) as u8)
            .collect();
        board.load_save(&save).unwrap();
        for length in [0, 1, 32768, SAVE_BYTES - 1, SAVE_BYTES + 1] {
            let before = board.clone();
            assert!(board.load_save(&vec![0; length]).is_err());
            assert_eq!(board, before);
        }
        for mapping in [Mapping::Linear, Mapping::Switched, Mapping::Disconnected] {
            for locked in [false, true] {
                board.mapping = mapping;
                board.locked = locked;
                let encoded = encode(&board);
                assert_eq!(encoded.len(), SAVE_BYTES + 7);
                let mut restored = Sf001::new(revision);
                let mut reader = StateReader::new(&encoded);
                restored.read_state(&mut reader).unwrap();
                assert!(reader.is_exhausted());
                assert_eq!(restored, board);
                assert_eq!(encode(&restored), encoded);
                for length in [0, 1, 2, 3, 6, 7, SAVE_BYTES + 6] {
                    let before = restored.clone();
                    assert!(
                        restored
                            .read_state(&mut StateReader::new(&encoded[..length]))
                            .is_err()
                    );
                    assert_eq!(restored, before);
                }
                for (index, value) in [(0, 0), (0, 3), (1, 3), (1, 255), (2, 2), (3, 1), (6, 1)] {
                    let mut malformed = encoded.clone();
                    malformed[index] = value;
                    let before = restored.clone();
                    assert!(
                        restored
                            .read_state(&mut StateReader::new(&malformed))
                            .is_err()
                    );
                    assert_eq!(restored, before);
                }
                let other = if revision == Revision::HiddenSave {
                    Revision::UpperSave
                } else {
                    Revision::HiddenSave
                };
                let mut wrong_revision = Sf001::new(other);
                let before = wrong_revision.clone();
                assert!(
                    wrong_revision
                        .read_state(&mut StateReader::new(&encoded))
                        .is_err()
                );
                assert_eq!(wrong_revision, before);
            }
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn journal_overlay_replays_mapping_lock_and_mirrored_sram_without_mutation() {
    let events = [
        (0xe00, 0),
        (0xe00, 0x40),
        (0xe01, 0x80),
        (0xfeff, 0xa0),
        (0x1e00, 0x20),
        (0xeeff, 0xe0),
        (0xdff, 0x80),
        (0x10e00, 0x40),
        (0x3c0000, 0x12),
        (0x3d0000, 0x34),
        (0x3fffff, 0x56),
        (0x400000, 0x78),
        (0x400001, 0x9a),
        (0x40ffff, 0xbc),
        (0xa130f0, 0),
        (0x410000, 0xde),
    ];
    for revision in [Revision::HiddenSave, Revision::UpperSave] {
        let bytes = rom(revision);
        let mut board = Sf001::new(revision);
        for mapping in [Mapping::Linear, Mapping::Switched, Mapping::Disconnected] {
            for locked in [false, true] {
                board.mapping = mapping;
                board.locked = locked;
                let original = board.clone();
                for first in events {
                    for second in events {
                        let writes = [first, second];
                        let mut published = board.clone();
                        for (address, value) in writes {
                            published.write(address, value);
                        }
                        for address in [
                            0, 0x3ffff, 0x40000, 0x3bffff, 0x3c0000, 0x3d0000, 0x3fffff, 0x400000,
                            0x400001, 0x40ffff, 0x410000,
                        ] {
                            assert_eq!(
                                board.read_after_writes(
                                    address,
                                    |at| bytes.get(at).copied(),
                                    &writes
                                ),
                                published.read(address, |at| bytes.get(at).copied())
                            );
                        }
                    }
                }
                assert_eq!(board, original);
            }
        }
    }
    let board = Sf001::new(Revision::HiddenSave);
    let bytes = rom(Revision::HiddenSave);
    assert_eq!(
        board.read_after_writes(
            0x3c0000,
            |at| bytes.get(at).copied(),
            &[(0xe00, 0x80), (0x3d0000, 0x37)]
        ),
        Some(0x37)
    );
    assert_eq!(
        board.read_after_writes(
            0x3d0000,
            |at| bytes.get(at).copied(),
            &[(0xe00, 0xa0), (0xe00, 0x40)]
        ),
        Some(0xff)
    );
    assert_eq!(
        board.read_after_writes(
            0x3c0000,
            |at| bytes.get(at).copied(),
            &[(0x3d0000, 0x37), (0xe00, 0x80)]
        ),
        Some(0xff)
    );
    assert_eq!(
        board.read_after_writes(
            0,
            |at| bytes.get(at).copied(),
            &[(0xe00, 0x80), (0xe00, 0x40)]
        ),
        None
    );
    assert_eq!(
        board.read_after_writes(
            0x3c0000,
            |at| bytes.get(at).copied(),
            &[(0xe00, 0x80), (0x3d0000, 0x37), (0xe00, 0), (0xe00, 0x80)]
        ),
        Some(0x37)
    );
    assert_eq!(board.save_bytes()[0], 0xff);
    assert_eq!(board.mapping(), Mapping::Linear);
    assert!(!board.locked());
}
