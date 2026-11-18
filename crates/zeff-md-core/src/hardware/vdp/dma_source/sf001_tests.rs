use super::*;
use crate::cartridge::sf001::Revision;

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn sf001_dma_tracks_switched_rom_mirrored_save_and_typed_disconnection() {
    for revision in [Revision::HiddenSave, Revision::UpperSave] {
        let mut cartridge =
            crate::cartridge::Cartridge::new(crate::cartridge::sf001::tests::rom(revision))
                .unwrap();
        for mode in [0, 0x80, 0x40, 0x80, 0] {
            let board = cartridge.sf001.as_mut().unwrap();
            board.control(0xe00, mode);
            for (offset, value) in [(0, 0x12), (1, 0x34), (0xfffe, 0xab), (0xffff, 0xcd)] {
                let address = if revision == Revision::HiddenSave {
                    0x3c0000
                } else {
                    0x400000
                };
                board.write(address + offset, value);
            }
            let source = VdpDmaSource::from_cartridge(&cartridge, &[0x56, 0x78]);
            for address in [
                0, 2, 0x3fffe, 0x40000, 0x3bfffe, 0x3c0000, 0x3ffffe, 0x400000, 0x40fffe, 0x410000,
            ] {
                let expected = cartridge
                    .read(address)
                    .zip(cartridge.read(address + 1))
                    .map(|(high, low)| u16::from_be_bytes([high, low]));
                match expected {
                    Some(value) => assert_eq!(source.read(address), Ok(value)),
                    None => assert_eq!(
                        source.read(address),
                        Err(VdpDmaError::UnsupportedSource(address))
                    ),
                }
            }
            assert_eq!(source.read(0xe00000), Ok(0x5678));
            if revision == Revision::HiddenSave && mode == 0x80 {
                for address in [0x3c0000, 0x3d0000, 0x3e0000, 0x3f0000] {
                    assert_eq!(source.read(address), Ok(0x1234));
                    assert_eq!(source.read(address + 0xfffe), Ok(0xabcd));
                }
            }
            if revision == Revision::UpperSave {
                assert_eq!(source.read(0x400000), Ok(0x1234));
                assert_eq!(source.read(0x40fffe), Ok(0xabcd));
            }
        }
    }
    let mut cartridge = crate::emulator::sf002_tests::cartridge();
    for control in [None, Some(0), Some(0x81)] {
        if let Some(value) = control {
            cartridge
                .sf002
                .as_mut()
                .unwrap()
                .control(crate::emulator::sf002_tests::access(0xe01), value);
        }
        let source = VdpDmaSource::from_cartridge(&cartridge, &[]);
        match control {
            None => assert_eq!(
                source.read(0x20796c),
                Err(VdpDmaError::UnsupportedSource(0x20796c))
            ),
            Some(0) => assert_eq!(source.read(0x20796c), Ok(0x8383)),
            Some(0x81) => {
                assert_eq!(source.read(0x20796c), Ok(0x1717));
                assert_eq!(source.read(0x3bfffe), Ok(0x1717));
            }
            _ => unreachable!(),
        }
        assert_eq!(source.read(0x796c), Ok(0x1717));
        for address in [0x3c4000, 0x3ffffe] {
            assert_eq!(
                source.read(address),
                Err(VdpDmaError::UnsupportedSource(address))
            );
        }
        for address in [
            0x1ffffe, 0x200000, 0x21fffe, 0x220000, 0x3bfffe, 0x3c0000, 0x3c3ffe, 0x3c4000,
            0x3ffffe,
        ] {
            let expected = cartridge
                .read(address)
                .zip(cartridge.read(address + 1))
                .map(|(high, low)| u16::from_be_bytes([high, low]));
            assert_eq!(
                source.read(address),
                expected.ok_or(VdpDmaError::UnsupportedSource(address))
            );
        }
    }
}
