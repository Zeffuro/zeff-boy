use super::*;
use zeff_m68k::bus::{AccessKind, BusAccess, ByteLanes, FunctionCode};

pub(crate) const PC: usize = 0x50000;

pub(crate) fn fixture(words: &[u16]) -> Vec<u8> {
    let mut rom = vec![0; ORIGINAL_BYTES];
    rom[..8].copy_from_slice(&[0, 0xff, 0xff, 0xfc, 0, 5, 0, 0]);
    rom[0x80..0x90].copy_from_slice(b"DOWNLOAD_RAM_TST");
    rom[0x100..0x104].copy_from_slice(b"SEGA");
    rom[0x1b0..0x1bc].fill(b' ');
    rom[0x1234] = 0x67;
    for (index, word) in words.iter().enumerate() {
        rom[PC + 2 * index..PC + 2 * index + 2].copy_from_slice(&word.to_be_bytes());
    }
    rom
}

pub(crate) fn access(address: u32, lanes: ByteLanes) -> BusAccess {
    BusAccess {
        logical_address: address,
        bus_address: address & !1,
        lanes,
        function_code: FunctionCode::SupervisorData,
        kind: AccessKind::Data,
    }
}

fn identity() {
    let mut rom = fixture(&[]);
    rom[0x80..0x90].fill(0);
    let mut header = CartridgeHeader::parse(&rom).unwrap();
    header.calculated_checksum = 0xf9d1;
    assert!(DownloadRam::detect(&rom, Some(&header), || IDENTITY).is_some());
    for index in 0..32 {
        let mut digest = IDENTITY;
        digest[index] ^= 1;
        assert!(DownloadRam::detect(&rom, Some(&header), || digest).is_none());
    }
    for index in 0x1b0..0x1bc {
        let mut bad = rom.clone();
        bad[index] ^= 1;
        assert!(
            DownloadRam::detect(&bad, Some(&header), || panic!("unmatched media hashed")).is_none()
        );
    }
    for (stored, calculated) in [(1, 0xf9d1), (0, 0xf9d0)] {
        let mut bad = header.clone();
        bad.checksum = stored;
        bad.calculated_checksum = calculated;
        assert!(
            DownloadRam::detect(&rom, Some(&bad), || panic!("unmatched checksum hashed")).is_none()
        );
    }
    for size in [0x200000, 0x2fffff, 0x300001, 0x400000] {
        let mut bad = rom.clone();
        bad.resize(size, 0);
        assert!(
            DownloadRam::detect(&bad, Some(&header), || panic!("unmatched size hashed")).is_none()
        );
    }
    assert!(Cartridge::new(rom).unwrap().download.is_none());
}

fn controls(template: &Cartridge) {
    for offset in 0..=255 {
        let values = if offset == 0xf1 {
            (0..=u8::MAX).collect::<Vec<_>>()
        } else {
            vec![0, 1, 0xfe, 0xff]
        };
        for value in values {
            let mut cart = template.clone();
            let address = 0xa13000 + offset;
            let lanes = if address & 1 == 0 {
                ByteLanes::Upper
            } else {
                ByteLanes::Lower
            };
            let data = if lanes == ByteLanes::Upper {
                u16::from(value) << 8
            } else {
                u16::from(value)
            };
            assert_eq!(
                cart.write_download(access(address, lanes), data),
                Some(true)
            );
            assert_eq!(
                cart.download.as_ref().unwrap().writable,
                offset == 0xf1 && value & 1 == 0
            );
        }
    }
    for value in [0, 1, 0x0100, 0x0101, 0xffff] {
        let mut cart = template.clone();
        assert_eq!(
            cart.write_download(access(0xa130f0, ByteLanes::Both), value),
            Some(true)
        );
        assert!(!cart.download.as_ref().unwrap().writable);
    }
}

fn memory_and_guards(template: &Cartridge) {
    let mut cart = template.clone();
    for address in [0, 7, 0x1234, 0x2fffff, 0x300000, 0x3fffff] {
        assert_eq!(
            cart.read(address),
            Some(if address >= ORIGINAL_BYTES as u32 {
                0xff
            } else {
                cart.bytes[address as usize]
            })
        );
        assert_eq!(cart.rom_source_offset(address), None);
        assert!(cart.mode_sensitive_read(address));
    }
    assert_eq!(cart.read(0x400000), None);
    for enabled in [false, true, false] {
        cart.write_download(access(0xa130f1, ByteLanes::Lower), u16::from(!enabled))
            .unwrap();
        for (address, lanes, data, high, low) in [
            (0x1234, ByteLanes::Upper, 0x5500, 0x55, 0),
            (0x300001, ByteLanes::Lower, 0x66, 0xff, 0x66),
            (0x3ffffe, ByteLanes::Both, 0x7788, 0x77, 0x88),
        ] {
            let before = [cart.read(address & !1), cart.read(address | 1)];
            assert_eq!(
                cart.write_download(access(address, lanes), data),
                Some(true)
            );
            assert_eq!(
                [cart.read(address & !1), cart.read(address | 1)],
                if enabled {
                    [Some(high), Some(low)]
                } else {
                    before
                }
            );
        }
    }
    assert_eq!(cart.bytes, template.bytes);
    assert_ne!(cart.download, template.download);
    crate::hardware::vdp::VdpDmaSource::assert_download_reads(&cart);
    for request in [
        BusAccess {
            logical_address: 0x1001234,
            ..access(0x1234, ByteLanes::Both)
        },
        BusAccess {
            bus_address: 0x1235,
            ..access(0x1234, ByteLanes::Both)
        },
        BusAccess {
            function_code: FunctionCode::CpuSpace,
            ..access(0xa13001, ByteLanes::Lower)
        },
        BusAccess {
            kind: AccessKind::Prefetch,
            ..access(0xa13001, ByteLanes::Lower)
        },
        access(0xa130f0, ByteLanes::Lower),
    ] {
        let before = cart.clone();
        assert_eq!(cart.write_download(request, 0), Some(false));
        assert_eq!(cart, before);
    }
    let mut stripped = template.clone();
    stripped.download = None;
    assert_ne!(stripped, *template);
    assert!(stripped.validate_download().is_err());
    let mut changed = template.clone();
    *Arc::make_mut(&mut changed.bytes).last_mut().unwrap() ^= 1;
    assert!(changed.validate_download().is_err());
    let mut conflict = template.clone();
    conflict.parallel_adapter = Some(parallel_adapter::ParallelAdapter::new(
        parallel_adapter::Model::First,
    ));
    assert!(conflict.validate_download().is_err());
}

pub(crate) fn contract() {
    identity();
    let rom = fixture(&[0x4e71, 0x4e71]);
    let template = Cartridge::new(rom.clone()).unwrap();
    template.validate_download().unwrap();
    assert!(template.download.is_some());
    controls(&template);
    memory_and_guards(&template);
    crate::emulator::download_tests::contract(rom);
}
