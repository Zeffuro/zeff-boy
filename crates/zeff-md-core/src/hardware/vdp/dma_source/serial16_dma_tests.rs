use super::*;
use crate::cartridge::Cartridge;

fn line(cartridge: &mut Cartridge, data: bool, clock: bool) {
    let save = cartridge.save.as_mut().unwrap();
    assert!(save.serial_write(u16::from(data), false, true, 0, 53_693_175));
    assert!(save.serial_write(u16::from(clock) << 8, true, false, 0, 53_693_175));
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn banked_serial_dma_rejects_held_ack_without_mutation_and_reads_rom() {
    check_profile(b"GM T-081586-00", 0xae, false);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn cached_serial_dma_rejects_held_ack_without_mutation_and_reads_rom() {
    check_profile(b"GM T-81576 -00", 0xa0, true);
    check_profile(b"GM T-81476 -00", 0xa0, false);
}

fn check_profile(product: &[u8; 14], control: u8, college: bool) {
    let mut bytes = vec![0x69; 0x400000];
    bytes[..8].copy_from_slice(&[0, 0xff, 0xff, 0xfc, 0, 0, 2, 0]);
    bytes[0x180..0x18e].copy_from_slice(product);
    bytes[0x1b0..0x1bc].copy_from_slice(b"RA\xe8\x40\x00\x20\x00\x00\x00\x20\x00\x01");
    if college {
        bytes[0x1b7] = 1;
    }
    let mut cartridge = Cartridge::new(bytes).unwrap();
    assert!(
        cartridge
            .save
            .as_mut()
            .unwrap()
            .serial_write(0, true, true, 0, 53_693_175)
    );
    line(&mut cartridge, false, true);
    line(&mut cartridge, false, false);
    for bit in (0..8).rev() {
        let data = (control >> bit) & 1 != 0;
        line(&mut cartridge, data, false);
        line(&mut cartridge, data, true);
        line(&mut cartridge, data, false);
    }
    line(&mut cartridge, true, false);
    line(&mut cartridge, true, true);
    assert_eq!(cartridge.read(0x200001), Some(0));
    for rom_visible in [false, true, false] {
        assert!(cartridge.save.as_mut().unwrap().serial_write(
            u16::from(rom_visible),
            true,
            true,
            0,
            53_693_175
        ));
        let before = cartridge.clone();
        let source = VdpDmaSource::from_cartridge(&cartridge, &[0; 4]);
        for address in [0x200000, 0x2ffffe] {
            if rom_visible {
                assert_eq!(source.read(address), Ok(0x6969));
            } else {
                assert_eq!(
                    source.read(address),
                    Err(VdpDmaError::UnsupportedSource(address))
                );
                assert_eq!(cartridge.read(address), None);
            }
        }
        for address in [0x1ffffe, 0x300000, 0x3ffffe] {
            assert_eq!(source.read(address), Ok(0x6969));
        }
        assert_eq!(cartridge, before);
    }
    assert_eq!(cartridge.read(0x200001), Some(0));
    use crate::hardware::bus::{Bus, BusError};
    use zeff_m68k::bus::{AccessKind, BusAccess, ByteLanes, Direction, FunctionCode};
    use zeff_m68k::timing::ClockedBus;
    for lanes in [ByteLanes::Upper, ByteLanes::Lower, ByteLanes::Both] {
        let mut bus = Bus::new(cartridge.clone(), crate::Region::NtscUs);
        let before = bus.cartridge.clone();
        let access = BusAccess {
            logical_address: if lanes == ByteLanes::Lower {
                0xa130f1
            } else {
                0xa130f0
            },
            bus_address: 0xa130f0,
            function_code: FunctionCode::SupervisorData,
            kind: AccessKind::Data,
            lanes,
        };
        let clock = bus.cpu_clock;
        assert_eq!(
            bus.write_cycle(access, 1, clock).result,
            Err(BusError::UnsupportedAccess {
                address: 0xa130f0,
                direction: Direction::Write
            })
        );
        assert_eq!(bus.cartridge, before);
    }
}
