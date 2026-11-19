use super::sf001::cycle;
use super::*;

fn rig(program: &[u16], concurrent: bool) -> Emulator {
    rig_with_marker(program, concurrent, b"UNPROTECTED_TEST")
}

fn rig_with_marker(program: &[u16], concurrent: bool, marker: &[u8; 16]) -> Emulator {
    let mut rom = vec![0; 0x400];
    rom[..8].copy_from_slice(&[0, 0xff, 0xff, 0xfc, 0, 0, 1, 0]);
    rom[0x80..0x90].copy_from_slice(marker);
    for (index, word) in program.iter().enumerate() {
        rom[0x100 + 2 * index..0x102 + 2 * index].copy_from_slice(&word.to_be_bytes());
    }
    let mut machine = Emulator::new(
        rom,
        LoadConfig {
            region: Region::NtscUs,
            concurrent_sound: concurrent,
        },
    )
    .unwrap();
    assert!(machine.bus.cartridge.unprotected_rom.is_some());
    controls(&mut machine, true);
    machine
}

pub(super) fn check(hash: &mut u64, cases: &mut usize) {
    crate::cartridge::unprotected_rom::check_identity();
    for route in 0..3 {
        let template = rig(&[0x4e71; 2], route != 0);
        for address in [0x400000, 0x400006, 0x400046, 0x761858, 0x7ffffe] {
            for lanes in [ByteLanes::Upper, ByteLanes::Lower, ByteLanes::Both] {
                for history in [
                    Prefetch::Empty,
                    Prefetch::UpperOnly(0x51),
                    Prefetch::Full(0xa55a),
                ] {
                    let mut current = template.clone();
                    current.bus.prefetch = history;
                    let mut request = access(address, AccessKind::Data, lanes);
                    request.logical_address |= 0x1f000000 | u32::from(lanes == ByteLanes::Lower);
                    let before = current.clone();
                    assert_eq!(current.peek(request.logical_address), None);
                    assert_eq!(current, before);
                    let result = cycle(&mut current, route, request);
                    assert_eq!(result.wait_clocks, 0);
                    assert_eq!(
                        result.result,
                        history.sample(lanes).map_or(failure(address), Ok)
                    );
                    assert_eq!(current.bus.prefetch, Prefetch::Empty);
                    assert_eq!(cycle(&mut current, route, request).result, failure(address));
                    assert_eq!(current.bus.cartridge, before.bus.cartridge);
                    record(hash, (route, address, lanes, history, result));
                    *cases += 1;
                }
            }
        }
        guards(&template, route);
        interference(&template, route, hash, cases);
        let mut unknown = machine(&[0x4e71; 2], route != 0, None);
        controls(&mut unknown, true);
        assert!(unknown.bus.cartridge.unprotected_rom.is_none());
        for address in [0x400004, 0x400006, 0xa13000, 0xa15006] {
            unknown.bus.prefetch = Prefetch::Full(0xa55a);
            assert_eq!(
                cycle(
                    &mut unknown,
                    route,
                    access(address, AccessKind::Data, ByteLanes::Both)
                )
                .result,
                failure(address)
            );
        }
        let mut current = template;
        let cart = current.bus.cartridge.clone();
        let clock = current.bus.cpu_clock;
        let write = with_bus(&mut current, route != 0, |bus, _| {
            bus.write_cycle(
                access(0x400000, AccessKind::Data, ByteLanes::Both),
                0xf000,
                clock,
            )
        });
        assert_eq!(write.result, Ok(()));
        assert_eq!(current.bus.cartridge, cart);
        assert_eq!(current.peek(0x400000), None);
        assert_eq!(current.bus.cartridge.read(0), Some(0));
    }
    cpu_futures(hash, cases);
    identity_state();
    time_contract(hash, cases);
}

fn time_contract(hash: &mut u64, cases: &mut usize) {
    let program = [
        0x3039, 0x00a1, 0x3000, 0x3039, 0x00a1, 0x3000, 0x0200, 0, 0x13c0, 0x00ff, 0x110b, 0x13fc,
        1, 0x00ff, 0x110c, 0x4e71,
    ];
    for concurrent in [false, true] {
        let mut original = rig_with_marker(&program, concurrent, b"UNPROTECTED_TIME");
        let raw = original.encode_state().unwrap();
        let front = original.encode_frontend_state().unwrap();
        let mut cloned = original.clone();
        let mut restored = original.clone();
        restored.decode_state(&raw).unwrap();
        let mut frontend = original.clone();
        frontend.decode_frontend_state(&front).unwrap();
        for machine in [&mut original, &mut cloned, &mut restored, &mut frontend] {
            machine.step_instruction().unwrap();
            assert_eq!(machine.registers().d[0] & 0xffff, 0x3039);
            machine.step_instruction().unwrap();
            assert_eq!(machine.registers().d[0] & 0xffff, 0x0200);
            for _ in 0..3 {
                machine.step_instruction().unwrap();
            }
            assert_eq!(machine.bus.ram[0x110b], 0);
            assert_eq!(machine.bus.ram[0x110c], 1);
        }
        let endpoint = original.encode_state().unwrap();
        for machine in [&cloned, &restored, &frontend] {
            assert_eq!(machine.encode_state().unwrap(), endpoint);
        }
        for (time, address) in [(false, 0xa13000), (true, 0xa12ffe), (true, 0xa13100)] {
            let mut machine = rig_with_marker(
                &program,
                concurrent,
                if time {
                    b"UNPROTECTED_TIME"
                } else {
                    b"UNPROTECTED_TEST"
                },
            );
            machine.bus.prefetch = Prefetch::Full(0xa55a);
            assert_eq!(
                read(
                    &mut machine,
                    concurrent,
                    access(address, AccessKind::Data, ByteLanes::Both)
                ),
                failure(address)
            );
        }
        for source in [
            Prefetch::Empty,
            Prefetch::UpperOnly(0x51),
            Prefetch::Full(0xa55a),
        ] {
            let mut machine = original.clone();
            machine.bus.prefetch = source;
            assert_eq!(
                read(
                    &mut machine,
                    concurrent,
                    access(0xa13000, AccessKind::Data, ByteLanes::Both)
                ),
                source.sample(ByteLanes::Both).map_or(failure(0xa13000), Ok)
            );
            assert_eq!(machine.bus.prefetch, Prefetch::Empty);
        }
        record(hash, (concurrent, original.registers(), endpoint));
        *cases += 1;
    }
}

fn guards(template: &Emulator, route: u8) {
    let request = access(0x400006, AccessKind::Data, ByteLanes::Both);
    for variant in 0..15 {
        let mut current = template.clone();
        let mut bad = request;
        match variant {
            0 => bad.kind = AccessKind::Opcode,
            1 => bad.kind = AccessKind::Prefetch,
            2 => bad.kind = AccessKind::Stack,
            3 => bad.function_code = FunctionCode::CpuSpace,
            4 => bad.function_code = FunctionCode::SupervisorProgram,
            5 => bad.logical_address ^= 1,
            6 => bad.bus_address |= 1,
            7 => bad = access(0x800000, AccessKind::Data, ByteLanes::Both),
            8 => current.bus.cartridge.unprotected_rom = None,
            9 => current.bus.cartridge.ssf2 = Some(crate::cartridge::ssf2::Ssf2::new()),
            10 => {
                current.bus.cartridge.save = Some(crate::cartridge::ram::headerless_odd8_fixture())
            }
            11 => bad.function_code = FunctionCode::UserProgram,
            12 => bad.logical_address += 2,
            13 => {
                let mut rmw = access(0xff0100, AccessKind::Data, ByteLanes::Upper);
                rmw.function_code = FunctionCode::SupervisorData;
                assert!(matches!(
                    with_bus(&mut current, route != 0, |bus, clock| bus
                        .begin_rmw(rmw, clock))
                    .unwrap(),
                    zeff_m68k::bus::BusResponse::Complete(())
                ));
            }
            _ => bad = access(0x9503ac, AccessKind::Data, ByteLanes::Both),
        }
        current.bus.prefetch = Prefetch::Full(0xa55a);
        assert_eq!(
            cycle(&mut current, route, bad).result,
            failure(bad.bus_address)
        );
    }
    let mut current = template.clone();
    current.bus.prefetch = Prefetch::Full(0xa55a);
    let mut user = request;
    user.function_code = FunctionCode::UserData;
    assert_eq!(cycle(&mut current, route, user).result, Ok(0xa55a));
}

fn interference(template: &Emulator, route: u8, hash: &mut u64, cases: &mut usize) {
    for local in [true, false] {
        let mut current = template.clone();
        current.bus.sound.request(false);
        current.bus.sound.set_reset(false);
        let program: &[u8] = if local {
            &[0, 0x76]
        } else {
            &[0x3a, 0, 0x80, 0x76]
        };
        current.bus.sound.bank = 0x1fe;
        current.bus.sound.ram[..program.len()].copy_from_slice(program);
        let pending = PendingSound::prepare(
            &current.sound_cpu,
            &current.bus.sound,
            &current.bus.cartridge,
            current.bus.now.get(),
        )
        .unwrap();
        assert_eq!(pending.any_bank_access, !local);
        current.sound_schedule.pending = Some(pending);
        current.bus.prefetch = Prefetch::Full(0xa55a);
        let result = cycle(
            &mut current,
            route,
            access(0x400006, AccessKind::Data, ByteLanes::Both),
        );
        assert_eq!(
            result.result,
            if route == 0 || local {
                Ok(0xa55a)
            } else {
                failure(0x400006)
            }
        );
        assert_eq!(current.bus.prefetch, Prefetch::Empty);
        record(hash, (route, local, result));
        *cases += 1;
    }
    let mut armed = template.clone();
    super::super::super::super::vdp_dma::configure(&mut armed, 17, 0, 2);
    armed.bus.advance_machine(1).unwrap();
    let write = armed
        .bus
        .vdp
        .queue_register(1, 0x14, Region::NtscUs)
        .unwrap();
    armed
        .bus
        .advance_machine(write.publish_at.get() - armed.bus.now.get() + 1)
        .unwrap();
    armed.bus.ram[0x100..0x102].copy_from_slice(&0xa55au16.to_be_bytes());
    assert_eq!(
        cycle(
            &mut armed,
            route,
            access(0xff0100, AccessKind::Prefetch, ByteLanes::Both)
        )
        .result,
        Ok(0xa55a)
    );
    armed.bus.vdp.write_control(0x4000, Region::NtscUs).unwrap();
    armed.bus.vdp.write_control(0x80, Region::NtscUs).unwrap();
    let result = cycle(
        &mut armed,
        route,
        access(0x400006, AccessKind::Data, ByteLanes::Both),
    );
    assert!(result.wait_clocks > 0);
    assert_eq!(result.result, failure(0x400006));
    assert_eq!(armed.bus.prefetch, Prefetch::Empty);
    record(hash, (route, result));
    *cases += 1;
}

fn cpu_futures(hash: &mut u64, cases: &mut usize) {
    #[cfg(feature = "first-failure-diagnostic")]
    use crate::diagnostic::{Callback, Capture, Observation, Outcome};
    let program = [
        0x3039, 0x0040, 0x0006, 0x0240, 0xff00, 0x0c40, 0xf000, 0x6600, 0x0008, 0x2e7c, 0x00ff,
        0xff30, 0x5279, 0x00ff, 0xff32, 0x4e71, 0x4e71,
    ];
    for concurrent in [false, true] {
        let mut original = rig(&program, concurrent);
        let before = original.registers();
        let raw = original.encode_state().unwrap();
        let front = original.encode_frontend_state().unwrap();
        let mut cloned = original.clone();
        let mut restored = rig(&program, concurrent);
        restored.decode_state(&raw).unwrap();
        let mut frontend = rig(&program, concurrent);
        frontend.decode_frontend_state(&front).unwrap();
        for current in [&mut original, &mut cloned, &mut restored, &mut frontend] {
            #[cfg(feature = "first-failure-diagnostic")]
            let capture = Capture::begin().unwrap();
            current.step_instruction().unwrap();
            assert_eq!(current.registers().d[0] & 0xffff, 0x0240);
            assert_eq!(current.registers().pc, 0x106);
            #[cfg(feature = "first-failure-diagnostic")]
            {
                let report = capture.finish(current).unwrap();
                assert!(report.complete);
                let reads: Vec<_> = report
                    .history
                    .iter()
                    .flatten()
                    .filter(|record| {
                        record.callback == Callback::Read
                            && record
                                .access
                                .is_some_and(|access| access.bus_address == 0x400006)
                    })
                    .collect();
                assert_eq!(reads.len(), 1);
                assert_eq!(
                    reads[0].access,
                    Some(access(0x400006, AccessKind::Data, ByteLanes::Both))
                );
                assert_eq!(
                    reads[0].service.unwrap().prefetch,
                    Observation::Full(0x0240)
                );
                assert_eq!(reads[0].outcome, Some(Outcome::Read(0x0240)));
                assert_eq!(reads[0].returned.unwrap().bus.prefetch, Observation::Empty);
            }
            current.step_instruction().unwrap();
            assert_eq!(current.registers().d[0] & 0xffff, 0x0200);
            current.step_instruction().unwrap();
            assert_eq!(current.registers().status_register() & 4, 0);
            current.step_instruction().unwrap();
            assert_eq!(current.registers().pc, 0x118);
            assert_eq!(current.registers().ssp, before.ssp);
            current.step_instruction().unwrap();
            assert_eq!(&current.bus.ram[0xff32..0xff34], &[0, 1]);
            assert_eq!(current.registers().ssp, before.ssp);
        }
        assert_eq!(original, cloned);
        assert_eq!(
            original.encode_state().unwrap(),
            restored.encode_state().unwrap()
        );
        assert_eq!(
            original.encode_state().unwrap(),
            frontend.encode_state().unwrap()
        );
        record(hash, (concurrent, original.registers()));
        *cases += 1;
    }
}

fn identity_state() {
    let original = rig(&[0x4e71; 2], false);
    let raw = original.encode_state().unwrap();
    let front = original.encode_frontend_state().unwrap();
    for change in 0..4 {
        let mut target = original.clone();
        match change {
            0 => {
                *std::sync::Arc::make_mut(&mut target.bus.cartridge.bytes)
                    .last_mut()
                    .unwrap() ^= 1
            }
            1 => target.bus.cartridge.ssf2 = Some(crate::cartridge::ssf2::Ssf2::new()),
            2 => target.bus.cartridge.save = Some(crate::cartridge::ram::headerless_odd8_fixture()),
            _ => target.bus.cartridge.unprotected_rom = None,
        }
        assert!(target.encode_state().is_err());
        let before = target.clone();
        assert!(target.decode_state(&raw).is_err());
        assert_eq!(target, before);
        assert!(target.decode_frontend_state(&front).is_err());
        assert_eq!(target, before);
    }
}
