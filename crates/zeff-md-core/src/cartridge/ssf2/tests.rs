use super::*;
use crate::cartridge::Cartridge;
use std::sync::Arc;
use zeff_m68k::bus::{AccessKind, FunctionCode};

pub(crate) fn access(address: u32) -> BusAccess {
    BusAccess {
        logical_address: address,
        bus_address: address & !1,
        lanes: ByteLanes::Lower,
        kind: AccessKind::Data,
        function_code: FunctionCode::SupervisorData,
    }
}

pub(crate) fn cartridge(program: &[u16]) -> Cartridge {
    let mut rom = vec![0; ROM_BYTES];
    for page in 0..10 {
        rom[page * PAGE_BYTES..(page + 1) * PAGE_BYTES].fill(page as u8);
    }
    rom[..0x200].fill(0);
    rom[..4].copy_from_slice(&0xffff_f000_u32.to_be_bytes());
    rom[4..8].copy_from_slice(&0x200_u32.to_be_bytes());
    rom[0x200..0x204].copy_from_slice(&[0x60, 0xfe, 0x4e, 0x71]);
    for (index, word) in program.iter().enumerate() {
        rom[0x200 + index * 2..0x202 + index * 2].copy_from_slice(&word.to_be_bytes());
    }
    let mut cartridge = Cartridge::new(rom[..PAGE_BYTES].to_vec()).unwrap();
    let mut board = Ssf2::new();
    board.fixture_sha = Some(Sha256::digest(&rom).into());
    cartridge.bytes = Arc::from(rom);
    cartridge.ssf2 = Some(board);
    cartridge
}

pub(crate) fn page_program(cartridge: &mut Cartridge, page: usize, bytes: &[u8]) {
    Arc::make_mut(&mut cartridge.bytes)[page * PAGE_BYTES..page * PAGE_BYTES + bytes.len()]
        .copy_from_slice(bytes);
    cartridge.ssf2.as_mut().unwrap().fixture_sha = Some(Sha256::digest(&cartridge.bytes).into());
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn exact_identity_slots_and_atomic_control_state() {
    for (product, digest) in PROFILES {
        assert!(Ssf2::identity(
            ROM_BYTES,
            Some(product),
            Some(&[b' '; 12]),
            || digest.to_owned()
        ));
        for index in 0..64 {
            let mut changed = digest.as_bytes().to_vec();
            changed[index] = if changed[index] == b'0' { b'1' } else { b'0' };
            assert!(!Ssf2::identity(
                ROM_BYTES,
                Some(product),
                Some(&[b' '; 12]),
                || String::from_utf8(changed).unwrap()
            ));
        }
        assert!(!Ssf2::identity(
            ROM_BYTES - 1,
            Some(product),
            Some(&[b' '; 12]),
            || panic!("prefilter")
        ));
        assert!(!Ssf2::identity(
            ROM_BYTES,
            Some(b"GM UNKNOWN -00"),
            Some(&[b' '; 12]),
            || panic!("prefilter")
        ));
    }
    let mut cart = cartridge(&[]);
    assert!(!Ssf2::detect(&cart.bytes));
    assert!(matches!(
        Cartridge::new(cart.bytes.to_vec()),
        Err(super::super::LoadError::CartridgeSize(ROM_BYTES))
    ));
    for slot in 1..8 {
        let address = slot * PAGE_BYTES as u32;
        assert_eq!(cart.read(address), Some(slot as u8));
        for page in [0, 8, 9] {
            assert!(
                cart.ssf2
                    .as_mut()
                    .unwrap()
                    .control(access(0xa130f1 + slot * 2), page)
            );
            for offset in [0, 1, PAGE_BYTES as u32 - 1] {
                let source = page as usize * PAGE_BYTES + offset as usize;
                assert_eq!(cart.rom_source_offset(address + offset), Some(source));
                assert_eq!(cart.read(address + offset), Some(cart.bytes[source]));
            }
        }
    }
    let before = cart.ssf2.clone();
    assert!(cart.ssf2.as_mut().unwrap().control(access(0xa130f1), 2));
    assert_eq!(cart.ssf2, before);
    for address in [0xa130f1, 0xa130f2, 0x1a130f3] {
        assert!(!cart.ssf2.as_mut().unwrap().control(access(address), 8));
    }
    for value in [10, 16, 255] {
        assert!(!cart.ssf2.as_mut().unwrap().control(access(0xa130f3), value));
    }
    for lanes in [ByteLanes::Both, ByteLanes::Upper] {
        let mut control = access(0xa130f3);
        control.lanes = lanes;
        assert!(!cart.ssf2.as_mut().unwrap().control(control, 8));
    }
    assert_eq!(cart.ssf2, before);
    assert_eq!(cart.rom_source_offset(0), Some(0));
    assert_eq!(cart.read(0x400000), None);
    let mut writer = StateWriter::new();
    cart.ssf2.as_ref().unwrap().write_state(&mut writer);
    let state = writer.into_bytes();
    let mut restored = Ssf2::new();
    restored.read_state(&mut StateReader::new(&state)).unwrap();
    assert_eq!(restored.slots, before.unwrap().slots);
    for invalid in [10, 254, 255] {
        let mut bytes = state.clone();
        bytes[6] = invalid;
        let old = restored.clone();
        assert!(restored.read_state(&mut StateReader::new(&bytes)).is_err());
        assert_eq!(restored, old);
    }
    restored.power_on();
    assert_eq!(restored.slots, INITIAL_SLOTS);
}
