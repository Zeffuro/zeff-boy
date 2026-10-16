use super::*;
use crate::constants::SOUND_BUS_REQUEST;
use crate::hardware::sound::schedule::PendingSound;
use zeff_m68k::bus::{AccessKind, BusAccess, ByteLanes, FunctionCode};
use zeff_m68k::timing::{ClockedBus, InterruptVector};

fn access(address: u32, kind: AccessKind, lanes: ByteLanes) -> BusAccess {
    BusAccess {
        logical_address: address,
        bus_address: address,
        kind,
        lanes,
        function_code: FunctionCode::SupervisorData,
    }
}

fn with_bus<T>(
    machine: &mut Emulator,
    concurrent: bool,
    f: impl FnOnce(&mut dyn ClockedBus<Error = BusError>, CpuClocks) -> T,
) -> T {
    let clock = machine.bus.cpu_clock;
    if concurrent {
        f(
            &mut ScheduledBus {
                bus: &mut machine.bus,
                cpu: &mut machine.sound_cpu,
                schedule: &mut machine.sound_schedule,
            },
            clock,
        )
    } else {
        f(&mut machine.bus, clock)
    }
}

fn read(machine: &mut Emulator, concurrent: bool, access: BusAccess) -> Result<u16, BusError> {
    with_bus(machine, concurrent, |bus, clock| {
        let result = bus.read_cycle(access, clock);
        assert_eq!(result.wait_clocks, 0);
        result.result
    })
}

fn seed(machine: &mut Emulator, concurrent: bool) {
    assert_eq!(
        read(
            machine,
            concurrent,
            access(0x100, AccessKind::Prefetch, ByteLanes::Both)
        )
        .map(|word| word >> 8),
        Ok(0x4e)
    );
    assert_eq!(machine.bus.prefetch_upper, Some(0x4e));
}

#[test]
fn prefetch_sources_and_read_consumption() {
    for concurrent in [false, true] {
        let mut machine = super::machine(&[0x4e71, 0x4e71]);
        for address in [0xe00000, 0xff0000] {
            machine.bus.ram[..2].copy_from_slice(&[0x7a, 0x55]);
            assert_eq!(
                read(
                    &mut machine,
                    concurrent,
                    access(address, AccessKind::Prefetch, ByteLanes::Both)
                ),
                Ok(0x7a55)
            );
            assert_eq!(machine.bus.prefetch_upper, Some(0x7a));
        }
        for offset in (0..=0xfe).step_by(2) {
            seed(&mut machine, concurrent);
            let saved = machine.clone();
            assert_eq!(machine.peek(0x100), Some(0x4e));
            assert_eq!(machine.peek(0xff0000), Some(0x7a));
            assert_eq!(machine, saved);
            let controls = machine.bus.sound.clone();
            assert_eq!(
                read(
                    &mut machine,
                    concurrent,
                    access(
                        SOUND_BUS_REQUEST + offset,
                        AccessKind::Data,
                        ByteLanes::Upper
                    )
                ),
                Ok(0x4f00)
            );
            assert_eq!(machine.bus.prefetch_upper, None);
            assert_eq!(machine.bus.sound, controls);
            assert_eq!(
                read(
                    &mut machine,
                    concurrent,
                    access(
                        SOUND_BUS_REQUEST + offset,
                        AccessKind::Data,
                        ByteLanes::Upper
                    )
                ),
                Ok(0x100)
            );
            seed(&mut machine, concurrent);
            assert_eq!(
                read(
                    &mut machine,
                    concurrent,
                    access(
                        SOUND_BUS_REQUEST + offset,
                        AccessKind::Data,
                        ByteLanes::Both
                    )
                ),
                Ok(0x100)
            );
            assert_eq!(machine.bus.prefetch_upper, None);
        }
        for kind in [
            AccessKind::Data,
            AccessKind::Opcode,
            AccessKind::Extension,
            AccessKind::ResetVector,
        ] {
            seed(&mut machine, concurrent);
            read(
                &mut machine,
                concurrent,
                access(0x100, kind, ByteLanes::Both),
            )
            .unwrap();
            assert_eq!(machine.bus.prefetch_upper, None);
        }
        for lanes in [ByteLanes::Upper, ByteLanes::Lower] {
            seed(&mut machine, concurrent);
            read(
                &mut machine,
                concurrent,
                access(0x100, AccessKind::Prefetch, lanes),
            )
            .unwrap();
            assert_eq!(machine.bus.prefetch_upper, None);
        }
        let mut bytes = machine.bus.cartridge.bytes.to_vec();
        bytes.push(0x7a);
        machine.bus.cartridge.bytes = bytes.into();
        for address in [0x200, 0x202] {
            seed(&mut machine, concurrent);
            assert_eq!(
                read(
                    &mut machine,
                    concurrent,
                    access(address, AccessKind::Prefetch, ByteLanes::Both)
                ),
                Ok(if address == 0x200 { 0x7aff } else { 0xffff })
            );
            assert_eq!(machine.bus.prefetch_upper, None);
        }
    }
}

#[test]
fn completed_activity_invalidates_prefetch() {
    for concurrent in [false, true] {
        for address in [0x100, 0xff0000, SOUND_BUS_REQUEST, 0xa11300] {
            let mut machine = super::machine(&[0x4e71, 0x4e71]);
            seed(&mut machine, concurrent);
            let result = with_bus(&mut machine, concurrent, |bus, clock| {
                bus.write_cycle(access(address, AccessKind::Data, ByteLanes::Both), 0, clock)
            });
            assert_eq!(result.result.is_ok(), address != 0xa11300);
            assert_eq!(machine.bus.prefetch_upper, None);
        }
        let mut machine = super::machine(&[0x4e71, 0x4e71]);
        seed(&mut machine, concurrent);
        read(
            &mut machine,
            concurrent,
            access(0xa11200, AccessKind::Data, ByteLanes::Upper),
        )
        .unwrap_err();
        assert_eq!(machine.bus.prefetch_upper, None);
        seed(&mut machine, concurrent);
        with_bus(&mut machine, concurrent, |bus, clock| bus.idle(clock, 0));
        assert_eq!(machine.bus.prefetch_upper, Some(0x4e));
        with_bus(&mut machine, concurrent, |bus, clock| bus.idle(clock, 1));
        assert_eq!(machine.bus.prefetch_upper, None);
        seed(&mut machine, concurrent);
        let result = with_bus(&mut machine, concurrent, |bus, clock| {
            bus.acknowledge_cycle(
                access(0xfffffa, AccessKind::InterruptAcknowledge, ByteLanes::Both),
                clock,
            )
        });
        assert_eq!(
            result.result,
            Ok(zeff_m68k::bus::BusResponse::Complete(
                InterruptVector::Autovector
            ))
        );
        assert_eq!(machine.bus.prefetch_upper, None);
        seed(&mut machine, concurrent);
        with_bus(&mut machine, concurrent, |bus, clock| {
            bus.begin_reset_output(clock).unwrap();
            bus.end_reset_output(clock);
        });
        assert_eq!(machine.bus.prefetch_upper, None);
        machine.reset_cpu().unwrap();
        assert_eq!(machine.bus.prefetch_upper, Some(0x4e));
        Reset::reset(&mut machine);
        assert_eq!(machine.bus.prefetch_upper, Some(0x4e));
    }
}

#[test]
fn failed_advancement_preserves_prefetch() {
    for concurrent in [false, true] {
        for operation in 0..4 {
            let mut machine = super::machine(&[0x4e71, 0x4e71]);
            seed(&mut machine, concurrent);
            machine.bus.now = MasterTicks::new(u64::MAX - 1);
            let saved = machine.clone();
            let request = access(SOUND_BUS_REQUEST, AccessKind::Data, ByteLanes::Upper);
            with_bus(&mut machine, concurrent, |bus, clock| match operation {
                0 => assert_eq!(
                    bus.read_cycle(request, clock).result,
                    Err(BusError::ClockOverflow)
                ),
                1 => assert_eq!(
                    bus.write_cycle(request, 0, clock).result,
                    Err(BusError::ClockOverflow)
                ),
                2 => assert_eq!(
                    bus.acknowledge_cycle(request, clock).result,
                    Err(BusError::ClockOverflow)
                ),
                3 => bus.idle(clock, 1),
                _ => unreachable!(),
            });
            assert_eq!(machine.bus.prefetch_upper, Some(0x4e));
            if operation == 3 {
                assert!(machine.bus.clock_fault);
                machine.bus.clock_fault = saved.bus.clock_fault;
            }
            assert_eq!(machine, saved);
            assert_eq!(machine.reset_cpu(), Err(TimingFault::ClockOverflow));
            assert_eq!(machine, saved);
        }
        let mut machine = super::machine(&[0x4e71, 0x4e71]);
        seed(&mut machine, concurrent);
        let saved = machine.clone();
        with_bus(&mut machine, concurrent, |bus, clock| {
            assert!(matches!(
                bus.read_cycle(
                    access(0x100, AccessKind::Prefetch, ByteLanes::Both),
                    CpuClocks(clock.0 + 1)
                )
                .result,
                Err(BusError::ClockMismatch { .. })
            ))
        });
        assert_eq!(machine, saved);
    }
}

#[test]
fn sound_activity_and_stop_discard_observation() {
    let mut held = super::machine(&[0x4e71, 0x4e71]);
    held.bus.sound.set_reset(false);
    held.bus.sound.ram[..2].copy_from_slice(&[0x3e, 0x55]);
    let future = PendingSound::prepare(
        &held.sound_cpu,
        &held.bus.sound,
        &held.bus.cartridge,
        held.bus.now.get(),
    )
    .unwrap();
    assert_eq!(future.deadline, 150);
    held.sound_schedule.pending = Some(future);
    held.bus.sound.request(true);
    seed(&mut held, true);
    assert_eq!(
        read(
            &mut held,
            true,
            access(SOUND_BUS_REQUEST, AccessKind::Data, ByteLanes::Upper)
        ),
        Ok(0x100)
    );
    assert_eq!(held.bus.now.get(), 112);
    assert_eq!(held.sound_cpu.cycles(), 0);
    assert_eq!(held.sound_deadline(), Some(MasterTicks::new(150)));
    assert!(!held.sound_bus_granted());
    assert_eq!(held.bus.prefetch_upper, None);
    let mut machine = super::machine(&[0x4e71, 0x4e71]);
    machine.bus.sound.set_reset(false);
    let pending = PendingSound::prepare(
        &machine.sound_cpu,
        &machine.bus.sound,
        &machine.bus.cartridge,
        machine.bus.now.get(),
    )
    .unwrap();
    assert_eq!(pending.deadline, 105);
    machine.sound_schedule.pending = Some(pending);
    machine.bus.sound.request(true);
    seed(&mut machine, true);
    assert_eq!(machine.bus.now.get(), 84);
    assert!(!machine.sound_bus_granted());
    assert_eq!(
        read(
            &mut machine,
            true,
            access(SOUND_BUS_REQUEST, AccessKind::Data, ByteLanes::Upper)
        ),
        Ok(0)
    );
    assert!(machine.sound_bus_granted());
    assert_eq!(machine.sound_cpu.cycles(), 4);
    assert_eq!(machine.bus.prefetch_upper, None);
    seed(&mut machine, true);
    ScheduledBus {
        bus: &mut machine.bus,
        cpu: &mut machine.sound_cpu,
        schedule: &mut machine.sound_schedule,
    }
    .advance_machine(0)
    .unwrap();
    assert_eq!(machine.bus.prefetch_upper, Some(0x4e));
    ScheduledBus {
        bus: &mut machine.bus,
        cpu: &mut machine.sound_cpu,
        schedule: &mut machine.sound_schedule,
    }
    .advance_machine(1)
    .unwrap();
    assert_eq!(machine.bus.prefetch_upper, None);
    for concurrent_sound in [false, true] {
        let mut machine = super::machine(&[0x4e72, 0x2000, 0x4e71, 0x4e71]);
        machine.concurrent_sound = concurrent_sound;
        machine.step_instruction().unwrap();
        assert_eq!(machine.bus.prefetch_upper, None);
        seed(&mut machine, concurrent_sound);
        assert_eq!(
            machine.step_instruction().unwrap().cpu,
            TimedOutcome::Stopped
        );
        assert_eq!(machine.bus.prefetch_upper, None);
        seed(&mut machine, concurrent_sound);
        machine.bus.now = MasterTicks::new(u64::MAX);
        assert!(machine.step_instruction().is_err());
        assert_eq!(machine.bus.prefetch_upper, Some(0x4e));
    }
    let mut machine = super::machine(&[0x4e72, 0x2000, 0x4e71, 0x4e71]);
    machine.bus.sound.set_reset(false);
    machine.step_instruction().unwrap();
    assert_eq!(machine.bus.prefetch_upper, None);
    seed(&mut machine, false);
    machine.step_sound_instruction().unwrap_err();
    assert_eq!(machine.bus.prefetch_upper, Some(0x4e));
    machine.step_instruction().unwrap();
    seed(&mut machine, false);
    machine.step_sound_instruction().unwrap();
    assert_eq!(machine.bus.prefetch_upper, None);
}
