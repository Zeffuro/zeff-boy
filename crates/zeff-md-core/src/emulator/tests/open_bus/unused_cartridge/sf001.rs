use super::*;
use crate::cartridge::sf001::{Mapping, Revision};
use crate::hardware::bus::scheduled::CompactBus;
use zeff_m68k::timing::CycleResult;

fn rig_with(program: &[u16], concurrent: bool) -> Emulator {
    let mut rom = crate::cartridge::sf001::tests::rom(Revision::HiddenSave);
    for (index, word) in program.iter().enumerate() {
        rom[0x200 + 2 * index..0x202 + 2 * index].copy_from_slice(&word.to_be_bytes());
    }
    rom[0x3ffffe..].fill(0);
    let sum = rom[0x200..]
        .as_chunks::<2>()
        .0
        .iter()
        .fold(0_u16, |sum, pair| {
            sum.wrapping_add(u16::from_be_bytes(*pair))
        });
    rom[0x3ffffe..].copy_from_slice(&0x3e08u16.wrapping_sub(sum).to_be_bytes());
    let mut machine = Emulator::new(
        rom,
        LoadConfig {
            region: Region::NtscUs,
            concurrent_sound: concurrent,
        },
    )
    .unwrap();
    controls(&mut machine, true);
    machine
}

pub(super) fn cycle(
    machine: &mut Emulator,
    route: u8,
    request: BusAccess,
) -> CycleResult<u16, BusError> {
    let clock = machine.bus.cpu_clock;
    if route == 2 {
        let response = CompactBus(ScheduledBus::new(
            &mut machine.bus,
            &mut machine.sound_cpu,
            &mut machine.sound_schedule,
        ))
        .read_cycle(request, clock);
        CycleResult {
            result: response.result.map_err(|error| *error),
            wait_clocks: response.wait_clocks,
        }
    } else {
        with_bus(machine, route == 1, |bus, clock| {
            bus.read_cycle(request, clock)
        })
    }
}

fn rig(concurrent: bool) -> Emulator {
    rig_with(&[0x14d1, 0x4e71, 0x4e71], concurrent)
}

pub(super) fn check(hash: &mut u64, cases: &mut usize) {
    for route in 0..3 {
        let template = rig(route != 0);
        for address in [0x400000, 0x400046, 0x761858, 0x7ffffe] {
            for lanes in [ByteLanes::Upper, ByteLanes::Lower, ByteLanes::Both] {
                for history in [
                    Prefetch::Empty,
                    Prefetch::UpperOnly(0x51),
                    Prefetch::Full(0xa55a),
                ] {
                    let mut machine = template.clone();
                    machine.bus.prefetch = history;
                    let mut request = access(address, AccessKind::Data, lanes);
                    request.logical_address |= 0x1f000000 | u32::from(lanes == ByteLanes::Lower);
                    let before = machine.clone();
                    assert_eq!(machine.peek(request.logical_address), None);
                    assert!(machine == before);
                    let result = cycle(&mut machine, route, request);
                    assert_eq!(result.wait_clocks, 0);
                    assert_eq!(
                        result.result,
                        history.sample(lanes).map_or(failure(address), Ok)
                    );
                    assert_eq!(machine.bus.prefetch, Prefetch::Empty);
                    assert_eq!(cycle(&mut machine, route, request).result, failure(address));
                    assert_eq!(machine.bus.cartridge, before.bus.cartridge);
                    record(hash, (route, address, lanes, history, result));
                    *cases += 1;
                }
            }
        }
        for negative in 0..11 {
            let mut machine = template.clone();
            machine.bus.prefetch = Prefetch::Full(0xa55a);
            let mut request = access(0x400000, AccessKind::Data, ByteLanes::Upper);
            match negative {
                0 => request.kind = AccessKind::Opcode,
                1 => request.function_code = FunctionCode::CpuSpace,
                2 => request.logical_address += 1,
                3 => request.bus_address += 1,
                4 => request.kind = AccessKind::Prefetch,
                5 => request.function_code = FunctionCode::SupervisorProgram,
                6 => request = access(0x800000, AccessKind::Data, ByteLanes::Upper),
                7 => {
                    machine
                        .bus
                        .cartridge
                        .sf001
                        .as_mut()
                        .unwrap()
                        .control(0xe00, 0x80);
                }
                8 => {
                    machine
                        .bus
                        .cartridge
                        .sf001
                        .as_mut()
                        .unwrap()
                        .control(0xe00, 0x40);
                }
                9 => {
                    machine
                        .bus
                        .cartridge
                        .sf001
                        .as_mut()
                        .unwrap()
                        .control(0xe00, 0x20);
                }
                _ => machine.bus.cartridge.ssf2 = Some(crate::cartridge::ssf2::Ssf2::new()),
            }
            let before = machine.bus.cartridge.clone();
            assert_eq!(
                cycle(&mut machine, route, request).result,
                failure(request.bus_address)
            );
            assert_eq!(machine.bus.cartridge, before);
            record(hash, (route, negative, machine.bus.prefetch));
            *cases += 1;
        }
        let mut mapped = template.clone();
        mapped.bus.cartridge = crate::cartridge::Cartridge::new(
            crate::cartridge::sf001::tests::rom(Revision::UpperSave),
        )
        .unwrap();
        mapped.bus.prefetch = Prefetch::Full(0xa55a);
        assert_eq!(
            cycle(
                &mut mapped,
                route,
                access(0x400000, AccessKind::Data, ByteLanes::Upper)
            )
            .result,
            Ok(0xff00)
        );
        assert_eq!(mapped.bus.prefetch, Prefetch::Empty);
        assert_eq!(
            mapped.bus.cartridge.sf001.as_ref().unwrap().mapping(),
            Mapping::Linear
        );
        *cases += 1;
        for lanes in [ByteLanes::Upper, ByteLanes::Lower, ByteLanes::Both] {
            for local in [true, false] {
                let mut machine = template.clone();
                machine.bus.sound.request(false);
                machine.bus.sound.set_reset(false);
                let program: &[u8] = if local {
                    &[0, 0x76]
                } else {
                    &[0x3a, 0, 0x80, 0x76]
                };
                if !local {
                    machine.bus.sound.bank = 0x1fe;
                }
                machine.bus.sound.ram[..program.len()].copy_from_slice(program);
                machine.sound_schedule.pending = Some(
                    PendingSound::prepare(
                        &machine.sound_cpu,
                        &machine.bus.sound,
                        &machine.bus.cartridge,
                        machine.bus.now.get(),
                    )
                    .unwrap(),
                );
                if !local {
                    machine.bus.sound.request(true);
                }
                machine.bus.prefetch = Prefetch::Full(0xa55a);
                let mut request = access(0x400000, AccessKind::Data, lanes);
                request.logical_address += u32::from(lanes == ByteLanes::Lower);
                let result = cycle(&mut machine, route, request);
                assert_eq!(
                    result.result,
                    if route == 0 || local {
                        Ok(Prefetch::Full(0xa55a).sample(lanes).unwrap())
                    } else {
                        failure(0x400000)
                    }
                );
                assert_eq!(machine.bus.prefetch, Prefetch::Empty);
                record(hash, (route, lanes, local, result));
                *cases += 1;
            }
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
        for lanes in [ByteLanes::Upper, ByteLanes::Lower, ByteLanes::Both] {
            let mut machine = armed.clone();
            let mut request = access(0x400000, AccessKind::Data, lanes);
            request.logical_address += u32::from(lanes == ByteLanes::Lower);
            let result = cycle(&mut machine, route, request);
            assert!(result.wait_clocks > 0);
            assert_eq!(result.result, failure(0x400000));
            assert_eq!(machine.bus.prefetch, Prefetch::Empty);
            record(hash, (route, lanes, result));
            *cases += 1;
        }
    }
    cpu_futures(hash, cases);
}

fn cpu_futures(hash: &mut u64, cases: &mut usize) {
    let program = &[0x3e3c, 1, 0x14d9, 0x51cf, 0xfffc, 0x4e71];
    for concurrent in [false, true] {
        let mut original = rig_with(program, concurrent);
        original.cpu.registers.a[1] = 0x400000;
        original.cpu.registers.a[2] = 0xff0100;
        original.step_instruction().unwrap();
        assert_eq!(original.bus.prefetch, Prefetch::Full(0x51cf));
        let raw = original.encode_state().unwrap();
        let mut restored = rig_with(program, concurrent);
        restored.decode_state(&raw).unwrap();
        assert_eq!(raw, restored.encode_state().unwrap());
        let mut cloned = original.clone();
        for machine in [&mut original, &mut cloned, &mut restored] {
            for offset in 0..2 {
                machine.step_instruction().unwrap();
                assert_eq!(
                    machine.bus.ram[0x100 + offset],
                    if offset == 0 { 0x51 } else { 0xcf }
                );
                machine.step_instruction().unwrap();
            }
            assert_eq!(machine.registers().pc, 0x20a);
            assert_eq!(machine.registers().d[7], 0xffff);
            assert_eq!(machine.registers().a[1], 0x400002);
            assert_eq!(machine.registers().a[2], 0xff0102);
        }
        assert!(original == cloned);
        assert_eq!(original.cpu, restored.cpu);
        assert_eq!(original.bus.cartridge, restored.bus.cartridge);
        assert_eq!(original.bus.ram, restored.bus.ram);
        record(hash, (concurrent, original.registers()));
        *cases += 1;
        let mut word = rig_with(&[0x3011, 0x4e71, 0x4e71], concurrent);
        word.cpu.registers.a[1] = 0x400000;
        word.step_instruction().unwrap();
        assert_eq!(word.registers().d[0] & 0xffff, 0x4e71);
        record(hash, (concurrent, word.registers()));
        *cases += 1;
    }
}
