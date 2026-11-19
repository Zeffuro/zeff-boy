use crate::Region;
use crate::cartridge::Cartridge;
use crate::hardware::bus::{Bus, BusError};
use zeff_m68k::bus::{AccessKind, BusAccess, ByteLanes, Direction, FunctionCode};
use zeff_m68k::timing::ClockedBus;

pub(crate) fn rom(profile: usize, words: &[u16]) -> Vec<u8> {
    let (stored, actual) = [
        (0x0080_u16, 0x3dba_u16),
        (0, 0x5843),
        (0, 0x16cd),
        (0xffff, 0xd472),
        (0, 0x7037),
    ][profile];
    let mut bytes = vec![0; if profile == 3 { 0x200000 } else { 0x100000 }];
    bytes[..8].copy_from_slice(&[0, 0xff, 0xff, 0xfc, 0, 0, 2, 0]);
    bytes[0x18e..0x190].copy_from_slice(&stored.to_be_bytes());
    if profile == 4 {
        bytes[0x110..0x120].copy_from_slice(b"(C)1995 CREATON.");
        bytes[0x180..0x18e].copy_from_slice(b"MDGM-0001-0001");
    }
    for (index, word) in words.iter().enumerate() {
        bytes[0x200 + index * 2..0x202 + index * 2].copy_from_slice(&word.to_be_bytes());
    }
    let sum = bytes[0x200..]
        .as_chunks::<2>()
        .0
        .iter()
        .fold(0_u16, |sum, pair| {
            sum.wrapping_add(u16::from_be_bytes(*pair))
        });
    let checksum_word = bytes.len() - 2;
    bytes[checksum_word..].copy_from_slice(&actual.wrapping_sub(sum).to_be_bytes());
    bytes
}

fn access(address: u32, lanes: ByteLanes) -> BusAccess {
    BusAccess {
        logical_address: address,
        bus_address: address & !1,
        function_code: FunctionCode::SupervisorData,
        kind: AccessKind::Data,
        lanes,
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn fixed_protection_identity_and_bus_lanes_have_no_writable_aliases() {
    for (profile, expected) in [
        [Some(0x55), Some(0x0f), Some(0xc9), Some(0x18)],
        [Some(0x55), Some(0x0f), Some(0xaa), Some(0xf0)],
        [Some(0x55), Some(0x0f), None, None],
        [Some(0x63), Some(0x98), Some(0xc9), Some(0x18)],
    ]
    .into_iter()
    .enumerate()
    {
        let cartridge = Cartridge::new(rom(profile, &[0x60fe, 0x4e71])).unwrap();
        let other_size = if profile == 3 { 0x100000 } else { 0x200000 };
        assert!(super::FixedProtection::detect(other_size, cartridge.header.as_deref()).is_none());
        assert!(super::FixedProtection::detect(cartridge.bytes.len(), None).is_none());
        assert!(cartridge.registers.is_none());
        assert!(cartridge.save.is_none());
        let mut bus = Bus::new(cartridge, Region::NtscUs);
        for address in [0x400000, 0x400001, 0x400006, 0x40ffff] {
            for lanes in [ByteLanes::Upper, ByteLanes::Lower, ByteLanes::Both] {
                let before = bus.cartridge.clone();
                bus.write_cycle(access(address, lanes), 0x1234, bus.cpu_clock)
                    .result
                    .unwrap();
                assert_eq!(bus.cartridge, before);
            }
        }
        for (index, value) in expected.into_iter().enumerate() {
            let address = 0x400000 + index as u32 * 2;
            assert_eq!(bus.peek(address), value);
            assert_eq!(bus.peek(address | 0xff000000), value);
            assert_eq!(bus.peek(address + 1), None);
            for lanes in [ByteLanes::Upper, ByteLanes::Both] {
                let actual = bus.read_cycle(access(address, lanes), bus.cpu_clock).result;
                if let Some(value) = value {
                    let word = u16::from(value) << if lanes == ByteLanes::Upper { 8 } else { 0 };
                    assert_eq!(actual, Ok(word));
                } else {
                    assert!(actual.is_err());
                }
            }
            assert_eq!(
                bus.read_cycle(access(address + 1, ByteLanes::Lower), bus.cpu_clock)
                    .result,
                Err(BusError::UnsupportedAccess {
                    address: address + 1,
                    direction: Direction::Read,
                })
            );
        }
        for address in [0x400008, 0x400010, 0x400100, 0x40fffe, 0x410000] {
            assert!(
                bus.read_cycle(access(address, ByteLanes::Both), bus.cpu_clock)
                    .result
                    .is_err()
            );
            assert_eq!(bus.peek(address), None);
        }
        let before = bus.cartridge.clone();
        assert!(!before.protection.unwrap().contains(0x410000));
        bus.write_cycle(access(0x410000, ByteLanes::Both), 0, bus.cpu_clock)
            .result
            .unwrap();
        assert_eq!(bus.cartridge, before);
        assert!(
            bus.read_cycle(access(0x410000, ByteLanes::Both), bus.cpu_clock)
                .result
                .is_err()
        );
        assert_eq!(bus.peek(0x410000), None);
        assert_eq!(bus.peek(0x200), Some(0x60));
        assert_eq!(bus.peek(0x200001), Some(0xff));
        for change in 0..4 {
            let mut mismatch = rom(profile, &[0x60fe, 0x4e71]);
            match change {
                0 => mismatch[0x18e] ^= 1,
                1 => mismatch[0x300] ^= 1,
                2 => mismatch.extend([0, 0]),
                _ => {
                    mismatch.truncate(mismatch.len() - 2);
                }
            }
            assert!(Cartridge::new(mismatch).unwrap().protection.is_none());
        }
    }
    sparse_upper_pair_identity_and_bus_lanes();
    for profile in 0..3 {
        let cartridge = Cartridge::new(crate::cartridge::registers::tests::rom(profile)).unwrap();
        assert!(cartridge.registers.is_some());
        assert!(cartridge.protection.is_none());
    }
}

fn sparse_upper_pair_identity_and_bus_lanes() {
    let cartridge = Cartridge::new(rom(4, &[0x60fe, 0x4e71])).unwrap();
    assert!(cartridge.protection.is_some());
    assert!(cartridge.save.is_none());
    assert!(cartridge.registers.is_none());
    assert_eq!(
        cartridge.header.as_ref().unwrap().calculated_checksum,
        0x7037
    );
    let mut bus = Bus::new(cartridge, Region::NtscUs);
    for (address, value) in [(0x400000, 0x90), (0x401000, 0xd3)] {
        assert_eq!(bus.peek(address), Some(value));
        assert_eq!(bus.peek(address | 0xff000000), Some(value));
        assert_eq!(bus.peek(address + 1), None);
        assert_eq!(
            bus.read_cycle(access(address, ByteLanes::Upper), bus.cpu_clock)
                .result,
            Ok(u16::from(value) << 8)
        );
        for lanes in [ByteLanes::Lower, ByteLanes::Both] {
            let target = address + u32::from(lanes == ByteLanes::Lower);
            assert_eq!(
                bus.read_cycle(access(target, lanes), bus.cpu_clock).result,
                Err(BusError::UnsupportedAccess {
                    address: target,
                    direction: Direction::Read,
                })
            );
        }
    }
    for address in [
        0x400002, 0x400006, 0x400100, 0x400ffe, 0x401002, 0x40fffe, 0x410000,
    ] {
        assert_eq!(bus.peek(address), None);
        for lanes in [ByteLanes::Upper, ByteLanes::Lower, ByteLanes::Both] {
            assert!(
                bus.read_cycle(access(address, lanes), bus.cpu_clock)
                    .result
                    .is_err()
            );
        }
    }
    for address in [0x400000, 0x400002, 0x401000, 0x401002, 0x40fffe] {
        for lanes in [ByteLanes::Upper, ByteLanes::Lower, ByteLanes::Both] {
            let before = bus.cartridge.clone();
            assert_eq!(
                bus.write_cycle(access(address, lanes), 0x1234, bus.cpu_clock)
                    .result,
                Err(BusError::UnsupportedAccess {
                    address,
                    direction: Direction::Write,
                })
            );
            assert_eq!(bus.cartridge, before);
        }
    }
    for change in 0..8 {
        let mut bytes = rom(4, &[0x60fe, 0x4e71]);
        match change {
            0 => bytes[0x18e] ^= 1,
            1 => bytes[0x300] ^= 1,
            2 => bytes.extend([0, 0]),
            3 => bytes.truncate(bytes.len() - 2),
            4 => bytes[0x110] ^= 1,
            5 => bytes[0x180] ^= 1,
            6 => bytes[0x11f] ^= 1,
            _ => bytes[0x18d] ^= 1,
        }
        assert!(Cartridge::new(bytes).unwrap().protection.is_none());
    }
    assert!(super::FixedProtection::detect(0x100000, None).is_none());
}
