use super::*;

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn cartridge_register_peek_does_not_admit_unsupported_dma_source() {
    or_bank_dma_projection();
    external_ram_dma_projection();
    bounded_split_dma_projection();
    linear_rom_dma_projection();
    banked_rom_dma_projection();
    normal_slot_dma_projection();
    let mut inserted = crate::cartridge::registers::tests::inserted_save_fixture();
    for enabled in [false, true, false] {
        inserted.lock_on_save.as_mut().unwrap().enabled = enabled;
        let source = VdpDmaSource::from_cartridge(&inserted, &[0; 4]);
        for address in [0x200000, 0x200002, 0x2003fe, 0x300000, 0x3ffffe] {
            if enabled {
                assert_eq!(
                    source.read(address),
                    Err(VdpDmaError::UnsupportedSource(address))
                );
            } else {
                assert_eq!(
                    source.read(address),
                    Ok(u16::from_be_bytes([
                        inserted.bytes[address as usize],
                        inserted.bytes[address as usize + 1]
                    ]))
                );
            }
        }
        assert!(source.read(0x200400).is_ok());
    }
    let cartridge =
        crate::cartridge::Cartridge::new(crate::cartridge::registers::tests::rom(0)).unwrap();
    let source = VdpDmaSource::from_cartridge(&cartridge, &[0; 4]);
    for address in [0x400000, 0x400002, 0x400004, 0x400006, 0x400008, 0x40fffe] {
        assert_eq!(
            source.read(address),
            Err(VdpDmaError::UnsupportedSource(address))
        );
    }
    assert_eq!(source.read(0x200), Ok(0x60fe));
}

fn or_bank_dma_projection() {
    let mut cart = crate::cartridge::registers::bank_tests::fixture();
    for bank in [0, 1, 2, 3, 0x1f, 0x20, 0x80] {
        cart.registers.as_mut().unwrap().write_bank(bank);
        for address in [0x1000, 0x9000, 0x11000, 0xf9000] {
            let expected = cart.read(address).map(|value| u16::from(value) * 0x101);
            let source = VdpDmaSource::from_cartridge(&cart, &[]);
            assert_eq!(
                source.read(address),
                expected.ok_or(VdpDmaError::UnsupportedSource(address))
            );
        }
        assert_eq!(
            VdpDmaSource::from_cartridge(&cart, &[]).read(0x100000),
            Ok(0xffff)
        );
        assert!(
            VdpDmaSource::from_cartridge(&cart, &[])
                .read(0x600004)
                .is_err()
        );
    }
}

fn linear_rom_dma_projection() {
    let cartridge = crate::cartridge::linear_rom::fixture(&[0x4e71, 0x60fc]);
    let before = cartridge.clone();
    let source = VdpDmaSource::from_cartridge(&cartridge, &[]);
    assert_eq!(source.read(0xffffc), Ok(0x1234));
    assert_eq!(source.read(0xffffe), Ok(0x5678));
    assert_eq!(
        source.read(0xfffff),
        Err(VdpDmaError::UnsupportedSource(0x100000))
    );
    for address in [0x100000, 0x200000, 0x3ffffe, 0x400000, 0x9ffffe] {
        assert_eq!(
            source.read(address),
            Err(VdpDmaError::UnsupportedSource(address))
        );
    }
    assert_eq!(cartridge, before);
}

fn banked_rom_dma_projection() {
    use crate::cartridge::ssf2::tests::{access, cartridge};
    let mut cart = cartridge(&[]);
    assert_eq!(
        VdpDmaSource::from_cartridge(&cart, &[]).read(0x80000),
        Ok(0x0101)
    );
    for slot in 1..8 {
        cart.ssf2
            .as_mut()
            .unwrap()
            .control(access(0xa130f1 + slot * 2), if slot == 7 { 9 } else { 8 });
    }
    for (address, expected) in [(0x80000, 0x0808), (0x37ffff, 0x0809), (0x3ffffe, 0x0909)] {
        assert_eq!(
            VdpDmaSource::from_cartridge(&cart, &[]).read(address),
            Ok(expected)
        );
    }
    assert_eq!(
        VdpDmaSource::from_cartridge(&cart, &[]).read(0x3fffff),
        Err(VdpDmaError::UnsupportedSource(0x400000))
    );
}

fn normal_slot_dma_projection() {
    use crate::cartridge::registers::LockOn::*;
    for mapping in [Empty, HalfMiB, MiB, TwoMiB, Patch] {
        let mut cartridge = crate::cartridge::Cartridge::new(
            crate::cartridge::registers::tests::normal_rom_fixture(mapping),
        )
        .unwrap();
        cartridge.lock_on = Some(mapping);
        let source = VdpDmaSource::from_cartridge(&cartridge, &[0; 4]);
        for address in [
            0x200000, 0x27fffe, 0x280000, 0x300000, 0x33fffe, 0x380000, 0x3ffffe,
        ] {
            assert_eq!(
                source.read(address),
                Ok(u16::from_be_bytes([
                    cartridge.read(address).unwrap(),
                    cartridge.read(address + 1).unwrap()
                ]))
            );
        }
        assert_eq!(
            source.read(0x3fffff),
            Err(VdpDmaError::UnsupportedSource(0x400000))
        );
        if mapping == Patch {
            for address in [0x300000, 0x33fffe, 0x340000, 0x380000, 0x3ffffe] {
                let index = 0x300000 + (address as usize & 0x3ffff);
                assert_eq!(
                    source.read(address),
                    Ok(u16::from_be_bytes(
                        cartridge.bytes[index..index + 2].try_into().unwrap()
                    ))
                );
            }
        }
    }
}

fn bounded_split_dma_projection() {
    for product in [b"GM T-172116-00", b"GM T-172026-04"] {
        let mut cart = crate::cartridge::ram::profile_tests::bounded_split_fixture(product);
        let save = cart.save.as_mut().unwrap();
        save.bytes[0] = 0x35;
        save.bytes[32767] = 0xa7;
        let source = VdpDmaSource::from_cartridge(&cart, &[]);
        for (address, value) in [
            (0x1ffffe, 0x1234),
            (0x200000, 0xff35),
            (0x20fffe, 0xffa7),
            (0x300000, 0xdef0),
            (0x3ffffe, 0x2468),
        ] {
            assert_eq!(source.read(address), Ok(value));
        }
        for address in [0x210000, 0x2ffffe] {
            assert_eq!(
                source.read(address),
                Err(VdpDmaError::UnsupportedSource(address))
            );
        }
        assert_eq!(
            source.read(0x20ffff),
            Err(VdpDmaError::UnsupportedSource(0x210000))
        );
    }
}

fn external_ram_dma_projection() {
    let ram: Vec<_> = (0..65536_u32)
        .map(|at| (at as u8).wrapping_mul(73) ^ (at >> 8) as u8)
        .collect();
    let source = VdpDmaSource::new(&[0x12, 0x34], &ram);
    for base in (0xc00000..=0xff0000).step_by(0x10000) {
        for offset in [0, 2, 0x3ffe, 0xdddc, 0xfffe] {
            let expected = u16::from_be_bytes([ram[offset], ram[offset + 1]]);
            assert_eq!(source.read(base + offset as u32), Ok(expected));
        }
    }
    assert_eq!(
        source.read(0xc0ffff),
        Ok(u16::from_be_bytes([ram[65535], ram[0]]))
    );
    for address in [
        0x800000, 0x9ffffe, 0xa00000, 0xa10000, 0xa13000, 0xbffffe, 0x1000000,
    ] {
        assert_eq!(
            source.read(address),
            Err(VdpDmaError::UnsupportedSource(address))
        );
    }
    assert_eq!(source.read(0), Ok(0x1234));
    let missing = VdpDmaSource::new(&[], &[]);
    assert_eq!(
        missing.read(0xcddddc),
        Err(VdpDmaError::UnsupportedSource(0xcddddc))
    );
    let partial = VdpDmaSource::new(&[], &[0x56, 0x78]);
    assert_eq!(partial.read(0xc00000), Ok(0x5678));
    assert_eq!(
        partial.read(0xc00002),
        Err(VdpDmaError::UnsupportedSource(0xc00002))
    );
    assert_eq!(
        source.read(0xffffff),
        Err(VdpDmaError::UnsupportedSource(0x1000000))
    );
}
