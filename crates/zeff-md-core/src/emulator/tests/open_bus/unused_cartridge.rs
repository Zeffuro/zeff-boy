use super::*;

#[path = "unused_cartridge/sf001.rs"]
mod sf001;
#[path = "unused_cartridge/unprotected.rs"]
mod unprotected;
use zeff_m68k::bus::Direction;

fn failure(address: u32) -> Result<u16, BusError> {
    Err(BusError::UnsupportedAccess {
        address,
        direction: Direction::Read,
    })
}

fn machine(program: &[u16], concurrent_sound: bool, save_bytes: Option<u32>) -> Emulator {
    let mut rom = vec![0; 0x400];
    rom[..8].copy_from_slice(&[0, 0xff, 0xff, 0xfc, 0, 0, 1, 0]);
    for (index, word) in program.iter().enumerate() {
        rom[0x100 + 2 * index..0x102 + 2 * index].copy_from_slice(&word.to_be_bytes());
    }
    if let Some(bytes) = save_bytes {
        rom[0x1b0..0x1b4].copy_from_slice(b"RA\xf8\x20");
        rom[0x1b4..0x1b8].copy_from_slice(&0x200001u32.to_be_bytes());
        rom[0x1b8..0x1bc].copy_from_slice(&(0x200001 + 2 * bytes - 2).to_be_bytes());
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

fn fixture(concurrent: bool) -> Emulator {
    let mut machine = machine(&[0x4e71; 2], concurrent, Some(8192));
    controls(&mut machine, true);
    machine
}

fn headerless_fixed_mapping(concurrent: bool, hash: &mut u64, cases: &mut usize) {
    for lanes in [ByteLanes::Upper, ByteLanes::Lower, ByteLanes::Both] {
        for history in [
            Prefetch::Empty,
            Prefetch::UpperOnly(0x4e),
            Prefetch::Full(0x4e71),
        ] {
            let mut machine = fixture(concurrent);
            machine.bus.cartridge.save = Some(crate::cartridge::ram::headerless_odd8_fixture());
            machine.bus.prefetch = history;
            let mut request = access(0x400046, AccessKind::Data, lanes);
            request.logical_address += u32::from(lanes == ByteLanes::Lower);
            let expected = history.sample(lanes).map_or(failure(0x400046), Ok);
            assert_eq!(read(&mut machine, concurrent, request), expected);
            assert_eq!(machine.bus.prefetch, Prefetch::Empty);
            assert_eq!(read(&mut machine, concurrent, request), failure(0x400046));
            record(hash, (concurrent, lanes, history, expected));
            *cases += 1;
        }
    }
}

fn cpu_copy(concurrent: bool, hash: &mut u64, cases: &mut usize) {
    let mut machine = machine(
        &[0x3e3c, 0x03df, 0x14d9, 0x51cf, 0xfffc, 0x4e71],
        concurrent,
        Some(8192),
    );
    controls(&mut machine, true);
    machine.cpu.registers.a[1] = 0x1f761858;
    machine.cpu.registers.a[2] = 0xa00a00;
    machine.step_instruction().unwrap();
    assert_eq!(machine.bus.prefetch, Prefetch::Full(0x51cf));
    let saved = machine.encode_state().unwrap();
    let mut cloned = machine.clone();
    let mut restored = machine.clone();
    restored.bus.prefetch = Prefetch::Empty;
    restored.decode_state(&saved).unwrap();
    assert_eq!(saved, restored.encode_state().unwrap());
    for current in [&mut machine, &mut cloned, &mut restored] {
        for offset in 0..992usize {
            assert_eq!(current.bus.prefetch, Prefetch::Full(0x51cf));
            let clock = current.cpu_clock();
            let step = current.step_instruction().unwrap();
            assert_eq!(current.cpu_clock().0 - clock.0, 12);
            assert_eq!(step.elapsed.get(), 84);
            assert_eq!(current.registers().a[1], 0x1f761859 + offset as u32);
            assert_eq!(current.registers().a[2], 0xa00a01 + offset as u32);
            assert_eq!(
                current.bus.sound.ram[0xa00 + offset],
                if offset & 1 == 0 { 0x51 } else { 0xcf }
            );
            current.step_instruction().unwrap();
        }
        assert_eq!(current.registers().pc, 0x10a);
        assert_eq!(current.registers().d[7], 0xffff);
    }
    assert!(machine == cloned);
    assert_eq!(machine.cpu, restored.cpu);
    assert_eq!(machine.bus.prefetch, restored.bus.prefetch);
    assert_eq!(machine.bus.sound, restored.bus.sound);
    assert_eq!(machine.bus.now, restored.bus.now);
    record(
        hash,
        (
            concurrent,
            machine.registers(),
            &machine.bus.sound.ram[0xa00..0xde0],
        ),
    );
    *cases += 1;
}

fn sound_visibility(hash: &mut u64, cases: &mut usize) {
    for local in [true, false] {
        for lanes in [ByteLanes::Upper, ByteLanes::Lower, ByteLanes::Both] {
            let mut machine = machine(&[0x4e71; 2], true, Some(8192));
            machine.bus.sound.set_reset(false);
            let program: &[u8] = if local {
                &[0, 0x76]
            } else {
                &[0x3a, 0, 0x80, 0x76]
            };
            machine.bus.sound.ram[..program.len()].copy_from_slice(program);
            let pending = PendingSound::prepare(
                &machine.sound_cpu,
                &machine.bus.sound,
                &machine.bus.cartridge,
                machine.bus.now.get(),
            )
            .unwrap();
            assert_eq!(pending.any_bank_access, !local);
            assert!(pending.bank_access.is_none() && pending.reservation().is_none());
            machine.sound_schedule.pending = Some(pending);
            if !local {
                machine.bus.sound.request(true);
            }
            assert!(!machine.bus.sound.granted && !machine.bus.sound.reset_asserted);
            machine.bus.prefetch = Prefetch::Full(0xa55a);
            let mut request = access(0x761858, AccessKind::Data, lanes);
            request.logical_address += u32::from(lanes == ByteLanes::Lower);
            let result = read(&mut machine, true, request);
            assert_eq!(
                result,
                if local {
                    Ok(Prefetch::Full(0xa55a).sample(lanes).unwrap())
                } else {
                    failure(0x761858)
                }
            );
            assert_eq!(machine.bus.prefetch, Prefetch::Empty);
            record(hash, (local, lanes, result));
            *cases += 1;
        }
    }
}

fn invalidation(concurrent: bool, hash: &mut u64, cases: &mut usize) {
    let request = access(0x761858, AccessKind::Data, ByteLanes::Upper);
    for event in 0..3 {
        let mut machine = fixture(concurrent);
        machine.bus.prefetch = Prefetch::Full(0x51cf);
        match event {
            0 => with_bus(&mut machine, concurrent, |bus, clock| bus.idle(clock, 1)),
            1 => {
                read(
                    &mut machine,
                    concurrent,
                    access(0x100, AccessKind::Data, ByteLanes::Both),
                )
                .unwrap();
            }
            _ => {
                with_bus(&mut machine, concurrent, |bus, clock| {
                    bus.write_cycle(
                        access(0xff0100, AccessKind::Data, ByteLanes::Both),
                        0,
                        clock,
                    )
                })
                .result
                .unwrap();
            }
        }
        assert_eq!(machine.bus.prefetch, Prefetch::Empty);
        assert_eq!(
            read(&mut machine, concurrent, request),
            failure(request.bus_address)
        );
        record(hash, (concurrent, event, machine.bus.prefetch));
        *cases += 1;
    }
    let mut machine = fixture(concurrent);
    super::super::super::vdp_dma::configure(&mut machine, 17, 0, 2);
    machine.bus.advance_machine(1).unwrap();
    let write = machine
        .bus
        .vdp
        .queue_register(1, 0x14, Region::NtscUs)
        .unwrap();
    machine
        .bus
        .advance_machine(write.publish_at.get() - machine.bus.now.get() + 1)
        .unwrap();
    machine.bus.ram[0x100..0x102].copy_from_slice(&0xa55au16.to_be_bytes());
    assert_eq!(
        read(
            &mut machine,
            concurrent,
            access(0xff0100, AccessKind::Prefetch, ByteLanes::Both)
        ),
        Ok(0xa55a)
    );
    machine
        .bus
        .vdp
        .write_control(0x4000, Region::NtscUs)
        .unwrap();
    machine.bus.vdp.write_control(0x80, Region::NtscUs).unwrap();
    let result = with_bus(&mut machine, concurrent, |bus, clock| {
        bus.read_cycle(request, clock)
    });
    assert!(result.wait_clocks > 0);
    assert_eq!(result.result, failure(request.bus_address));
    assert_eq!(machine.bus.prefetch, Prefetch::Empty);
    record(hash, (concurrent, result));
    *cases += 1;
}

pub(super) fn check(hash: &mut u64, cases: &mut usize) {
    unprotected::check(hash, cases);
    sf001::check(hash, cases);
    sound_visibility(hash, cases);
    for concurrent in [false, true] {
        headerless_fixed_mapping(concurrent, hash, cases);
        cpu_copy(concurrent, hash, cases);
        invalidation(concurrent, hash, cases);
        for bytes in [None, Some(512), Some(32768)] {
            let mut excluded = machine(&[0x4e71; 2], concurrent, bytes);
            controls(&mut excluded, true);
            for address in [0x400006, 0x400046] {
                for lanes in [ByteLanes::Upper, ByteLanes::Lower, ByteLanes::Both] {
                    excluded.bus.prefetch = Prefetch::Full(0x4e71);
                    let mut request = access(address, AccessKind::Data, lanes);
                    request.logical_address += u32::from(lanes == ByteLanes::Lower);
                    assert_eq!(
                        read(&mut excluded, concurrent, request),
                        failure(request.bus_address)
                    );
                }
            }
            record(hash, (bytes, excluded.bus.prefetch));
            *cases += 1;
        }
        let mut machine = fixture(concurrent);
        for address in [0x400000, 0x761858, 0x7ffffe] {
            for source in [Prefetch::UpperOnly(0x51), Prefetch::Full(0xa55a)] {
                for code in [FunctionCode::UserData, FunctionCode::SupervisorData] {
                    machine.bus.prefetch = source;
                    let mut request = access(address, AccessKind::Data, ByteLanes::Upper);
                    request.function_code = code;
                    request.logical_address |= 0x1f000000;
                    let before = machine.clone();
                    assert_eq!(machine.peek(request.logical_address), None);
                    assert_eq!(machine, before);
                    assert_eq!(
                        read(&mut machine, concurrent, request),
                        Ok(u16::from(source.upper().unwrap()) << 8)
                    );
                    assert_eq!(machine.bus.prefetch, Prefetch::Empty);
                    assert_eq!(read(&mut machine, concurrent, request), failure(address));
                }
            }
        }
        let request = access(0x761858, AccessKind::Data, ByteLanes::Upper);
        for lanes in [ByteLanes::Lower, ByteLanes::Both] {
            machine.bus.prefetch = Prefetch::Full(0x51cf);
            let mut request = request;
            request.lanes = lanes;
            request.logical_address |= 0x1f000000 | u32::from(lanes == ByteLanes::Lower);
            assert_eq!(
                read(&mut machine, concurrent, request),
                Ok(if lanes == ByteLanes::Lower {
                    0xcf
                } else {
                    0x51cf
                })
            );
            assert_eq!(machine.bus.prefetch, Prefetch::Empty);
            for source in [Prefetch::UpperOnly(0x51), Prefetch::Empty] {
                machine.bus.prefetch = source;
                assert_eq!(
                    read(&mut machine, concurrent, request),
                    failure(request.bus_address)
                );
            }
            machine.bus.prefetch = Prefetch::Full(0x51cf);
            request.logical_address ^= 1;
            assert_eq!(
                read(&mut machine, concurrent, request),
                failure(request.bus_address)
            );
        }
        machine.bus.prefetch = Prefetch::Full(0xa55a);
        assert_eq!(
            read(
                &mut machine,
                concurrent,
                access(0x400000, AccessKind::Data, ByteLanes::Both)
            ),
            Ok(0xa55a)
        );
        assert_eq!(
            read(
                &mut machine,
                concurrent,
                access(0x400002, AccessKind::Data, ByteLanes::Both)
            ),
            failure(0x400002)
        );
        for address in [0x800000, 0x9503ac, 0x9ffffe] {
            machine.bus.prefetch = Prefetch::Full(0x51cf);
            assert_eq!(
                read(
                    &mut machine,
                    concurrent,
                    access(address, AccessKind::Data, ByteLanes::Upper)
                ),
                failure(address)
            );
        }
        for kind in [AccessKind::Prefetch, AccessKind::Opcode, AccessKind::Stack] {
            machine.bus.prefetch = Prefetch::Full(0x51cf);
            let mut bad = request;
            bad.kind = kind;
            assert_eq!(
                read(&mut machine, concurrent, bad),
                failure(request.bus_address)
            );
        }
        for code in [
            FunctionCode::UserProgram,
            FunctionCode::SupervisorProgram,
            FunctionCode::CpuSpace,
        ] {
            machine.bus.prefetch = Prefetch::Full(0x51cf);
            let mut bad = request;
            bad.function_code = code;
            assert_eq!(
                read(&mut machine, concurrent, bad),
                failure(request.bus_address)
            );
        }
        for mismatch in [1, 2] {
            machine.bus.prefetch = Prefetch::Full(0x51cf);
            let mut bad = request;
            bad.logical_address += mismatch;
            assert_eq!(
                read(&mut machine, concurrent, bad),
                failure(request.bus_address)
            );
        }
        machine.bus.prefetch = Prefetch::Full(0x51cf);
        let mut bad = request;
        bad.bus_address |= 1;
        assert_eq!(
            read(&mut machine, concurrent, bad),
            failure(bad.bus_address)
        );
        machine.bus.cartridge.ssf2 = Some(crate::cartridge::ssf2::Ssf2::new());
        machine.bus.prefetch = Prefetch::Full(0x51cf);
        assert_eq!(
            read(&mut machine, concurrent, request),
            failure(request.bus_address)
        );
    }
}
