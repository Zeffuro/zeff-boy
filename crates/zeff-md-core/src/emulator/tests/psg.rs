use super::*;
use zeff_m68k::bus::{AccessKind, BusAccess, ByteLanes, Direction, FunctionCode};
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

fn write(
    machine: &mut Emulator,
    address: u32,
    lanes: ByteLanes,
    value: u16,
) -> Result<(), BusError> {
    let clock = machine.bus.cpu_clock;
    if machine.concurrent_sound {
        ScheduledBus {
            bus: &mut machine.bus,
            cpu: &mut machine.sound_cpu,
            schedule: &mut machine.sound_schedule,
        }
        .write_cycle(access(address, lanes), value, clock)
        .result
    } else {
        machine
            .bus
            .write_cycle(access(address, lanes), value, clock)
            .result
    }
}

#[test]
fn mapped_lanes_and_decode() {
    for concurrent in [false, true] {
        for base in [0xc00000, 0xc80000, 0xd8ff00] {
            for port in [0x10, 0x12, 0x14, 0x16] {
                let mut machine = super::machine(&[0x4e71, 0x4e71]);
                machine.concurrent_sound = concurrent;
                write(&mut machine, base + port, ByteLanes::Upper, 0x9000).unwrap();
                assert_eq!(machine.psg_snapshot().volumes, [15; 4]);
                write(&mut machine, base + port + 1, ByteLanes::Lower, 0x0093).unwrap();
                assert_eq!(machine.psg_snapshot().volumes, [3, 15, 15, 15]);
                write(&mut machine, base + port, ByteLanes::Both, 0x00b5).unwrap();
                assert_eq!(machine.psg_snapshot().volumes, [3, 5, 15, 15]);
                write(&mut machine, base + port, ByteLanes::Both, 0x9007).unwrap();
                assert_eq!(machine.psg_snapshot().volumes, [3, 7, 15, 15]);
            }
        }
        let mut machine = super::machine(&[0x4e71, 0x4e71]);
        machine.concurrent_sound = concurrent;
        for address in [0xc00018, 0xc00030, 0xc20010, 0xc00090] {
            assert_eq!(
                write(&mut machine, address, ByteLanes::Both, 0x0090),
                Err(BusError::UnsupportedAccess {
                    address,
                    direction: Direction::Write
                })
            );
            assert_eq!(machine.psg_snapshot().volumes, [15; 4]);
        }
    }
}

#[test]
fn granted_sound_aperture_uses_byte_parity() {
    let mut machine = super::machine(&[0x4e71, 0x4e71]);
    machine.bus.sound.set_reset(false);
    machine.bus.sound.request(true);
    machine.bus.sound.complete_boundary();
    for port in [0x7f10, 0x7f12, 0x7f14, 0x7f16] {
        write(&mut machine, 0xa00000 + port, ByteLanes::Both, 0x9000).unwrap();
        assert_eq!(machine.psg_snapshot().volumes[0], 15);
        write(&mut machine, 0xa00000 + port + 1, ByteLanes::Lower, 0x0090).unwrap();
        assert_eq!(machine.psg_snapshot().volumes[0], 0);
        write(&mut machine, 0xa00000 + port + 1, ByteLanes::Lower, 0x009f).unwrap();
    }
    assert!(write(&mut machine, 0xa07f21, ByteLanes::Lower, 0x90).is_err());
    machine.bus.sound.set_reset(true);
    assert!(write(&mut machine, 0xa07f11, ByteLanes::Lower, 0x90).is_err());
    assert_eq!(machine.psg_snapshot().volumes, [15; 4]);
}

#[test]
fn failed_restart_restores_audio_and_buffer() {
    for concurrent in [false, true] {
        let mut machine = super::machine(&[0x4e71, 0x4e71]);
        machine.concurrent_sound = concurrent;
        for value in [0x85, 0x06, 0x90] {
            write(&mut machine, 0xc00011, ByteLanes::Lower, value).unwrap();
        }
        machine
            .bus
            .advance_machine(crate::constants::NTSC_MASTER_HZ / 100)
            .unwrap();
        machine.bus.cartridge.initial.pc = 0xc00010;
        let saved = machine.clone();
        assert!(machine.reset_cpu().is_err());
        assert_eq!(machine, saved);
        let mut saved = saved;
        assert_eq!(machine.drain_audio_samples(), saved.drain_audio_samples());
        machine.bus.cartridge.initial.pc = 0x100;
        machine.set_sample_rate(44_100);
        Reset::reset(&mut machine);
        assert_eq!(machine.psg_snapshot().volumes, [15; 4]);
        assert_eq!(machine.sample_rate(), 44_100);
        assert!(machine.drain_audio_samples().is_empty());
    }
}

#[test]
fn faulted_cpu_cannot_repeat_psg_or_advance_audio() {
    for concurrent in [false, true] {
        let mut machine = super::machine(&[0x13fc, 0x0090, 0x00c0, 0x0011, 0x3010, 0x4e71, 0x4e71]);
        machine.concurrent_sound = concurrent;
        machine.cpu.registers.a[0] = 0xc00010;
        machine.step_instruction().unwrap();
        assert_eq!(machine.psg_snapshot().volumes[0], 0);
        assert!(matches!(
            machine.step_instruction(),
            Err(MachineFault::Main(TimingFault::Bus {
                source: BusError::UnsupportedAccess {
                    address: 0xc00010,
                    direction: Direction::Read
                },
                ..
            }))
        ));
        let saved = machine.clone();
        assert_eq!(
            machine.step_instruction(),
            Err(MachineFault::Main(TimingFault::Frozen))
        );
        assert_eq!(machine, saved);
        machine.bus.cartridge.initial.pc = 0x100;
        machine.reset_cpu().unwrap();
        assert_eq!(machine.psg_snapshot().volumes[0], 0);
    }
}

#[test]
fn failed_concurrent_prime_rolls_back_published_psg_write() {
    use crate::hardware::sound::schedule::PendingSound;
    let mut machine = super::machine(&[0x4e71, 0x4e71]);
    machine.concurrent_sound = true;
    machine.bus.sound.set_reset(false);
    machine.bus.sound.ram[..5].copy_from_slice(&[0x3e, 0x90, 0x32, 0x11, 0x7f]);
    PendingSound::prepare(
        &machine.sound_cpu,
        &machine.bus.sound,
        &machine.bus.cartridge,
        0,
    )
    .unwrap()
    .publish(
        &mut machine.sound_cpu,
        &mut machine.bus.sound,
        &mut machine.bus.ram,
        &mut crate::hardware::audio::Audio::default(),
    );
    machine.bus.advance_machine(156).unwrap();
    let pending = PendingSound::prepare(
        &machine.sound_cpu,
        &machine.bus.sound,
        &machine.bus.cartridge,
        56,
    )
    .unwrap();
    assert_eq!(pending.deadline, 240);
    machine.sound_schedule.pending = Some(pending);
    machine.bus.sound.request(true);
    machine.bus.cartridge.initial.pc = 0xc0000e;
    let saved = machine.clone();
    let mut witness = saved.clone();
    ScheduledBus {
        bus: &mut witness.bus,
        cpu: &mut witness.sound_cpu,
        schedule: &mut witness.sound_schedule,
    }
    .advance_machine(28)
    .unwrap();
    assert_eq!(witness.psg_snapshot().volumes[0], 0);
    assert_eq!(machine.psg_snapshot().volumes[0], 15);
    assert!(machine.reset_cpu().is_err());
    assert_eq!(machine, saved);
    let mut saved = saved;
    assert_eq!(machine.drain_audio_samples(), saved.drain_audio_samples());
}
