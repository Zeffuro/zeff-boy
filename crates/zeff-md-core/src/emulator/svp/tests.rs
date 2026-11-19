use super::*;
use zeff_m68k::bus::{AccessKind, BusAccess, ByteLanes, Direction, FunctionCode};
use zeff_m68k::timing::ClockedBus;

fn images() -> (Vec<u8>, Vec<u16>) {
    let mut rom = vec![0; 0x1000];
    rom[..8].copy_from_slice(&[0, 0xff, 1, 0, 0, 0, 1, 0]);
    let mut main = vec![
        0x33fc, 0x1234, 0x00a1, 0x5000, 0x33fc, 0x7788, 0x00ff, 0x0200, 0x1039, 0x00a1, 0x5005,
        0x0200, 1, 0x67f4, 0x3239, 0x00a1, 0x5000, 0x3439, 0x0030, 0, 0x3639, 0x0030, 2,
    ];
    main.extend_from_slice(&[
        0x13fc, 0xab, 0x30, 4, 0x13fc, 0xcd, 0x30, 5, 0x3839, 0x30, 4,
    ]);
    for value in [
        0x8114, 0x8f02, 0x9302, 0x9400, 0x9500, 0x9600, 0x9718, 0x4400, 0x0080,
    ] {
        main.extend_from_slice(&[0x33fc, value, 0x00c0, 4]);
    }
    main.extend_from_slice(&[0x4e71, 0x60fe]);
    for (i, word) in main.into_iter().enumerate() {
        rom[0x100 + i * 2..0x102 + i * 2].copy_from_slice(&u16::to_be_bytes(word));
    }
    let dsp = [
        0x0840, 0, 0x0038, 0xa800, 2, 0x4d50, 0x402, 0x003b, 0x8800, 1, 0x0013, 0x0840, 0x20,
        0x08e0, 0, 0x08e0, 0x0818, 0x0080, 0x0081, 0x0081, 0x0840, 0, 0x00b1, 0x4c00, 0x416,
    ];
    for (i, word) in dsp.into_iter().enumerate() {
        rom[0x800 + i * 2..0x802 + i * 2].copy_from_slice(&u16::to_be_bytes(word));
    }
    let mut irom = vec![0; 1024];
    irom[8..10].copy_from_slice(&[0x0860, 0x400]);
    irom[0x3fc] = 0xfc08;
    (rom, irom)
}

fn fresh() -> Emulator {
    let (rom, irom) = images();
    Emulator::new_synthetic_svp(
        rom,
        LoadConfig {
            region: Region::NtscUs,
            concurrent_sound: false,
        },
        irom,
    )
    .unwrap()
}

fn access(address: u32, lanes: ByteLanes) -> BusAccess {
    BusAccess {
        logical_address: address | u32::from(lanes == ByteLanes::Lower),
        bus_address: address,
        lanes,
        kind: AccessKind::Data,
        function_code: FunctionCode::SupervisorData,
    }
}

fn restore(machine: &Emulator) {
    let bytes = machine.encode_state().unwrap();
    assert_eq!(&bytes[8..12], &1_u32.to_le_bytes());
    let mut copy = fresh();
    copy.decode_state(&bytes).unwrap();
    assert!(copy.encode_state().unwrap() == bytes);
    let expected = machine.clone();
    assert_eq!(copy.advance_synthetic_svp(false), Ok(Progress::Waiting));
    assert!(copy.encode_state().unwrap() == expected.encode_state().unwrap());
    let mut expected = expected;
    for _ in 0..4 {
        assert_eq!(
            copy.advance_synthetic_svp(true),
            expected.advance_synthetic_svp(true)
        );
        assert_eq!(
            copy.encode_state().unwrap(),
            expected.encode_state().unwrap()
        );
    }
}

fn rejection(machine: &mut Emulator, address: u32, lanes: ByteLanes, writing: bool) {
    let before = machine.clone();
    let start = machine.bus.cpu_clock;
    let result = if writing {
        machine
            .bus
            .write_cycle(access(address, lanes), 0xabcd, start)
            .result
    } else {
        machine
            .bus
            .read_cycle(access(address, lanes), start)
            .result
            .map(|_| ())
    };
    assert!(
        matches!(result,Err(BusError::Svp{direction,..}) if direction==if writing {Direction::Write} else {Direction::Read})
    );
    assert_eq!(*machine, before);
    let start = machine.bus.cpu_clock;
    let mut bus = ScheduledBus::new(
        &mut machine.bus,
        &mut machine.sound_cpu,
        &mut machine.sound_schedule,
    );
    let result = if writing {
        bus.write_cycle(access(address, lanes), 0xabcd, start)
            .result
    } else {
        bus.read_cycle(access(address, lanes), start)
            .result
            .map(|_| ())
    };
    assert!(matches!(result, Err(BusError::Svp { .. })));
    assert!(*machine == before, "scheduled rejection mutated machine");
}

impl Emulator {
    pub(crate) fn assert_synthetic_svp_transport() {
        serviced::contract();
        clocked::contract();
        boundary_control::contract();
        concurrent::contract();
        let mut machine = fresh();
        sound_boundaries(&machine);
        assert!(Bus::new_owned(machine.bus.cartridge.clone(), Region::NtscUs).is_err());
        for _ in 0..4 {
            restore(&machine);
            machine.advance_synthetic_svp(true).unwrap();
        }
        assert_eq!(machine.synthetic_svp().unwrap().registers().pc, 0x400);
        machine.step_instruction().unwrap();
        assert_eq!(machine.synthetic_svp().unwrap().peek_result(), 0x1234);
        let mut writes = false;
        for _ in 0..100 {
            restore(&machine);
            let progress = machine.advance_synthetic_svp(true).unwrap();
            if matches!(progress, Progress::Prepared { pc: 0x412, .. }) {
                rejection(&mut machine, 0x300004, ByteLanes::Both, true);
                rejection(&mut machine, 0xa15004, ByteLanes::Lower, false);
                machine.step_instruction().unwrap();
                writes = true;
            }
            if machine.synthetic_svp().unwrap().peek_status() & 1 != 0 {
                break;
            }
        }
        assert!(writes);
        assert_eq!(&machine.work_ram()[0x200..0x202], &[0x77, 0x88]);
        assert_eq!(machine.peek(0xa15005), Some(1));
        let before = machine.clone();
        assert_eq!(machine.peek(0xa15005), Some(1));
        assert_eq!(machine, before);
        for _ in 0..6 {
            machine.step_instruction().unwrap();
        }
        assert_eq!(&machine.registers().d[..4], &[1, 0x1235, 0x1235, 0x1235]);
        assert_eq!(machine.peek(0xa15005), Some(0));
        for _ in 0..3 {
            machine.step_instruction().unwrap();
        }
        assert_eq!(machine.registers().d[4], 0xabcd);
        let mut bus = machine.bus.clone();
        let start = bus.cpu_clock;
        assert_eq!(
            bus.read_cycle(access(0x300004, ByteLanes::Upper), start)
                .result
                .unwrap()
                & 0xff00,
            0xab00
        );
        let start = bus.cpu_clock;
        assert_eq!(
            bus.read_cycle(access(0x300004, ByteLanes::Lower), start)
                .result
                .unwrap()
                & 0xff,
            0xcd
        );
        for (address, lanes, writing) in [
            (0xa15006, ByteLanes::Both, true),
            (0xa15008, ByteLanes::Both, false),
            (0xa15004, ByteLanes::Upper, false),
            (0x390000, ByteLanes::Both, false),
            (0x100, ByteLanes::Both, true),
        ] {
            rejection(&mut machine, address, lanes, writing);
        }
        let before = machine.clone();
        assert!(machine.reset_cpu().is_err());
        assert_eq!(machine, before);
        assert!(machine.check_warm_reset().is_err());
        assert_eq!(machine, before);
        let bytes = machine.encode_state().unwrap();
        for malformed in [&bytes[..bytes.len() - 1], &bytes[..bytes.len() - 12]] {
            assert!(machine.decode_state(malformed).is_err());
            assert_eq!(machine, before);
        }
        let (rom, mut irom) = images();
        irom[0] ^= 1;
        let mut mismatch = Emulator::new_synthetic_svp(
            rom,
            LoadConfig {
                region: Region::NtscUs,
                concurrent_sound: false,
            },
            irom,
        )
        .unwrap();
        let saved = mismatch.encode_state().unwrap();
        assert!(mismatch.decode_state(&bytes).is_err());
        assert!(mismatch.encode_state().unwrap() == saved);
        for _ in 0..10 {
            machine.step_instruction().unwrap();
        }
        assert_eq!(
            &machine.vdp().memory().vram()[0x400..0x404],
            &[0x12, 0x35, 0x12, 0x35]
        );
        restore(&machine);
        machine.reset();
        assert_eq!(
            machine
                .synthetic_svp()
                .unwrap()
                .registers()
                .committed_instructions,
            0
        );
        assert_eq!(machine.synthetic_svp().unwrap().peek_dram(0), 0);
    }
}

#[path = "tests/control.rs"]
mod boundary_control;

fn sound_boundaries(machine: &Emulator) {
    use crate::hardware::sound::{CandidateBus, SoundMemory};
    use zeff_z80::Z80Bus;
    for (target, expected) in [
        (0x100, Some(0x33)),
        (0x1000, None),
        (0x300000, None),
        (0xa15004, None),
    ] {
        let mut memory = SoundMemory {
            bank: (target >> 15) as u16,
            ..Default::default()
        };
        let bus = CandidateBus::new(&mut memory, &machine.bus.cartridge);
        let value = bus.cpu_read((0x8000 | (target & 0x7fff)) as u16);
        if let Some(expected) = expected {
            assert_eq!(value, expected);
            assert!(bus.fault.get().is_none());
        } else {
            assert!(bus.fault.get().is_some());
        }
    }
}

#[path = "tests/concurrent.rs"]
mod concurrent;

#[path = "tests/service.rs"]
mod serviced;

#[path = "tests/clock.rs"]
mod clocked;
#[path = "tests/rom_history.rs"]
mod rom_history;
