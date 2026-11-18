use super::*;
use crate::cartridge::sf001::{Mapping, Revision};
use zeff_m68k::bus::FunctionCode;

fn access(address: u32, lanes: ByteLanes) -> BusAccess {
    BusAccess {
        logical_address: address,
        bus_address: address & !1,
        lanes,
        kind: AccessKind::Data,
        function_code: FunctionCode::SupervisorData,
    }
}

fn bus(revision: Revision) -> Bus {
    Bus::new(
        Cartridge::new(crate::cartridge::sf001::tests::rom(revision)).unwrap(),
        Region::NtscUs,
    )
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn sf001_main_all_control_words_lanes_windows_modes_and_lock() {
    for revision in [Revision::HiddenSave, Revision::UpperSave] {
        let mut bus = bus(revision);
        for seed in [0, 0x80, 0x40] {
            for window in 0..16 {
                for lanes in [ByteLanes::Upper, ByteLanes::Lower, ByteLanes::Both] {
                    for value in 0..=255_u16 {
                        let board = bus.cartridge.sf001.as_mut().unwrap();
                        board.power_on();
                        assert!(board.control(0xe00, seed));
                        let address =
                            window * 0x1000 + 0xe00 + u32::from(lanes == ByteLanes::Lower);
                        let encoded = if lanes == ByteLanes::Upper {
                            value << 8
                        } else {
                            0x6000 | value
                        };
                        bus.write(access(address, lanes), encoded).unwrap();
                        let board = bus.cartridge.sf001.as_ref().unwrap();
                        let mapping = if value & 0x40 != 0 {
                            Mapping::Disconnected
                        } else if value & 0x80 != 0 {
                            Mapping::Switched
                        } else {
                            Mapping::Linear
                        };
                        assert_eq!(board.mapping(), mapping);
                        assert_eq!(board.locked(), value & 0x20 != 0);
                        if value & 0x20 != 0 {
                            bus.write(access(address, lanes), !encoded).unwrap();
                            let board = bus.cartridge.sf001.as_ref().unwrap();
                            assert_eq!(board.mapping(), mapping);
                            assert!(board.locked());
                        }
                    }
                }
            }
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn sf001_main_mapping_both_save_lanes_mirrors_peek_and_disconnected_guards() {
    for revision in [Revision::HiddenSave, Revision::UpperSave] {
        let mut bus = bus(revision);
        let bytes = bus.cartridge.bytes.clone();
        assert_eq!(bus.peek(0x3bffff), Some(bytes[0x3bffff]));
        assert_eq!(
            bus.read(access(0x400000, ByteLanes::Both)).is_ok(),
            revision == Revision::UpperSave
        );
        bus.write(access(0xe00, ByteLanes::Both), 0x0080).unwrap();
        for address in [0, 2, 0xfffe, 0x10000, 0x3fffe, 0x40000, 0x3bfffe] {
            let offset = if address < 0x40000 {
                address + 0x380000
            } else {
                address
            };
            assert_eq!(
                bus.read(access(address, ByteLanes::Both)),
                Ok(u16::from_be_bytes([
                    bytes[offset as usize],
                    bytes[offset as usize + 1]
                ]))
            );
        }
        let save_start = if revision == Revision::HiddenSave {
            0x3c0000
        } else {
            0x400000
        };
        for offset in [0, 2, 0x7ffe, 0xfffe] {
            let address = save_start + offset;
            bus.write(access(address, ByteLanes::Both), 0xabcd).unwrap();
            bus.write(access(address, ByteLanes::Upper), 0x1200)
                .unwrap();
            bus.write(access(address + 1, ByteLanes::Lower), 0x34)
                .unwrap();
            assert_eq!(bus.read(access(address, ByteLanes::Upper)), Ok(0x1200));
            assert_eq!(bus.read(access(address + 1, ByteLanes::Lower)), Ok(0x34));
            assert_eq!(bus.read(access(address, ByteLanes::Both)), Ok(0x1234));
            let mirrors = if revision == Revision::HiddenSave {
                4
            } else {
                1
            };
            for bank in 0..mirrors {
                assert_eq!(bus.peek(address + bank * 0x10000), Some(0x12));
                assert_eq!(bus.peek(address + bank * 0x10000 + 1), Some(0x34));
            }
        }
        let snapshot = bus.cartridge.clone();
        for _ in 0..4 {
            assert_eq!(bus.peek(save_start), Some(0x12));
        }
        assert_eq!(bus.cartridge, snapshot);
        for mode in [0x40, 0x80, 0] {
            bus.write(access(0xe00, ByteLanes::Both), mode).unwrap();
            for lanes in [ByteLanes::Upper, ByteLanes::Lower, ByteLanes::Both] {
                assert_eq!(
                    bus.read(access(0x400000, lanes)).is_ok(),
                    revision == Revision::UpperSave
                );
                assert_eq!(bus.read(access(0, lanes)).is_err(), mode == 0x40);
                assert!(bus.read(access(0x410000, lanes)).is_err());
            }
        }
        bus.write(access(0xa130f0, ByteLanes::Both), 0xffff)
            .unwrap();
        assert_eq!(
            bus.cartridge.sf001.as_ref().unwrap().mapping(),
            Mapping::Linear
        );
        assert_eq!(bus.cartridge.bytes, bytes);
    }
    let mut bytes = crate::cartridge::sf001::tests::rom(Revision::UpperSave);
    bytes[0x18e] ^= 1;
    let mut ordinary = Bus::new(Cartridge::new(bytes).unwrap(), Region::NtscUs);
    assert!(ordinary.cartridge.sf001.is_none());
    for lanes in [ByteLanes::Upper, ByteLanes::Lower, ByteLanes::Both] {
        assert!(ordinary.read(access(0x400000, lanes)).is_err());
        let before = ordinary.clone();
        assert_eq!(ordinary.write(access(0x400000, lanes), 0x55), Ok(()));
        assert_eq!(ordinary, before);
    }
}
