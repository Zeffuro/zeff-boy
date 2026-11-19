use super::*;
use crate::cartridge::parallel_adapter::{Model, ParallelAdapter};
use zeff_emu_common::save_state::{StateReader, StateWriter};
use zeff_m68k::bus::{AccessKind, BusAccess, ByteLanes, FunctionCode};
use zeff_m68k::timing::{ClockedBus, CpuClocks, CycleResult};

fn access(address: u32, lanes: ByteLanes) -> BusAccess {
    BusAccess {
        logical_address: address | u32::from(lanes == ByteLanes::Lower),
        bus_address: address,
        function_code: FunctionCode::SupervisorData,
        kind: AccessKind::Data,
        lanes,
    }
}

fn fixture(words: &[u16], stack: u32, concurrent_sound: bool) -> Emulator {
    let mut rom = vec![0; 0x200];
    rom[..4].copy_from_slice(&stack.to_be_bytes());
    rom[4..8].copy_from_slice(&0x100_u32.to_be_bytes());
    for (index, word) in words.iter().enumerate() {
        rom[0x100 + index * 2..0x102 + index * 2].copy_from_slice(&word.to_be_bytes());
    }
    Emulator::new(
        rom,
        LoadConfig {
            region: Region::NtscUs,
            concurrent_sound,
        },
    )
    .unwrap()
}

fn machine_at(base: u32, concurrent_sound: bool) -> Emulator {
    fixture(
        &[
            0x2039,
            (base >> 16) as u16,
            0x0000,
            0x6100,
            0x000c,
            0x4eb9,
            (base >> 16) as u16,
            0x0020,
            0x4e71,
            0x4e71,
            0x7007,
            0x4e75,
        ],
        base + 0x5dc,
        concurrent_sound,
    )
}

fn machine(concurrent_sound: bool) -> Emulator {
    machine_at(0x420000, concurrent_sound)
}

fn discarded(
    machine: &mut Emulator,
    concurrent: bool,
    transfer: BusAccess,
    start: CpuClocks,
) -> CycleResult<(), BusError> {
    if concurrent {
        ScheduledBus::new(
            &mut machine.bus,
            &mut machine.sound_cpu,
            &mut machine.sound_schedule,
        )
        .read_discarded_cycle(transfer, start)
    } else {
        machine.bus.read_discarded_cycle(transfer, start)
    }
}

fn clear_machine_at(base: u32, concurrent_sound: bool) -> Emulator {
    fixture(
        &[
            0x3c3c,
            0x0001,
            0x267c,
            (base >> 16) as u16,
            0xfffc,
            0x429b,
            0x51ce,
            0xfffc,
            0x4e71,
            0x4e71,
            0x4e71,
        ],
        base + 0x5dc,
        concurrent_sound,
    )
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn parallel_adapter_lanes_stack_codec_and_admission() {
    for (length, text, model) in [
        (
            0x20000,
            "79711fa55e8a4ea66201cfa10e01e4e222380821bd10b03c660a1287ce5c64c3",
            Model::First,
        ),
        (
            0x10000,
            "bf122867894f95789679df4923c807c886d6095e34a030d2493e19c9030d164b",
            Model::First,
        ),
        (
            0x10000,
            "52e60077497ecd792a0ce088831207ca752254bffc1499ed0949a02ab08375be",
            Model::Second,
        ),
    ] {
        let mut digest = [0; 32];
        for (index, value) in digest.iter_mut().enumerate() {
            *value = u8::from_str_radix(&text[index * 2..index * 2 + 2], 16).unwrap();
        }
        assert_eq!(
            ParallelAdapter::matches_identity(length, digest),
            Some(model)
        );
        assert_eq!(ParallelAdapter::matches_identity(length + 1, digest), None);
        digest[17] ^= 1;
        assert_eq!(ParallelAdapter::matches_identity(length, digest), None);
    }
    assert_eq!(ParallelAdapter::detect(&vec![0; 0x10000]), None);
    let mut board = ParallelAdapter::new(Model::First);
    for address in [0x420000, 0x42fffe] {
        assert_eq!(
            board.write(access(address, ByteLanes::Both), 0xabcd),
            Some(true)
        );
        let before = board.clone();
        assert_eq!(
            board.write(access(address, ByteLanes::Lower), 0x0073),
            Some(false)
        );
        assert_eq!(board, before);
        assert_eq!(
            board.write(access(address, ByteLanes::Upper), 0x3100),
            Some(true)
        );
        assert_eq!(board.read(address), Some(0x31));
        assert_eq!(board.read(address + 1), Some(0x31));
        assert_eq!(before.read(address), Some(0xab));
        assert_eq!(before.read(address + 1), Some(0xcd));
    }
    for address in [0x41fffe, 0x430000] {
        assert_eq!(board.read(address), None);
        assert_eq!(board.write(access(address, ByteLanes::Both), 0x1234), None);
    }
    let before = board.clone();
    assert_eq!(
        board.write(access(0x100018, ByteLanes::Both), 0xffff),
        Some(false)
    );
    assert_eq!(board, before);
    let mut writer = StateWriter::new();
    board.write_state(&mut writer);
    let bytes = writer.into_bytes();
    let mut copy = ParallelAdapter::new(Model::First);
    let mut reader = StateReader::new(&bytes);
    copy.read_state(&mut reader).unwrap();
    assert_eq!(copy, board);
    assert!(reader.is_exhausted());
    let before = copy.clone();
    assert!(
        copy.read_state(&mut StateReader::new(&bytes[..bytes.len() - 1]))
            .is_err()
    );
    assert_eq!(copy, before);

    for (model, base, concurrent) in [
        (Model::First, 0x420000, false),
        (Model::First, 0x420000, true),
        (Model::Second, 0x600000, false),
        (Model::Second, 0x600000, true),
    ] {
        let mut e = machine_at(base, concurrent);
        let current = e.encode_state().unwrap();
        e.bus.cartridge.parallel_adapter = Some(ParallelAdapter::new(model));
        let before = e.clone();
        assert!(e.encode_state().is_err());
        assert!(e.decode_state(&current).is_err());
        assert_eq!(e, before);
        assert!(e.reset_cpu().is_err());
        assert_eq!(e, before);
        assert_eq!(
            e.check_warm_reset(),
            Err(WarmResetRejection::CartridgeOwner)
        );
        assert_eq!(e, before);
        for address in [base, 0x100000] {
            let before = e.clone();
            let start = e.bus.cpu_clock;
            let result = if concurrent {
                ScheduledBus::new(&mut e.bus, &mut e.sound_cpu, &mut e.sound_schedule)
                    .write_cycle(access(address, ByteLanes::Lower), 0x73, start)
                    .result
            } else {
                e.bus
                    .write_cycle(access(address, ByteLanes::Lower), 0x73, start)
                    .result
            };
            assert!(result.is_err());
            assert_eq!(e, before);
        }
        let board = e.bus.cartridge.parallel_adapter.as_mut().unwrap();
        assert_eq!(
            board.write(access(base, ByteLanes::Both), 0xabcd),
            Some(true)
        );
        assert_eq!(
            board.write(access(base + 2, ByteLanes::Both), 0x1234),
            Some(true)
        );
        assert_eq!(
            board.write(access(base + 0x20, ByteLanes::Both), 0x7009),
            Some(true)
        );
        assert_eq!(
            board.write(access(base + 0x22, ByteLanes::Both), 0x4e75),
            Some(true)
        );
        e.step_instruction().unwrap();
        assert_eq!(e.registers().d[0], 0xabcd1234);
        for _ in 0..6 {
            e.step_instruction().unwrap();
        }
        assert_eq!(e.registers().d[0], 9);
        assert_eq!(e.registers().pc, 0x110);
        assert_eq!(e.registers().ssp, base + 0x5dc);
        assert!(!e.bus.reset_output);
    }
    for (model, base, concurrent) in [
        (Model::First, 0x420000, false),
        (Model::First, 0x420000, true),
        (Model::Second, 0x600000, false),
        (Model::Second, 0x600000, true),
    ] {
        let mut board = ParallelAdapter::new(model);
        for (address, value) in [
            (base, 0x7768),
            (base + 2, 0x622e),
            (base + 0xfffc, 0x1122),
            (base + 0xfffe, 0x3344),
        ] {
            assert_eq!(
                board.write(access(address, ByteLanes::Both), value),
                Some(true)
            );
        }
        let reference = board.clone();
        let mut e = clear_machine_at(base, concurrent);
        e.bus.cartridge.parallel_adapter = Some(board);
        e.step_instruction().unwrap();
        e.step_instruction().unwrap();
        let mut elapsed = Vec::new();
        for expected_address in [base + 0x10000, base + 0x10004] {
            let clock = e.bus.cpu_clock;
            let master = e.bus.now.get();
            e.step_instruction().unwrap();
            elapsed.push((e.bus.cpu_clock.0 - clock.0, e.bus.now.get() - master));
            assert_eq!(e.registers().a[3], expected_address);
            assert_eq!(e.registers().pc, 0x10c);
            e.step_instruction().unwrap();
        }
        assert_eq!(elapsed[0], elapsed[1]);
        assert_eq!(elapsed[0], (20, 140));
        assert_eq!(e.registers().pc, 0x110);
        assert_eq!(e.registers().d[6] & 0xffff, 0xffff);
        let board = e.bus.cartridge.parallel_adapter.as_ref().unwrap();
        for (offset, value) in [0x77, 0x68, 0x62, 0x2e].into_iter().enumerate() {
            assert_eq!(board.read(base + offset as u32), Some(value));
        }
        for address in base + 0xfffc..=base + 0xffff {
            assert_eq!(board.read(address), Some(0));
        }
        assert_eq!(reference.read(base + 0xfffc), Some(0x11));
        assert_eq!(reference.read(base + 0xffff), Some(0x44));
        assert_eq!(board.read(base + 0x10000), None);
        assert_eq!(board.read(base + 0x10002), None);
        assert_eq!(board.read(base), reference.read(base));

        for (address, code) in [
            (base + 0x10002, FunctionCode::UserData),
            (base + 0x1fffe, FunctionCode::SupervisorData),
        ] {
            let mut check = e.clone();
            if address == base + 0x10002 {
                check.bus.prefetch = crate::hardware::bus::Prefetch::Empty;
            }
            let mut transfer = access(address, ByteLanes::Both);
            transfer.function_code = code;
            let clock = check.bus.cpu_clock;
            let master = check.bus.now.get();
            let before = check.bus.cartridge.parallel_adapter.clone();
            let cycle = discarded(&mut check, concurrent, transfer, clock);
            assert_eq!(cycle.result, Ok(()));
            assert_eq!(cycle.wait_clocks, 0);
            assert_eq!(check.bus.cpu_clock.0 - clock.0, 4);
            assert_eq!(check.bus.now.get() - master, 28);
            assert_eq!(check.bus.prefetch, crate::hardware::bus::Prefetch::Empty);
            assert_eq!(check.bus.cartridge.parallel_adapter, before);
            let clock = check.bus.cpu_clock;
            let cycle = if concurrent {
                ScheduledBus::new(
                    &mut check.bus,
                    &mut check.sound_cpu,
                    &mut check.sound_schedule,
                )
                .read_cycle(transfer, clock)
            } else {
                check.bus.read_cycle(transfer, clock)
            };
            assert!(cycle.result.is_err());
        }
        let transfer = access(base + 0x10000, ByteLanes::Both);
        for current in [
            access(base + 0x10000, ByteLanes::Upper),
            access(base + 0x20000, ByteLanes::Both),
            access(
                if model == Model::First {
                    0x610000
                } else {
                    0x430000
                },
                ByteLanes::Both,
            ),
        ] {
            let mut check = e.clone();
            let clock = check.bus.cpu_clock;
            let before = check.bus.cartridge.parallel_adapter.clone();
            assert!(
                discarded(&mut check, concurrent, current, clock)
                    .result
                    .is_err()
            );
            assert_eq!(check.bus.cartridge.parallel_adapter, before);
        }
        let mut ordinary = clear_machine_at(base, concurrent);
        let clock = ordinary.bus.cpu_clock;
        assert!(
            discarded(&mut ordinary, concurrent, transfer, clock)
                .result
                .is_err()
        );
    }

    let mut second = ParallelAdapter::new(Model::Second);
    assert_ne!(second, ParallelAdapter::new(Model::First));
    for address in [0x600000, 0x60fffe] {
        assert_eq!(
            second.write(access(address, ByteLanes::Both), 0x1234),
            Some(true)
        );
        assert_eq!(
            second.write(access(address, ByteLanes::Upper), 0x3100),
            Some(true)
        );
        let before = second.clone();
        assert_eq!(
            second.write(access(address, ByteLanes::Lower), 0x0073),
            Some(false)
        );
        assert_eq!(second, before);
        assert_eq!(second.read(address), Some(0x31));
        assert_eq!(second.read(address + 1), Some(0x31));
    }
    for address in [0x420000, 0x42ffff, 0x5ffffe, 0x610000] {
        assert_eq!(second.read(address), None);
    }
    let mut first_writer = StateWriter::new();
    board.write_state(&mut first_writer);
    let mut second_writer = StateWriter::new();
    second.write_state(&mut second_writer);
    let second_bytes = second_writer.into_bytes();
    assert_eq!(first_writer.into_bytes(), second_bytes);
    let mut second_copy = ParallelAdapter::new(Model::Second);
    second_copy
        .read_state(&mut StateReader::new(&second_bytes))
        .unwrap();
    assert_eq!(second_copy, second);
    second_copy.power_on();
    assert_eq!(second_copy.model(), Model::Second);
    assert_eq!(second_copy.read(0x600000), Some(0));
    assert_eq!(second.read(0x600000), Some(0x31));

    for concurrent in [false, true] {
        let mut e = fixture(
            &[
                0x33fc, 0x0000, 0x0010, 0x0038, 0x13fc, 0x005a, 0x0060, 0x00e2, 0x2039, 0x0060,
                0x00de, 0x4e71, 0x4e71,
            ],
            0x6005dc,
            concurrent,
        );
        let mut board = ParallelAdapter::new(Model::Second);
        board
            .write(access(0x6000de, ByteLanes::Both), 0x4450)
            .unwrap();
        board
            .write(access(0x6000e0, ByteLanes::Both), 0x422e)
            .unwrap();
        e.bus.cartridge.parallel_adapter = Some(board);
        let before_ram = e.bus.cartridge.parallel_adapter.clone();
        let start = (e.bus.cpu_clock.0, e.bus.now.get());
        e.step_instruction().unwrap();
        assert_eq!(
            (e.bus.cpu_clock.0 - start.0, e.bus.now.get() - start.1),
            (20, 140)
        );
        assert_eq!(e.bus.cartridge.parallel_adapter, before_ram);
        e.step_instruction().unwrap();
        e.step_instruction().unwrap();
        assert_eq!(e.registers().d[0], 0x4450422e);
        let board = e.bus.cartridge.parallel_adapter.as_ref().unwrap();
        assert_eq!(board.read(0x6000e2), Some(0x5a));
        assert_eq!(board.read(0x6000e3), Some(0x5a));
        assert!(!board.contains(0x420000));
        assert!(e.encode_state().is_err());
        let transfer = access(0x100038, ByteLanes::Both);
        let mut rejected = vec![
            (transfer, 1),
            (access(0x100078, ByteLanes::Both), 0xffff),
            (access(0x100138, ByteLanes::Both), 0),
            (access(0x100038, ByteLanes::Upper), 0),
            (access(0x100038, ByteLanes::Lower), 0),
        ];
        for (kind, code, logical) in [
            (AccessKind::Stack, FunctionCode::SupervisorData, 0x100038),
            (AccessKind::Data, FunctionCode::SupervisorProgram, 0x100038),
            (AccessKind::Data, FunctionCode::SupervisorData, 0x10003a),
        ] {
            let mut current = transfer;
            current.kind = kind;
            current.function_code = code;
            current.logical_address = logical;
            rejected.push((current, 0));
        }
        for (current, value) in rejected {
            let mut direct = e.bus.cartridge.parallel_adapter.clone().unwrap();
            let before = direct.clone();
            assert!(direct.check_write(current, value).unwrap().is_err());
            assert_eq!(direct.write(current, value), Some(false));
            assert_eq!(direct, before);
            let before = e.clone();
            let start = e.bus.cpu_clock;
            let result = if concurrent {
                ScheduledBus::new(&mut e.bus, &mut e.sound_cpu, &mut e.sound_schedule)
                    .write_cycle(current, value, start)
            } else {
                e.bus.write_cycle(current, value, start)
            };
            assert!(result.result.is_err());
            assert_eq!(e, before);
        }
        let before = e.bus.cartridge.parallel_adapter.clone();
        let clock = e.bus.cpu_clock;
        assert!(
            discarded(&mut e, concurrent, access(0x430000, ByteLanes::Both), clock)
                .result
                .is_err()
        );
        assert_eq!(e.bus.cartridge.parallel_adapter, before);
        assert!(
            e.bus
                .cartridge
                .parallel_adapter
                .as_ref()
                .unwrap()
                .guarded(0x10fffe)
        );
        let mut first = machine(concurrent);
        first.bus.cartridge.parallel_adapter = Some(ParallelAdapter::new(Model::First));
        let before = first.clone();
        let clock = first.bus.cpu_clock;
        let result = if concurrent {
            ScheduledBus::new(
                &mut first.bus,
                &mut first.sound_cpu,
                &mut first.sound_schedule,
            )
            .write_cycle(access(0x010000, ByteLanes::Both), 0, clock)
        } else {
            first
                .bus
                .write_cycle(access(0x010000, ByteLanes::Both), 0, clock)
        };
        assert!(result.result.is_err());
        assert_eq!(first, before);
    }
}
