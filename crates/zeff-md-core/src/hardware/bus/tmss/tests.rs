use super::*;
use crate::{Emulator, LoadConfig};
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

fn image() -> Vec<u8> {
    let mut data = vec![0; 2048];
    data[..8].copy_from_slice(&[0, 0xff, 0xff, 0xfc, 0, 0, 2, 0]);
    data[0x200..0x204].copy_from_slice(&[0x60, 0xfe, 0x4e, 0x71]);
    data[0x7ff] = 0x5a;
    data
}

pub(super) fn contract() {
    let bios = image();
    let mut rom = bios.clone();
    rom[0x7ff] = 0xa5;
    let mut bus = Bus::new(Cartridge::new(rom.clone()).unwrap(), Region::NtscUs);
    bus.tmss = Some(crate::hardware::tmss::Tmss::new(bios.clone()).unwrap());
    assert_eq!(bus.peek(0xa10001), Some(0xa1));
    for address in [0x7ff, 0xfff, 0x1fffff, 0x3fffff] {
        assert_eq!(bus.peek(address), Some(0x5a));
    }
    let source =
        VdpDmaSource::from_cartridge(&bus.cartridge, &bus.ram).with_tmss(bus.tmss.as_ref());
    source.assert_word(0x3ffffe, 0x005a);
    bus.prefetch = Prefetch::Full(0x1234);
    assert!(
        bus.write(access(0xa14000, ByteLanes::Upper), 0x5300)
            .is_err()
    );
    assert_eq!(bus.prefetch, Prefetch::Full(0x1234));
    let before = bus.clone();
    for address in [0xc00000, 0xc00004, 0xc00008] {
        let port = access(address, ByteLanes::Both);
        let read = bus.read_cycle(port, bus.cpu_clock);
        assert_eq!(
            read.result,
            Err(BusError::UnsupportedAccess {
                address,
                direction: Direction::Read,
            })
        );
        assert_eq!(read.wait_clocks, 0);
        assert_eq!(bus, before);
        let write = bus.write_cycle(port, 0x8144, bus.cpu_clock);
        assert_eq!(
            write.result,
            Err(BusError::UnsupportedAccess {
                address,
                direction: Direction::Write,
            })
        );
        assert_eq!(write.wait_clocks, 0);
        assert_eq!(bus, before);
        assert_eq!(bus.prefetch, Prefetch::Full(0x1234));
    }
    for address in [0xa11200, 0xa11202, 0xa112fe] {
        assert!(
            bus.preflight_tmss(access(address, ByteLanes::Both), Some(0x100))
                .is_err()
        );
        assert!(bus.sound.reset_asserted);
    }
    for (address, word) in [(0xa14000, 0x5345), (0xa14002, 0x4741)] {
        bus.write(access(address, ByteLanes::Both), word).unwrap();
        assert_eq!(bus.read(access(address, ByteLanes::Both)), Ok(word));
    }
    assert!(bus.tmss.as_ref().unwrap().unlocked());
    assert!(
        bus.preflight_tmss(access(0xc00004, ByteLanes::Both), None)
            .is_ok()
    );
    bus.prefetch = Prefetch::Full(0x1234);
    assert_eq!(bus.read(access(0xa14100, ByteLanes::Both)), Ok(0x1234));
    bus.write(access(0xa14100, ByteLanes::Both), 1).unwrap();
    assert_eq!(bus.peek(0x7ff), Some(0xa5));
    bus.sound.reset_asserted = false;
    bus.prefetch = Prefetch::Full(0x5678);
    assert!(bus.write(access(0xa14101, ByteLanes::Lower), 0).is_err());
    assert_eq!(bus.prefetch, Prefetch::Full(0x5678));
    assert!(!bus.tmss.as_ref().unwrap().boot_mapped());

    let config = LoadConfig {
        region: Region::NtscUs,
        concurrent_sound: true,
    };
    let mut active = Emulator::new_with_tmss(rom.clone(), config, bios.clone()).unwrap();
    let state = active.encode_frontend_state().unwrap();
    assert_eq!(active.registers().pc, 0x200);
    let legacy = Emulator::new(rom.clone(), config)
        .unwrap()
        .encode_frontend_state()
        .unwrap();
    let mut plain = Emulator::new(rom.clone(), config).unwrap();
    assert!(plain.decode_frontend_state(&state).is_err());
    for invalid in [&state[..state.len() - 1], &legacy] {
        assert!(active.decode_frontend_state(invalid).is_err());
        assert_eq!(active.encode_frontend_state().unwrap(), state);
    }
    let mut other = bios;
    other[0x7fe] ^= 1;
    let mut wrong = Emulator::new_with_tmss(rom, config, other).unwrap();
    let before = wrong.encode_frontend_state().unwrap();
    assert!(wrong.decode_frontend_state(&state).is_err());
    assert_eq!(wrong.encode_frontend_state().unwrap(), before);
    assert!(active.reset_cpu().is_err());
    assert!(active.check_warm_reset().is_err());
    assert_eq!(active.encode_frontend_state().unwrap(), state);
    active.decode_frontend_state(&state).unwrap();
    assert_eq!(active.encode_frontend_state().unwrap(), state);
}
