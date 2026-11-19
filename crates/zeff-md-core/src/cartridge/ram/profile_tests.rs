use super::*;

pub(crate) mod overlay_fixture;

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn serial_chip_policy_rejects_internally_valid_wrong_rom_profile() {
    expansion_bytes::contract();
    headerless_odd8::contract();
    bounded_odd::contract();
    bounded_switched::contract();
    reserved_switched::contract();
    bounded_bytes::contract();
    split_512_contract();
    bounded_split_save_contract();
    internal_ram_declarations_require_full_identity();
    content_odd8_requires_full_identity();
    fixed_parallel_requires_full_identity_and_live_mapping();
    let mut bytes = vec![0; 0x400000];
    bytes[0x180..0x18e].copy_from_slice(b"GM T-081586-00");
    bytes[0x1b0..0x1bc].copy_from_slice(b"RA\xe8\x40\x00\x20\x00\x00\x00\x20\x00\x01");
    let exact = SaveMemory::from_rom(&bytes).unwrap().unwrap();
    assert_eq!(exact.bytes.len(), 2048);
    exact.validate_state().unwrap();
    exact.validate_rom_profile(&bytes).unwrap();
    let mut foreign = exact.clone();
    foreign.layout = OddSaveLayout::SplitSerial256;
    foreign.bytes = vec![0xff; 256].into_boxed_slice();
    foreign.serial = Some(Eeprom::new_split_lines());
    foreign.validate_state().unwrap();
    assert!(foreign.validate_rom_profile(&bytes).is_err());
    let mut mismatched = exact.clone();
    mismatched.serial = Some(Eeprom::new_split_lines());
    assert!(mismatched.validate_state().is_err());
    bytes.resize(0x300000, 0);
    bytes[0x180..0x18e].copy_from_slice(b"GM T-081276 00");
    foreign.validate_rom_profile(&bytes).unwrap();
    assert!(exact.validate_rom_profile(&bytes).is_err());
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn cached_serial_policy_is_distinct_from_valid_banked_serial() {
    crate::cartridge::save_latch::contract();
    crate::cartridge::download::contract();
    upper_control_requires_live_writable_fixed_mapping();
    for college in [false, true] {
        let mut bytes = vec![0; 0x400000];
        bytes[0x180..0x18e].copy_from_slice(if college {
            b"GM T-81576 -00"
        } else {
            b"GM T-81476 -00"
        });
        bytes[0x1b0..0x1bc].copy_from_slice(b"RA\xe8\x40\x00\x20\x00\x00\x00\x20\x00\x01");
        if college {
            bytes[0x1b7] = 1;
        }
        let exact = SaveMemory::from_rom(&bytes).unwrap().unwrap();
        assert_eq!(exact.bytes.len(), 8192);
        exact.validate_state().unwrap();
        exact.validate_rom_profile(&bytes).unwrap();
        for offset in (0x180..0x18e).chain(0x1b0..0x1bc) {
            let mut wrong = bytes.clone();
            wrong[offset] ^= 1;
            if let Ok(Some(save)) = SaveMemory::from_rom(&wrong) {
                assert_ne!(save.layout, OddSaveLayout::SplitSerial8192);
            }
        }
        for size in [0x300000, 0x3fffff, 0x400001] {
            let mut wrong = bytes.clone();
            wrong.resize(size, 0);
            if let Ok(Some(save)) = SaveMemory::from_rom(&wrong) {
                assert_ne!(save.layout, OddSaveLayout::SplitSerial8192);
            }
        }
        let mut foreign = exact.clone();
        foreign.layout = OddSaveLayout::SplitSerial2048;
        foreign.bytes = vec![0xff; 2048].into_boxed_slice();
        foreign.serial = Some(Eeprom::new_split_2048());
        foreign.validate_state().unwrap();
        assert!(foreign.validate_rom_profile(&bytes).is_err());
        let mut mismatched = exact.clone();
        mismatched.serial = Some(Eeprom::new_split_2048());
        assert!(mismatched.validate_state().is_err());
        bytes[0x180..0x18e].copy_from_slice(b"GM T-081586-00");
        bytes[0x1b7] = 0;
        foreign.validate_rom_profile(&bytes).unwrap();
        assert!(exact.validate_rom_profile(&bytes).is_err());
    }
}

fn upper_control_requires_live_writable_fixed_mapping() {
    for layout in [OddSaveLayout::KiB8, OddSaveLayout::KiB32] {
        let save = SaveMemory {
            layout,
            start: SAVE_START,
            switched: false,
            initially_enabled: true,
            time_latch: false,
            enabled: true,
            write_protected: false,
            bytes: vec![0x47; layout.bytes()].into_boxed_slice(),
            serial: None,
        };
        assert!(save.unchanged_upper_control(1));
        for disabled in [false, true] {
            let mut changed = save.clone();
            changed.enabled = !disabled;
            changed.write_protected = !disabled;
            let before = changed.clone();
            assert!(!changed.unchanged_upper_control(1));
            assert_eq!(changed, before);
        }
    }
}

fn content_odd8_requires_full_identity() {
    let digest = [
        0x6e, 0x5f, 0x0f, 0x3a, 0xb2, 0xf8, 0x00, 0x77, 0x3a, 0xc2, 0xdf, 0x61, 0xa4, 0xcc, 0x60,
        0xfb, 0x44, 0x92, 0x3f, 0x33, 0xa6, 0x23, 0x5e, 0x30, 0xc3, 0xf3, 0x53, 0xe9, 0x21, 0x49,
        0x7a, 0x36,
    ];
    let mut bytes = vec![0; 0x200000];
    bytes[0x180..0x18e].copy_from_slice(b"GM T-      -00");
    bytes[0x1b0..0x1bc].copy_from_slice(b"RA\xf8\x00\x00\x20\x00\x01\x00\x20\x3f\xff");
    let product = bytes.get(0x180..0x18e);
    let declaration = bytes.get(0x1b0..0x1bc);
    assert!(content_odd8_identity(
        bytes.len(),
        product,
        declaration,
        || digest,
    ));
    for index in 0..32 {
        let mut wrong = digest;
        wrong[index] ^= 1;
        assert!(!content_odd8_identity(
            bytes.len(),
            product,
            declaration,
            || wrong,
        ));
    }
    for size in [0, 0x100000, bytes.len() - 1, bytes.len() + 1, 0x300000] {
        assert!(!content_odd8_identity(size, product, declaration, || {
            panic!("size mismatch hashed content")
        }));
    }
    for field in [0x180..0x18e, 0x1b0..0x1bc] {
        for offset in field {
            let mut wrong = bytes.clone();
            wrong[offset] ^= 1;
            assert!(!content_odd8_identity(
                wrong.len(),
                wrong.get(0x180..0x18e),
                wrong.get(0x1b0..0x1bc),
                || panic!("metadata mismatch hashed content"),
            ));
        }
    }
    for (product, declaration) in [(None, declaration), (product, None), (None, None)] {
        assert!(!content_odd8_identity(
            bytes.len(),
            product,
            declaration,
            || { panic!("missing metadata hashed content") }
        ));
    }
    for marker in [0, 0x5a] {
        bytes[0x200] = marker;
        let before = bytes.clone();
        assert_eq!(
            SaveMemory::from_rom(&bytes),
            Err(LoadError::UnsupportedSaveHardware)
        );
        assert_eq!(bytes, before);
    }
}

fn fixed_parallel_requires_full_identity_and_live_mapping() {
    for (size, product, declaration, hash, layout) in [
        (
            0x10_0000,
            b"GM T-50286 -00",
            b"            ",
            "997cbd682bc7c0636302f07219d9152244c8ae06028fd7d91463d35756630ce5",
            OddSaveLayout::FixedOddKiB8,
        ),
        (
            0x20_0000,
            b"GM G-5519  -00",
            b"RA\xf8\x20\x00\x20\x00\x01\x00\x20\x03\xff",
            "3796f407ae0bf3b74bbe53335adf57f295f0bf53ef7d82b603ac73350eac8cae",
            OddSaveLayout::Bytes512,
        ),
        (
            0x20_0000,
            b"GM T-119076-00",
            b"RA\xf8\x40\x00\x20\x00\x01\x00\x20\x07\xff",
            "2498de5f6c5d23a2dde9a5b60281dbfdc435ff6eb7d2937d770c2ff4f37db926",
            OddSaveLayout::BoundedParallel2048,
        ),
        (
            0x10_0000,
            b"GM T-119016-00",
            b"RA\xf8\x40\x00\x20\x00\x01\x00\x20\x07\xff",
            "21274f87c50a7c774b87833becc4e6c3736e12349c6b8abc81b92d041fd17554",
            OddSaveLayout::BoundedParallel2048,
        ),
        (
            0x20_0000,
            b"GM T-119096-00",
            b"RA\xf8\x40\x00\x20\x00\x01\x00\x20\x03\xff",
            "6e3aa98a12385d1215b5608e2f4267eea0efec2879e9c0a21620b355eb6aa2ab",
            OddSaveLayout::Bytes512,
        ),
        (
            0x10_0000,
            b"GM T-119046-00",
            b"RA\xf8\x40\x00\x20\x00\x01\x00\x20\x0f\xff",
            "5adf28ad772d885569a50a4e48408004da37a2d425a8fe90a5f2a3cba9a01d94",
            OddSaveLayout::BoundedParallel2048,
        ),
    ] {
        let digest =
            std::array::from_fn(|i| u8::from_str_radix(&hash[i * 2..i * 2 + 2], 16).unwrap());
        let identify = |len, product, declaration, digest| {
            fixed_parallel_identity(len, product, declaration, || digest)
        };
        assert_eq!(
            identify(size, Some(product), Some(declaration), digest),
            Some(layout)
        );
        for index in 0..32 {
            let mut wrong = digest;
            wrong[index] ^= 1;
            assert_eq!(
                identify(size, Some(product), Some(declaration), wrong),
                None
            );
        }
        for len in [0, size - 1, size + 1, 0x30_0000] {
            assert_eq!(
                fixed_parallel_identity(len, Some(product), Some(declaration), || {
                    panic!("size mismatch hashed content")
                }),
                None
            );
        }
        for index in 0..product.len() {
            let mut wrong = *product;
            wrong[index] ^= 1;
            assert_eq!(
                fixed_parallel_identity(size, Some(&wrong), Some(declaration), || {
                    panic!("product mismatch hashed content")
                }),
                None
            );
        }
        for index in 0..declaration.len() {
            let mut wrong = *declaration;
            wrong[index] ^= 1;
            assert_eq!(
                fixed_parallel_identity(size, Some(product), Some(&wrong), || {
                    panic!("declaration mismatch hashed content")
                }),
                None
            );
        }
        for (product, declaration) in [
            (None, Some(declaration.as_slice())),
            (Some(product.as_slice()), None),
        ] {
            assert_eq!(
                fixed_parallel_identity(size, product, declaration, || {
                    panic!("missing metadata hashed content")
                }),
                None
            );
        }
        let mut bytes = vec![0; size];
        bytes[0x180..0x18e].copy_from_slice(product);
        bytes[0x1b0..0x1bc].copy_from_slice(declaration);
        if let Some((OddSaveLayout::Bytes512, _)) = odd_geometry(&bytes) {
            let generic = SaveMemory::from_rom(&bytes).unwrap().unwrap();
            assert_eq!(generic.layout, OddSaveLayout::Bytes512);
            assert!(!generic.initially_enabled);
            assert!(!generic.enabled);
            assert_eq!(generic.read(SAVE_START + 1), None);
            generic.validate_state().unwrap();
            generic.validate_rom_profile(&bytes).unwrap();
            assert!(generic.validate_fixed_parallel(layout).is_err());
        } else if declaration[..2] != *b"RA" {
            assert_eq!(SaveMemory::from_rom(&bytes), Ok(None));
        } else {
            assert_eq!(
                SaveMemory::from_rom(&bytes),
                Err(LoadError::UnsupportedSaveHardware)
            );
        }
        let save = SaveMemory {
            layout,
            start: SAVE_START,
            switched: false,
            initially_enabled: true,
            time_latch: false,
            enabled: true,
            write_protected: false,
            bytes: vec![0xff; layout.bytes()].into_boxed_slice(),
            serial: None,
        };
        save.validate_state().unwrap();
        save.validate_fixed_parallel(layout).unwrap();
        let mut mapped = save.clone();
        let end = SAVE_START + (layout.bytes() * 2) as u32;
        assert!(!mapped.contains(end) && mapped.contains(end - 1));
        for (address, value) in [(SAVE_START + 1, 0x12), (end - 1, 0x34)] {
            mapped.write(address, value, 0, 1);
            mapped.write(address - 1, 0, 0, 1);
            assert_eq!(mapped.read(address), Some(value));
            assert_eq!(mapped.read(address - 1), Some(0xff));
        }
        if layout == OddSaveLayout::FixedOddKiB8 {
            for value in [0, 1, 2, 3, 0xff] {
                let before = mapped.clone();
                assert!(!mapped.control(value, false) && !mapped.control(value, true));
                assert!(!mapped.unchanged_upper_control(value));
                assert_eq!(mapped, before);
            }
        }
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
                _ => wrong.serial = Some(Eeprom::default()),
            }
            assert!(wrong.validate_fixed_parallel(layout).is_err());
        }
    }
}
fn internal_ram_declarations_require_full_identity() {
    for hash in [
        "d575dc6f55cc28b3a67152ec5879ef971b452c04866369386a7f59bc9675c43d",
        "1bc1cad1736cdebbe230fcb6b464cf02e0c53d8b4134084a1a3b007667885b3b",
    ] {
        let digest: [u8; 32] = std::array::from_fn(|index| {
            u8::from_str_radix(&hash[index * 2..index * 2 + 2], 16).unwrap()
        });
        let product_bytes = b"MDGM-0001-0001".as_slice();
        let product = Some(product_bytes);
        let declaration_bytes = b"RA\x20\x20\x00\xff\x00\x00\x00\xff\xff\xff".as_slice();
        let declaration = Some(declaration_bytes);
        assert!(internal_ram_declaration_identity(
            0x100000,
            product,
            declaration,
            || digest
        ));
        for index in 0..32 {
            let mut wrong = digest;
            wrong[index] ^= 1;
            assert!(!internal_ram_declaration_identity(
                0x100000,
                product,
                declaration,
                || wrong
            ));
        }
        for size in [0, 0x80000, 0xfffff, 0x100001, 0x200000] {
            assert!(!internal_ram_declaration_identity(
                size,
                product,
                declaration,
                || { panic!("size mismatch hashed content") }
            ));
        }
        for (field, other, is_product) in [
            (product_bytes, declaration, true),
            (declaration_bytes, product, false),
        ] {
            for index in 0..field.len() {
                let mut wrong = field.to_vec();
                wrong[index] ^= 1;
                let (product, declaration) = if is_product {
                    (Some(wrong.as_slice()), other)
                } else {
                    (other, Some(wrong.as_slice()))
                };
                assert!(!internal_ram_declaration_identity(
                    0x100000,
                    product,
                    declaration,
                    || { panic!("metadata mismatch hashed content") }
                ));
            }
        }
        for (product, declaration) in [(None, declaration), (product, None), (None, None)] {
            assert!(!internal_ram_declaration_identity(
                0x100000,
                product,
                declaration,
                || { panic!("missing metadata hashed content") }
            ));
        }
    }
    let mut bytes = vec![0; 0x100000];
    bytes[0x180..0x18e].copy_from_slice(b"MDGM-0001-0001");
    bytes[0x1b0..0x1bc].copy_from_slice(b"RA\x20\x20\x00\xff\x00\x00\x00\xff\xff\xff");
    let before = bytes.clone();
    assert_eq!(
        SaveMemory::from_rom(&bytes),
        Err(LoadError::UnsupportedSaveHardware)
    );
    assert_eq!(bytes, before);
    bytes[0x1b0..0x1bc].copy_from_slice(b"RA\xf8\x20\x00\x20\x00\x01\x00\x20\x3f\xff");
    let save = SaveMemory::from_rom(&bytes).unwrap().unwrap();
    assert_eq!(save.bytes.len(), 8192);
    assert!(save.enabled);
}

pub(crate) fn bounded_split_fixture(product: &[u8]) -> crate::cartridge::Cartridge {
    let mut rom = vec![0; 0x400000];
    rom[..8].copy_from_slice(&[0, 0xff, 0xff, 0xfc, 0, 0, 2, 0]);
    rom[0x180..0x18e].copy_from_slice(product);
    rom[0x1b0..0x1bc].copy_from_slice(b"RA\xf8\x20\x00\x20\x00\x01\x00\x20\xff\xff");
    rom[0x200..0x204].copy_from_slice(&[0x4e, 0x71, 0x60, 0xfc]);
    rom[0x1ffffe..0x200000].copy_from_slice(&[0x12, 0x34]);
    rom[0x200000..0x200002].copy_from_slice(&[0x56, 0x78]);
    rom[0x210000..0x210002].copy_from_slice(&[0x9a, 0xbc]);
    rom[0x300000..0x300002].copy_from_slice(&[0xde, 0xf0]);
    rom[0x3ffffe..0x400000].copy_from_slice(&[0x24, 0x68]);
    crate::cartridge::Cartridge::new(rom).unwrap()
}

fn bounded_split_save_contract() {
    use crate::hardware::bus::{Bus, BusError};
    use crate::hardware::sound::{CandidateBus, SoundFault, SoundMemory};
    use zeff_m68k::bus::{AccessKind, BusAccess, ByteLanes, Direction, FunctionCode};
    use zeff_m68k::timing::ClockedBus;
    use zeff_z80::Z80Bus;

    let access = |address, lanes| BusAccess {
        logical_address: address,
        bus_address: address & !1,
        kind: AccessKind::Data,
        function_code: FunctionCode::SupervisorData,
        lanes,
    };
    for product in [b"GM T-172116-00", b"GM T-172026-04"] {
        let mut cart = bounded_split_fixture(product);
        let mut save = cart.save.clone().unwrap();
        assert!(save.bounded_split() && save.banked_readable() && !save.protectable());
        assert_eq!(
            (save.read(0x200001), save.read(0x20ffff)),
            (Some(0xff), Some(0xff))
        );
        for value in 0..=u8::MAX {
            for word in [false, true] {
                assert!(save.control(value, word));
                assert_eq!(save, cart.save.clone().unwrap());
            }
        }
        for (address, value) in [(0x200001, 0x35), (0x20ffff, 0xa7)] {
            save.write(address, value, 0, 53693175);
            assert_eq!(save.read(address), Some(value));
        }
        save.write(0x200000, 0, 0, 53693175);
        assert_eq!(save.read(0x200000), Some(0xff));
        save.reset_mapping(0);
        assert_eq!(
            (save.read(0x200001), save.read(0x20ffff)),
            (Some(0x35), Some(0xa7))
        );
        for address in [0x200000, 0x20ffff] {
            assert!(save.contains(address));
        }
        for address in [0x1fffff, 0x210000, 0x2fffff, 0x300000] {
            assert!(!save.contains(address));
        }
        for field in 0..8 {
            let mut wrong = save.clone();
            match field {
                0 => wrong.layout = OddSaveLayout::KiB32,
                1 => wrong.start += 2,
                2 => wrong.switched = true,
                3 => wrong.initially_enabled = false,
                4 => wrong.enabled = false,
                5 => wrong.write_protected = true,
                6 => wrong.time_latch = true,
                _ => wrong.serial = Some(Eeprom::default()),
            }
            assert!(wrong.validate_rom_profile(&cart.bytes).is_err());
        }
        for offset in (0x180..0x18e).chain(0x1b0..0x1bc) {
            let mut wrong = cart.bytes.to_vec();
            wrong[offset] ^= 1;
            assert!(!bounded_split_profile(&wrong));
            if let Ok(Some(other)) = SaveMemory::from_rom(&wrong) {
                assert!(!other.bounded_split());
            }
            assert!(save.validate_rom_profile(&wrong).is_err());
        }
        for len in [0x300000, 0x3fffff, 0x400001] {
            let mut wrong = cart.bytes.to_vec();
            wrong.resize(len, 0);
            assert!(!bounded_split_profile(&wrong));
            if let Ok(Some(other)) = SaveMemory::from_rom(&wrong) {
                assert!(!other.bounded_split());
            }
            assert!(save.validate_rom_profile(&wrong).is_err());
        }
        cart.save = Some(save);
        let mut bus = Bus::new(cart, crate::Region::NtscUs);
        for (address, lanes, value) in [
            (0x200000, ByteLanes::Upper, 0),
            (0x200001, ByteLanes::Lower, 0x26),
            (0x20fffe, ByteLanes::Both, 0x1234),
        ] {
            let now = bus.cpu_clock;
            assert_eq!(
                bus.write_cycle(access(address, lanes), value, now).result,
                Ok(())
            );
        }
        for (address, value) in [
            (0x200000, 0xff26),
            (0x20fffe, 0xff34),
            (0x300000, 0xdef0),
            (0x3ffffe, 0x2468),
        ] {
            let now = bus.cpu_clock;
            assert_eq!(
                bus.read_cycle(access(address, ByteLanes::Both), now).result,
                Ok(value)
            );
        }
        let mut fetch = access(0x200000, ByteLanes::Both);
        fetch.kind = AccessKind::Prefetch;
        let now = bus.cpu_clock;
        assert_eq!(bus.read_cycle(fetch, now).result, Ok(0xff26));
        assert_eq!(bus.prefetch, crate::hardware::bus::Prefetch::Empty);
        for address in [0x210000, 0x2ffffe] {
            for lanes in [ByteLanes::Upper, ByteLanes::Lower, ByteLanes::Both] {
                let request = access(address + u32::from(lanes == ByteLanes::Lower), lanes);
                let now = bus.cpu_clock;
                assert_eq!(
                    bus.read_cycle(request, now).result,
                    Err(BusError::UnsupportedAccess {
                        address,
                        direction: Direction::Read
                    })
                );
                let now = bus.cpu_clock;
                assert_eq!(
                    bus.write_cycle(request, 0xa55a, now).result,
                    Err(BusError::UnsupportedAccess {
                        address,
                        direction: Direction::Write
                    })
                );
            }
        }
        for lanes in [ByteLanes::Upper, ByteLanes::Lower, ByteLanes::Both] {
            for value in [0, 1, 3, 0x100, 0xa55a, 0xffff] {
                let before = bus.cartridge.save.clone();
                let now = bus.cpu_clock;
                assert_eq!(
                    bus.write_cycle(
                        access(SAVE_CONTROL + u32::from(lanes == ByteLanes::Lower), lanes),
                        value,
                        now
                    )
                    .result,
                    Ok(())
                );
                assert_eq!(bus.cartridge.save, before);
            }
            let now = bus.cpu_clock;
            assert!(
                bus.read_cycle(access(SAVE_CONTROL, lanes), now)
                    .result
                    .is_err()
            );
        }
        for address in [0x200000, 0x200001, 0x20ffff, 0x210000, 0x2fffff] {
            assert_eq!(bus.cartridge.rom_source_offset(address), None);
            assert!(bus.cartridge.mode_sensitive_read(address));
        }
        assert_eq!(bus.cartridge.rom_source_offset(0x300000), Some(0x300000));
        for target in [0x210000, 0x2fffff, 0x200001] {
            for direction in [Direction::Read, Direction::Write] {
                let mut memory = SoundMemory::default();
                memory.set_reset(false);
                memory.bank = (target >> 15) as u16;
                let mut candidate = CandidateBus::new(&mut memory, &bus.cartridge);
                let address = (0x8000 | (target & 0x7fff)) as u16;
                if direction == Direction::Read {
                    candidate.cpu_read(address);
                } else {
                    candidate.cpu_write(address, 0x5a);
                }
                assert_eq!(
                    candidate.fault.get(),
                    if target == 0x200001 && direction == Direction::Read {
                        None
                    } else {
                        Some(SoundFault::UnsupportedAccess { address, direction })
                    }
                );
            }
        }
        bounded_split_codecs(&bus.cartridge);
    }
}

fn bounded_split_codecs(cart: &crate::cartridge::Cartridge) {
    use crate::{Emulator, LoadConfig};
    use zeff_emu_common::time::Reset;
    let mut source = Emulator::new(cart.bytes.to_vec(), LoadConfig::default()).unwrap();
    source
        .load_save_ram(&cart.save.as_ref().unwrap().bytes)
        .unwrap();
    let raw = source.encode_state().unwrap();
    assert_eq!(&raw[8..12], &crate::save_state::STATE_VERSION.to_le_bytes());
    let front = source.encode_frontend_state().unwrap();
    let mut fresh = Emulator::new(cart.bytes.to_vec(), LoadConfig::default()).unwrap();
    fresh.decode_state(&raw).unwrap();
    assert_eq!(fresh.dump_save_ram(), source.dump_save_ram());
    assert_eq!(fresh.encode_state().unwrap(), raw);
    fresh.decode_frontend_state(&front).unwrap();
    assert_eq!(fresh.encode_frontend_state().unwrap(), front);
    let mut reader = StateReader::new(&raw);
    reader.set_position(52);
    for _ in 0..3 {
        reader
            .read_slice(crate::constants::MAX_STATE_BYTES)
            .unwrap();
    }
    reader.read_bool().unwrap();
    let at = reader.position() + 4;
    assert_eq!(
        reader
            .read_slice(crate::constants::MAX_STATE_BYTES)
            .unwrap()
            .len(),
        32771
    );
    for (offset, value) in [(at + 1, 0), (at + 2 + 32768, 1)] {
        let mut bad = raw.clone();
        bad[offset] = value;
        let before = fresh.encode_frontend_state().unwrap();
        assert!(fresh.decode_state(&bad).is_err());
        assert_eq!(fresh.encode_frontend_state().unwrap(), before);
    }
    source.reset();
    assert_eq!(source.dump_save_ram(), fresh.dump_save_ram());
    assert_eq!(source.peek(0x200001), Some(0x26));
    assert_eq!(source.peek(0x20ffff), Some(0x34));
}

struct Split512Wire {
    chip: Eeprom,
    bytes: Vec<u8>,
    now: u64,
    held: Vec<u8>,
}

impl Split512Wire {
    fn new() -> Self {
        let mut bytes = vec![0xff; 512];
        for (at, value) in [(0, 0x69), (0x100, 0x96), (0xfe, 0x35), (0x1fe, 0xca)] {
            bytes[at] = value;
        }
        Self {
            chip: Eeprom::new_split_512(),
            bytes,
            now: 0,
            held: Vec::new(),
        }
    }

    fn line(&mut self, value: u8) {
        self.chip.write(value, self.now, 1_000_000, &mut self.bytes);
        self.now += 1;
    }

    fn start(&mut self) {
        for value in [1, 3, 2, 0] {
            self.line(value);
        }
    }

    fn stop(&mut self) {
        for value in [0, 2, 3] {
            self.line(value);
        }
    }

    fn send(&mut self, value: u8) -> bool {
        for bit in (0..8).rev() {
            let data = (value >> bit) & 1;
            for line in [data, data | 2, data] {
                self.line(line);
            }
        }
        self.line(1);
        self.line(3);
        let ack = self.chip.output() == 0;
        let mut writer = StateWriter::new();
        self.chip.write_state(&mut writer);
        self.held = writer.into_bytes();
        assert_eq!(self.held.len(), 38);
        let mut restored = Eeprom::new_split_512();
        restored
            .read_state(&mut StateReader::new(&self.held))
            .unwrap();
        assert_eq!(restored, self.chip);
        self.line(1);
        ack
    }

    fn receive(&mut self, ack: bool) -> u8 {
        let mut result = 0;
        for _ in 0..8 {
            self.line(1);
            self.line(3);
            result = (result << 1) | self.chip.output();
            self.line(1);
        }
        let data = u8::from(!ack);
        for line in [data, data | 2, data] {
            self.line(line);
        }
        result
    }
}

fn split_512_contract() {
    for control in 0xa0..=0xaf {
        let mut wire = Split512Wire::new();
        wire.start();
        assert!(wire.send(control));
        let bank = usize::from(control & 2) << 7;
        assert_eq!(
            u16::from_le_bytes(wire.held[6..8].try_into().unwrap()),
            bank as u16
        );
        for (offset, value) in [(7, wire.held[7] ^ 1), (7, 2), (27, 2), (4, 10)] {
            let mut bad = wire.held.clone();
            bad[offset] = value;
            let before = wire.chip.clone();
            assert!(wire.chip.read_state(&mut StateReader::new(&bad)).is_err());
            assert_eq!(wire.chip, before);
        }
        let address = if control & 1 == 0 {
            assert!(wire.send(0xfe));
            wire.start();
            assert!(wire.send(control | 1));
            bank | 0xfe
        } else {
            bank
        };
        let expected = wire.bytes[address];
        assert_eq!(wire.receive(false), expected);
        wire.stop();
    }
    let mut wire = Split512Wire::new();
    wire.start();
    assert!(!wire.send(0xb0));
    wire.stop();
    wire.start();
    assert!(wire.send(0xa2));
    assert!(wire.send(0xfe));
    let initial = wire.bytes.clone();
    for value in [0x11, 0x22, 0x33, 0x44] {
        assert!(wire.send(value));
    }
    assert_eq!(wire.bytes, initial);
    wire.stop();
    let mut expected = initial;
    for (at, value) in [(0x1fe, 0x11), (0x1ff, 0x22), (0x1f0, 0x33), (0x1f1, 0x44)] {
        expected[at] = value;
    }
    assert_eq!(wire.bytes, expected);
    let deadline = wire.now - 1 + 5_000;
    wire.now = deadline - 100;
    wire.start();
    assert!(!wire.send(0xa2));
    wire.stop();
    wire.now = deadline;
    wire.start();
    assert!(wire.send(0xa2));
    assert!(wire.send(0xff));
    wire.start();
    assert!(wire.send(0xa3));
    assert_eq!(wire.receive(true), 0x22);
    assert_eq!(wire.receive(false), 0x69);
    wire.stop();
    split_512_identity();
    let mut save = SaveMemory {
        layout: OddSaveLayout::SplitSerial512,
        start: SAVE_START,
        switched: true,
        initially_enabled: false,
        time_latch: false,
        enabled: false,
        write_protected: false,
        bytes: vec![0xff; 512].into_boxed_slice(),
        serial: Some(Eeprom::new_split_512()),
    };
    save.validate_state().unwrap();
    assert!(save.serial_write(0, true, true, 0, 1_000_000));
    assert_eq!(save.serial_read(SAVE_START + 1), Some(1));
    let owner = save.serial.clone();
    assert!(save.serial_write(1, true, true, 0, 1_000_000));
    assert_eq!(save.serial, owner);
    assert_eq!(save.serial_read(SAVE_START + 1), None);
    assert!(save.serial_write(0, true, false, 0, 1_000_000));
    assert!(save.serial_write(0, false, true, 0, 1_000_000));
    assert!(save.serial_write(0, true, true, 0, 1_000_000));
    assert_eq!(save.serial_read(SAVE_START + 1), Some(0));
    let mut writer = StateWriter::new();
    save.write_state(&mut writer);
    let bytes = writer.into_bytes();
    assert_eq!(bytes.len(), 552);
    let mut restored = save.clone();
    restored.read_state(&mut StateReader::new(&bytes)).unwrap();
    assert_eq!(restored, save);
    for serial in [
        Eeprom::new_split_lines(),
        Eeprom::new_split_2048(),
        Eeprom::new_split_8192(),
    ] {
        let mut wrong = save.clone();
        wrong.serial = Some(serial);
        assert!(wrong.validate_state().is_err());
    }
}

fn split_512_identity() {
    for (size, text) in [
        (
            0x300000,
            "5b735b443102f771bc7db49dcf34de640c10d212f5cb4af1251d956751f21072",
        ),
        (
            0x400000,
            "8bdd2fa68e70f727f6c86f8266ec703457e09c2940cbd8f82c3760e2177b88fe",
        ),
    ] {
        let mut digest = [0; 32];
        for (index, byte) in digest.iter_mut().enumerate() {
            *byte = u8::from_str_radix(&text[index * 2..index * 2 + 2], 16).unwrap();
        }
        let accepts = |length, product: &[u8], declaration: &[u8], hash| {
            headerless_split_512_identity(length, Some(product), Some(declaration), || hash)
        };
        assert!(accepts(size, b"GM T-81406 -00", b"            ", digest));
        for index in 0..32 {
            let mut wrong = digest;
            wrong[index] ^= 1;
            assert!(!accepts(size, b"GM T-81406 -00", b"            ", wrong));
        }
        for index in 0..26 {
            let mut product = *b"GM T-81406 -00";
            let mut declaration = *b"            ";
            if index < 14 {
                product[index] ^= 1;
            } else {
                declaration[index - 14] ^= 1;
            }
            assert!(!accepts(size, &product, &declaration, digest));
        }
        for length in [size - 1, size + 1, 0x100000, 0x700000 - size] {
            assert!(!accepts(length, b"GM T-81406 -00", b"            ", digest));
        }
        let mut rom = vec![0; size];
        rom[0x180..0x18e].copy_from_slice(b"GM T-81406 -00");
        rom[0x1b0..0x1bc].copy_from_slice(b"            ");
        assert!(SaveMemory::from_rom(&rom).unwrap().is_none());
    }
}

pub(crate) mod origin;
