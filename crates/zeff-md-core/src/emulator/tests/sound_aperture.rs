use super::*;
use crate::hardware::sound::schedule::PendingSound;
use zeff_m68k::bus::{AccessKind, BusAccess, ByteLanes, Direction, FunctionCode};
use zeff_m68k::timing::{ClockedBus, CycleResult};

fn fixture(region: Region, concurrent_sound: bool, program: &[u16]) -> Emulator {
    Emulator::new(
        machine(program).rom_bytes().to_vec(),
        LoadConfig {
            region,
            concurrent_sound,
        },
    )
    .unwrap()
}

fn access(address: u32, lanes: ByteLanes) -> BusAccess {
    BusAccess {
        logical_address: address + u32::from(lanes == ByteLanes::Lower),
        bus_address: address,
        function_code: FunctionCode::SupervisorData,
        kind: AccessKind::Data,
        lanes,
    }
}

fn write(machine: &mut Emulator, access: BusAccess, value: u16) -> CycleResult<(), BusError> {
    let clock = machine.bus.cpu_clock;
    if machine.concurrent_sound {
        ScheduledBus::new(
            &mut machine.bus,
            &mut machine.sound_cpu,
            &mut machine.sound_schedule,
        )
        .write_cycle(access, value, clock)
    } else {
        machine.bus.write_cycle(access, value, clock)
    }
}

fn same_sound(machine: &Emulator, control: &Emulator) {
    assert_eq!(machine.bus.now, control.bus.now);
    assert_eq!(machine.bus.cpu_clock, control.bus.cpu_clock);
    assert_eq!(machine.bus.sound, control.bus.sound);
    assert_eq!(machine.sound_cpu, control.sound_cpu);
    assert_eq!(machine.sound_schedule, control.sound_schedule);
    assert_eq!(machine.bus.audio, control.bus.audio);
}

#[test]
fn unowned_writes_preserve_sound_and_elapsed_bus_effects() {
    for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
        for concurrent in [false, true] {
            for (reset, requested) in [(true, false), (true, true), (false, false)] {
                let mut seed = fixture(region, concurrent, &[0x4e71, 0x4e71]);
                seed.bus.sound.set_reset(reset);
                seed.bus.sound.request(requested);
                seed.bus.sound.bank = 0x155;
                seed.bus.sound.ram[0x11] = 0x5a;
                let now = seed.bus.now.get();
                seed.bus.audio.write_psg(now, 0x93);
                seed.bus.audio.write_opn2(now, 0, 0x2a);
                seed.bus.audio.write_opn2(now, 1, 0x55);
                for mirror in [0, 0x8000] {
                    for offset in [
                        0, 0x1ffe, 0x2000, 0x3ffe, 0x4000, 0x4002, 0x6000, 0x6002, 0x7efe, 0x7f00,
                        0x7f10, 0x7f20, 0x7ffe,
                    ] {
                        for lanes in [ByteLanes::Upper, ByteLanes::Lower, ByteLanes::Both] {
                            let mut actual = seed.clone();
                            let mut control = seed.clone();
                            let result = write(
                                &mut actual,
                                access(0xa00000 + mirror + offset, lanes),
                                0x9f01,
                            );
                            let expected = write(&mut control, access(0xe0fffe, lanes), 0);
                            assert_eq!(result, expected);
                            assert_eq!(result.result, Ok(()));
                            same_sound(&actual, &control);
                            assert_eq!(actual.bus.ram, control.bus.ram);
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn unowned_writes_do_not_shift_any_bank_and_owned_mirrors_still_write() {
    for bank in 0..512 {
        for mirror in [0, 0x8000] {
            let mut machine = machine(&[0x4e71, 0x4e71]);
            machine.bus.sound.set_reset(false);
            machine.bus.sound.bank = bank;
            write(
                &mut machine,
                access(0xa06000 + mirror, ByteLanes::Both),
                0x0100,
            )
            .result
            .unwrap();
            assert_eq!(machine.sound_bank(), bank);
            machine.bus.sound.request(true);
            machine.bus.sound.complete_boundary();
            write(
                &mut machine,
                access(0xa06000 + mirror, ByteLanes::Both),
                0x0100,
            )
            .result
            .unwrap();
            assert_eq!(machine.sound_bank(), (bank >> 1) | 0x100);
            write(
                &mut machine,
                access(0xa00010 + mirror, ByteLanes::Both),
                0x5a99,
            )
            .result
            .unwrap();
            write(
                &mut machine,
                access(0xa00010 + mirror, ByteLanes::Lower),
                0xa5,
            )
            .result
            .unwrap();
            assert_eq!(&machine.bus.sound.ram[0x10..0x12], &[0x5a, 0xa5]);
        }
    }
}

#[test]
fn pending_grant_is_resolved_before_aperture_write() {
    for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
        for delta in [-15i64, -1, 0, 1] {
            let mut machine = fixture(region, true, &[0x4e71, 0x4e71]);
            machine.bus.sound.set_reset(false);
            machine.bus.sound.ram[..6].copy_from_slice(&[0x3e, 0x5a, 0x32, 0, 1, 0x76]);
            let first = PendingSound::prepare(
                &machine.sound_cpu,
                &machine.bus.sound,
                &machine.bus.cartridge,
                0,
            )
            .unwrap();
            machine.bus.now = MasterTicks::new(first.deadline);
            first.publish(
                &mut machine.sound_cpu,
                &mut machine.bus.sound,
                &mut machine.bus.ram,
                &mut machine.bus.audio,
                &mut machine.bus.cartridge,
            );
            let pending = PendingSound::prepare(
                &machine.sound_cpu,
                &machine.bus.sound,
                &machine.bus.cartridge,
                machine.bus.now.get(),
            )
            .unwrap();
            assert_eq!(pending.deadline, 300);
            machine.sound_schedule.pending = Some(pending);
            machine.bus.sound.request(true);
            machine.bus.now = MasterTicks::new((272 + delta) as u64);
            let result = write(&mut machine, access(0xa08010, ByteLanes::Lower), 0x9f);
            let extra = (-delta).max(0) as u32;
            assert_eq!(result.result, Ok(()));
            assert_eq!(result.wait_clocks, extra.div_ceil(7));
            assert_eq!(
                machine.bus.now.get(),
                (300 + delta) as u64 + u64::from(result.wait_clocks) * 7
            );
            assert!(machine.sound_bus_granted());
            assert_eq!(machine.bus.sound.ram[0x11], 0x9f);
            assert_eq!(machine.bus.sound.ram[0x100], 0x5a);
            assert_eq!(machine.sound_cpu.cycles(), 20);
            assert!(machine.sound_deadline().is_none());
        }
    }
}

#[test]
fn unavailable_reads_and_owned_invalid_writes_stay_guarded() {
    for concurrent in [false, true] {
        for mirror in [0, 0x8000] {
            for lanes in [ByteLanes::Upper, ByteLanes::Lower, ByteLanes::Both] {
                let mut machine = machine(&[0x4e71, 0x4e71]);
                machine.concurrent_sound = concurrent;
                machine.bus.prefetch = Prefetch::Empty;
                for reset in [true, false] {
                    machine.bus.sound.set_reset(reset);
                    let clock = machine.bus.cpu_clock;
                    let address = access(0xa00010 + mirror, lanes);
                    let result = if concurrent {
                        ScheduledBus::new(
                            &mut machine.bus,
                            &mut machine.sound_cpu,
                            &mut machine.sound_schedule,
                        )
                        .read_cycle(address, clock)
                        .result
                    } else {
                        machine.bus.read_cycle(address, clock).result
                    };
                    assert_eq!(
                        result,
                        Err(BusError::UnsupportedAccess {
                            address: address.bus_address,
                            direction: Direction::Read
                        })
                    );
                }
                machine.bus.sound.request(true);
                machine.bus.sound.complete_boundary();
                let result = write(&mut machine, access(0xa07f20 + mirror, lanes), 0x9f01);
                assert_eq!(
                    result.result,
                    Err(BusError::UnsupportedAccess {
                        address: 0xa07f20 + mirror,
                        direction: Direction::Write
                    })
                );
            }
        }
    }
}

#[test]
fn ignored_main_instructions_restore_and_continue_through_fields() {
    for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
        for concurrent in [false, true] {
            let program = [
                0x13fc, 0x009f, 0x00a0, 0x8011, 0x13fc, 0x00bf, 0x00a0, 0x8011, 0x13fc, 0x00df,
                0x00a0, 0x8011, 0x13fc, 0x00ff, 0x00a0, 0x8011, 0x60fe, 0x4e71,
            ];
            let mut actual = fixture(region, concurrent, &program);
            actual.bus.sound.set_reset(false);
            actual.bus.sound.ram[..2].copy_from_slice(&[0x18, 0xfe]);
            actual.bus.sound.ram[0x11] = 0x5a;
            let mut control = actual.clone();
            for _ in 0..4 {
                actual.step_instruction().unwrap();
                assert_eq!(actual.bus.sound.ram[0x11], 0x5a);
            }
            assert_eq!(actual.registers().pc, 0x120);
            actual.drain_audio_samples();
            let state = actual.encode_state().unwrap();
            actual.decode_state(&state).unwrap();
            control.decode_state(&state).unwrap();
            let mut cloned = actual.clone();
            for _ in 0..3 {
                actual.step_field().unwrap();
                control.step_field().unwrap();
                cloned.step_field().unwrap();
                let samples = actual.drain_audio_samples();
                assert_eq!(samples, control.drain_audio_samples());
                assert_eq!(samples, cloned.drain_audio_samples());
                assert_eq!(
                    actual.encode_state().unwrap(),
                    control.encode_state().unwrap()
                );
                assert_eq!(
                    actual.encode_state().unwrap(),
                    cloned.encode_state().unwrap()
                );
                assert_eq!(actual, control);
            }
        }
    }
}

#[test]
fn owned_ym_mirrors_match_main_writes_reads_and_peek_on_both_aperture_halves() {
    for offset in [0, 4, 0x600, 0x1ffc] {
        for mirror in [0, 0x8000] {
            let mut actual = fixture(Region::NtscUs, false, &[0x4e71; 8]);
            actual.bus.sound.set_reset(false);
            actual.bus.sound.request(true);
            actual.bus.sound.complete_boundary();
            let mut canonical = actual.clone();
            let alias = 0xa04000 + mirror + offset;
            for (lanes, value) in [(ByteLanes::Upper, 0x2a00), (ByteLanes::Lower, 0x90)] {
                assert_eq!(
                    write(&mut actual, access(alias, lanes), value).result,
                    Ok(())
                );
                assert_eq!(
                    write(&mut canonical, access(0xa04000, lanes), value).result,
                    Ok(())
                );
                same_sound(&actual, &canonical);
            }
            assert_eq!(actual.opn2_snapshot().dac, 0x90);
            for port in 0..4 {
                assert_eq!(actual.peek(alias + port), canonical.peek(0xa04000 + port));
            }
            let aclock = actual.bus.cpu_clock;
            let cclock = canonical.bus.cpu_clock;
            let a = actual
                .bus
                .read_cycle(access(alias, ByteLanes::Both), aclock);
            let c = canonical
                .bus
                .read_cycle(access(0xa04000, ByteLanes::Both), cclock);
            assert_eq!(a, c);
            assert_eq!(a.result.unwrap() & 0x8080, 0x8080);
            same_sound(&actual, &canonical);
            assert_eq!(actual.peek(0xa06000 + mirror), Some(0xff));
        }
    }
}
