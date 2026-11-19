use super::*;
use crate::cartridge::{Cartridge, ram};
use crate::hardware::bus::scheduled::ScheduledBus;
use zeff_m68k::bus::{AccessKind, BusAccess, ByteLanes, FunctionCode};
use zeff_m68k::timing::ClockedBus;

fn access(address: u32, lanes: ByteLanes) -> BusAccess {
    BusAccess {
        logical_address: address,
        bus_address: address & !1,
        function_code: FunctionCode::SupervisorData,
        kind: AccessKind::Data,
        lanes,
    }
}

fn control(cart: &mut Cartridge, lanes: ByteLanes, value: u8) {
    let address = ram::SAVE_CONTROL + u32::from(lanes == ByteLanes::Lower);
    let data = if lanes == ByteLanes::Upper {
        u16::from(value) << 8 | 0x5a
    } else {
        0xa500 | u16::from(value)
    };
    assert_eq!(
        cart.write_save_latch(access(address, lanes), data, 0, 1),
        Some(true)
    );
}

fn bus_guards(rom: &[u8]) {
    let request = access(ram::SAVE_CONTROL, ByteLanes::Upper);
    let mut machine = Emulator::new(rom.to_vec(), LoadConfig::default()).unwrap();
    let before = machine.bus.clone();
    let clock = machine.bus.cpu_clock;
    let result = machine.bus.read_cycle(request, clock);
    assert!(result.result.is_err());
    assert_eq!(result.wait_clocks, 0);
    assert_eq!(machine.bus, before);
    let bad = BusAccess {
        kind: AccessKind::Prefetch,
        function_code: FunctionCode::SupervisorProgram,
        ..access(0x200000, ByteLanes::Both)
    };
    let result = machine.bus.read_cycle(bad, clock);
    assert!(result.result.is_err());
    assert_eq!(machine.bus, before);
    let alias = access(ram::SAVE_CONTROL + 2, ByteLanes::Both);
    let result = machine.bus.write_cycle(alias, 0, clock);
    assert!(result.result.is_err());
    assert_eq!(result.wait_clocks, 0);
    assert_eq!(machine.bus, before);
    let request = access(ram::SAVE_CONTROL, ByteLanes::Upper);
    let clock = machine.bus.cpu_clock;
    assert!(matches!(
        machine.bus.begin_rmw(request, clock).unwrap(),
        zeff_m68k::bus::BusResponse::Unsupported
    ));
    assert_eq!(machine.bus, before);
    let before = machine.clone();
    for (request, value) in [(request, None), (bad, None), (alias, Some(0))] {
        let mut bus = ScheduledBus::new(
            &mut machine.bus,
            &mut machine.sound_cpu,
            &mut machine.sound_schedule,
        );
        let (failed, waits) = if let Some(value) = value {
            let result = bus.write_cycle(request, value, clock);
            (result.result.is_err(), result.wait_clocks)
        } else {
            let result = bus.read_cycle(request, clock);
            (result.result.is_err(), result.wait_clocks)
        };
        assert!(failed);
        assert_eq!(waits, 0);
        assert_eq!(machine, before);
    }
}

fn state_modes(rom: &[u8]) {
    for value in [0, 1, 3] {
        let mut source = Emulator::new(rom.to_vec(), LoadConfig::default()).unwrap();
        control(&mut source.bus.cartridge, ByteLanes::Both, value);
        source.bus.cartridge.save.as_mut().unwrap().bytes[17] = 0x6d;
        let raw = source.encode_state().unwrap();
        let front = source.encode_frontend_state().unwrap();
        for frontend in [false, true] {
            let mut target = Emulator::new(rom.to_vec(), LoadConfig::default()).unwrap();
            if frontend {
                target.decode_frontend_state(&front).unwrap();
            } else {
                target.decode_state(&raw).unwrap();
            }
            assert_eq!(target.encode_state().unwrap(), raw);
            if frontend {
                assert_eq!(target, source);
            }
            target.bus.cartridge.save_latch = None;
            let before = target.clone();
            let result = if frontend {
                target.decode_frontend_state(&front)
            } else {
                target.decode_state(&raw)
            };
            assert!(result.is_err());
            assert_eq!(target, before);
        }
    }
}

fn cpu_futures(mut rom: Vec<u8>) {
    let program = [
        0x13fc_u16, 1, 0x00a1, 0x30f0, 0x41f9, 0x0020, 3, 0x705a, 0x7a07, 0x1080, 0x5448, 0x51cd,
        0xfffa, 0x13fc, 0, 0x00a1, 0x30f0, 0x1039, 0x0020, 3, 0x13fc, 1, 0x00a1, 0x30f0, 0x1239,
        0x0020, 3, 0x4e71, 0x4e71,
    ];
    for (index, word) in program.iter().enumerate() {
        rom[0x200 + 2 * index..0x202 + 2 * index].copy_from_slice(&word.to_be_bytes());
    }
    let mut source = Emulator::new(rom.clone(), LoadConfig::default()).unwrap();
    for _ in 0..29 {
        source.step_instruction().unwrap();
    }
    assert!(!source.bus.cartridge.save.as_ref().unwrap().enabled);
    assert_eq!(
        &source.bus.cartridge.save.as_ref().unwrap().bytes[1..9],
        &[0x5a; 8]
    );
    let raw = source.encode_state().unwrap();
    let front = source.encode_frontend_state().unwrap();
    let mut cloned = source.clone();
    let mut restored = Emulator::new(rom.clone(), LoadConfig::default()).unwrap();
    restored.decode_state(&raw).unwrap();
    let mut frontend = Emulator::new(rom, LoadConfig::default()).unwrap();
    frontend.decode_frontend_state(&front).unwrap();
    for current in [&mut source, &mut cloned, &mut restored, &mut frontend] {
        for _ in 0..4 {
            current.step_instruction().unwrap();
        }
        assert_eq!(current.cpu.registers.d[0] as u8, 0xfc);
        assert_eq!(current.cpu.registers.d[1] as u8, 0x5a);
    }
    let endpoint = source.encode_state().unwrap();
    for current in [&cloned, &restored, &frontend] {
        assert_eq!(current.encode_state().unwrap(), endpoint);
    }
    control(&mut source.bus.cartridge, ByteLanes::Upper, 3);
    let battery = source.bus.cartridge.save.as_ref().unwrap().bytes.clone();
    source.reset();
    let save = source.bus.cartridge.save.as_ref().unwrap();
    assert!(save.enabled && !save.write_protected);
    assert_eq!(save.bytes, battery);
}

pub(crate) fn contract(rom: Vec<u8>) {
    bus_guards(&rom);
    state_modes(&rom);
    cpu_futures(rom);
}
