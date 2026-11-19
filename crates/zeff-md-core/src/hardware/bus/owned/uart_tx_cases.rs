use super::*;
use zeff_m68k::bus::FunctionCode;
use zeff_m68k::timing::PrefetchCpu;

fn prefix(region: Region, port: usize) -> Bus {
    let mut words = vec![0x4e71_u16; 1000];
    words.extend([0x13fc, 0x38, 0x00a1, 0x13 + port as u16 * 6, 0x4e71, 0x60fc]);
    let mut rom = vec![0; 0x2000];
    rom[..8].copy_from_slice(&[0, 0xff, 0xff, 0xfc, 0, 0, 2, 0]);
    for (index, word) in words.iter().enumerate() {
        rom[0x200 + index * 2..0x202 + index * 2].copy_from_slice(&word.to_be_bytes());
    }
    let mut bus = Bus::new_owned(Cartridge::new(rom).unwrap(), region).unwrap();
    let mut cpu = PrefetchCpu::default();
    cpu.registers = bus.cartridge.effective_initial();
    cpu.prime(&mut bus.owned_context().unwrap()).unwrap();
    for _ in 0..1001 {
        cpu.step_boundary(&mut bus.owned_context().unwrap())
            .unwrap();
    }
    assert_eq!(cpu.registers.pc, 0x9d8);
    assert_eq!(cpu.clock(), bus.cpu_clock);
    assert_eq!(
        bus.input.read_serial(port * 3 + 2, bus.now.get(), region),
        0x38
    );
    assert_eq!(bus.input.fault(), None);
    // The remaining checks own standalone bus cycles; this CPU is not resumed.
    bus
}

fn access(port: usize, lanes: ByteLanes, function_code: FunctionCode) -> BusAccess {
    let bus_address = 0xa1000e + port as u32 * 6;
    BusAccess {
        logical_address: bus_address + u32::from(lanes == ByteLanes::Lower),
        bus_address,
        function_code,
        kind: AccessKind::Data,
        lanes,
    }
}

fn write(bus: &mut Bus, access: BusAccess, word: u16) -> Result<Transfer> {
    let start = bus.cpu_clock;
    bus.owned_context().unwrap().transfer(
        main_bus::Request {
            access,
            operation: main_bus::Operation::Write(word),
        },
        start,
    )
}

fn receipt(
    bus: &Bus,
    before: main_bus::Snapshot,
    transfer: &Transfer,
    clock: CpuClocks,
) -> (u64, u64) {
    let main = transfer.main;
    assert_eq!(before.origin_rise_half, 5);
    assert_eq!(before.phase, main_bus::Phase::Idle);
    assert!(before.pending.is_none() && before.completed.is_none());
    assert_eq!(main.start_half, before.last_edge_half);
    assert_eq!(main.start_half % 14, 5);
    assert_eq!(main.end_half - main.start_half, 56);
    assert_eq!(main.sampled_ack_half - main.start_half, 36);
    assert_eq!((main.waits, transfer.wait_clocks), (0, 0));
    assert_eq!(main.vpa, None);
    assert_eq!(bus.vdp.owned_half(), Some(main.end_half));
    let begin = main.start_half / 2 + 1;
    let end = main.end_half / 2 + 1;
    assert_eq!(end - begin, 28);
    assert_eq!(bus.now.get() + 1, end);
    let owner = bus.owned.as_ref().unwrap();
    assert_eq!(
        (owner.epoch_tick, owner.idle_clocks, clock.0),
        (3, 98_304, 4092)
    );
    assert_eq!(begin, owner.epoch_tick + 7 * (clock.0 + owner.idle_clocks));
    assert_eq!(
        (begin, end),
        (716_775, 716_803),
        "actual UART pulse {begin}..{end}, epoch={}, idle={}, clock={:?}, receipt={main:?}, region={:?}",
        bus.owned.as_ref().unwrap().epoch_tick,
        bus.owned.as_ref().unwrap().idle_clocks,
        bus.cpu_clock,
        bus.region,
    );
    (begin, end)
}

fn rejected(prefix: &Bus, access: BusAccess, word: u16, advances: bool) {
    let mut bus = prefix.clone();
    let input = bus.input.clone();
    let half = bus.vdp.owned_half().unwrap();
    assert!(write(&mut bus, access, word).is_err());
    assert_eq!(bus.input, input);
    assert_eq!(bus.input.fault(), None);
    if advances {
        assert!(bus.vdp.owned_half().unwrap() > half);
    }
}

fn positive(prefix: &Bus, port: usize, request: BusAccess, word: u16) {
    let mut bus = prefix.clone();
    let mut expected = bus.input.clone();
    let clock = bus.cpu_clock;
    let before = bus.owned.as_ref().unwrap().board.snapshot().unwrap();
    assert!(bus.owned.as_ref().unwrap().board.ready_tick().is_some());
    let transfer = write(&mut bus, request, word).unwrap();
    let (begin, end) = receipt(&bus, before, &transfer, clock);
    assert_eq!(bus.cpu_clock, CpuClocks(clock.0 + 4));
    assert!(expected.write_serial_at_range(port * 3, 0x53, begin, end, bus.region));
    assert_eq!(bus.input, expected);
    assert_eq!(bus.input.read_serial(port * 3, end, bus.region), 0x53);
    assert!(!bus.input.serial_read_allowed(port * 3 + 2));
    assert!(!bus.input.data_read_allowed(port));
    let load = if bus.region == Region::Pal {
        726_261
    } else {
        722_397
    };
    for (begin, end, status) in [
        (load - 50, load - 22, 0x39),
        (load - 25, load + 3, 0x39),
        (load - 24, load + 4, 0x38),
    ] {
        assert_eq!(
            bus.input
                .read_serial_at_range(port * 3 + 2, begin, end, bus.region),
            Some(status)
        );
    }
    let mut full = bus.input.clone();
    assert!(!full.write_serial_at_range(port * 3, 0x4d, end, end + 28, bus.region));
    assert_eq!(full, bus.input);
    rejected(
        &bus,
        access(port, ByteLanes::Lower, FunctionCode::SupervisorData),
        0x4d4d,
        true,
    );
    rejected(
        &bus,
        access(2, ByteLanes::Lower, FunctionCode::SupervisorData),
        0x4d4d,
        true,
    );
    let input = bus.input.clone();
    let mut read = bus.clone();
    let clock = read.cpu_clock;
    let result = read.owned_context().unwrap().transfer(
        main_bus::Request {
            access: BusAccess {
                logical_address: 0xa10013 + port as u32 * 6,
                bus_address: 0xa10012 + port as u32 * 6,
                ..access(port, ByteLanes::Lower, FunctionCode::SupervisorData)
            },
            operation: main_bus::Operation::Read,
        },
        clock,
    );
    assert!(result.is_err());
    assert_eq!(read.input, input);
}

pub(crate) fn checks() {
    for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
        for port in 0..2 {
            let base = prefix(region, port);
            for function_code in [FunctionCode::UserData, FunctionCode::SupervisorData] {
                for lanes in [ByteLanes::Lower, ByteLanes::Both] {
                    let request = access(port, lanes, function_code);
                    let word = if lanes == ByteLanes::Both {
                        0xa553
                    } else {
                        0x5353
                    };
                    positive(&base, port, request, word);
                    positive(
                        &base,
                        port,
                        BusAccess {
                            logical_address: request.logical_address | 0x0100_0000,
                            ..request
                        },
                        word,
                    );
                }
                let mut upper = base.clone();
                let input = upper.input.clone();
                let result = write(
                    &mut upper,
                    access(port, ByteLanes::Upper, function_code),
                    0xa5a5,
                )
                .unwrap();
                assert_eq!(result.main.waits, 0);
                assert_eq!(upper.input, input);
            }
            let request = access(port, ByteLanes::Lower, FunctionCode::SupervisorData);
            for bad in [
                BusAccess {
                    function_code: FunctionCode::UserProgram,
                    ..request
                },
                BusAccess {
                    function_code: FunctionCode::SupervisorProgram,
                    ..request
                },
                BusAccess {
                    kind: AccessKind::Prefetch,
                    ..request
                },
                BusAccess {
                    logical_address: 0xa1002f,
                    bus_address: 0xa1002e,
                    ..request
                },
            ] {
                rejected(&base, bad, 0x5353, true);
            }
            for bad in [
                BusAccess {
                    logical_address: request.logical_address - 1,
                    ..request
                },
                BusAccess {
                    bus_address: request.bus_address + 1,
                    ..request
                },
            ] {
                rejected(&base, bad, 0x5353, false);
            }
        }
    }
}
