use super::*;
use crate::hardware::audio::{Audio, Opn2};
use crate::hardware::sound::schedule::PendingSound;
use zeff_m68k::bus::{AccessKind, BusAccess, ByteLanes, Direction, FunctionCode};
use zeff_m68k::timing::ClockedBus;

fn fixture(region: Region, concurrent_sound: bool, program: &[u16]) -> Emulator {
    let mut image = vec![0; 0x200];
    image[..8].copy_from_slice(&[0, 0xff, 0xff, 0xfc, 0, 0, 1, 0]);
    for (index, word) in program.iter().enumerate() {
        image[0x100 + index * 2..0x102 + index * 2].copy_from_slice(&word.to_be_bytes());
    }
    Emulator::new(
        image,
        LoadConfig {
            region,
            concurrent_sound,
        },
    )
    .unwrap()
}

fn access(address: u32, lanes: ByteLanes) -> BusAccess {
    BusAccess {
        logical_address: address,
        bus_address: address & !1,
        function_code: FunctionCode::SupervisorData,
        kind: AccessKind::Data,
        lanes,
    }
}

fn transfer(
    machine: &mut Emulator,
    address: u32,
    lanes: ByteLanes,
    value: Option<u16>,
) -> Result<u16, BusError> {
    let clock = machine.bus.cpu_clock;
    if machine.concurrent_sound {
        let mut bus = ScheduledBus {
            bus: &mut machine.bus,
            cpu: &mut machine.sound_cpu,
            schedule: &mut machine.sound_schedule,
        };
        match value {
            Some(value) => bus
                .write_cycle(access(address, lanes), value, clock)
                .result
                .map(|()| 0),
            None => bus.read_cycle(access(address, lanes), clock).result,
        }
    } else {
        match value {
            Some(value) => machine
                .bus
                .write_cycle(access(address, lanes), value, clock)
                .result
                .map(|()| 0),
            None => machine.bus.read_cycle(access(address, lanes), clock).result,
        }
    }
}

fn grant(machine: &mut Emulator) {
    machine.bus.sound.set_reset(false);
    machine.bus.sound.request(true);
    machine.bus.sound.complete_boundary();
}

fn register(machine: &mut Emulator, bank: u32, address: u8, value: u8) {
    transfer(
        machine,
        0xa04000 + bank * 2,
        ByteLanes::Upper,
        Some(u16::from(address) << 8),
    )
    .unwrap();
    transfer(
        machine,
        0xa04001 + bank * 2,
        ByteLanes::Lower,
        Some(u16::from(value)),
    )
    .unwrap();
}

fn sounding(machine: &mut Emulator) {
    grant(machine);
    register(machine, 1, 0xb6, 0xc0);
    register(machine, 0, 0x2a, 255);
    register(machine, 0, 0x2b, 0x80);
    for value in [0x85, 0x06, 0x90] {
        transfer(machine, 0xc00011, ByteLanes::Lower, Some(value)).unwrap();
    }
    machine.bus.advance_machine(100_019).unwrap();
}

#[test]
fn granted_main_aperture_lanes_and_4000_c000_aliases() {
    for region in [Region::NtscUs, Region::Pal] {
        for concurrent in [false, true] {
            let mut machine = fixture(region, concurrent, &[0x4e71, 0x4e71]);
            grant(&mut machine);
            for base in [0xa04000, 0xa0c000] {
                transfer(&mut machine, base, ByteLanes::Both, Some(0x2a99)).unwrap();
                assert_eq!(machine.opn2_snapshot().address, 0x2a);
                transfer(&mut machine, base + 1, ByteLanes::Lower, Some(0x0055)).unwrap();
                assert_eq!(machine.opn2_snapshot().dac, 0x55);
                transfer(&mut machine, base, ByteLanes::Upper, Some(0x2b00)).unwrap();
                transfer(&mut machine, base + 1, ByteLanes::Lower, Some(0x0080)).unwrap();
                assert!(machine.opn2_snapshot().dac_enabled);
                assert_eq!(
                    transfer(&mut machine, base, ByteLanes::Both, None),
                    Ok(0x8080)
                );
                assert_eq!(
                    transfer(&mut machine, base + 1, ByteLanes::Lower, None),
                    Ok(0)
                );
                transfer(&mut machine, base + 2, ByteLanes::Both, Some(0xb600)).unwrap();
                transfer(&mut machine, base + 3, ByteLanes::Lower, Some(0x00c0)).unwrap();
                assert_eq!(machine.opn2_snapshot().address, 0x1b6);
                assert_eq!(
                    machine.bus.audio.opn2.sample_with_mutes([false; 6]),
                    [-2976, -2976]
                );
            }
            machine.bus.sound.request(false);
            assert!(transfer(&mut machine, 0xa04000, ByteLanes::Upper, Some(0x2a00)).is_err());
        }
    }
}

#[test]
fn published_zres_resets_chip_and_preserves_psg_and_queued_audio() {
    for region in [Region::NtscUs, Region::Pal] {
        for concurrent in [false, true] {
            let mut machine = fixture(region, concurrent, &[0x4e71, 0x4e71]);
            sounding(&mut machine);
            let mut expected = machine.bus.audio.clone();
            let mut prefix = machine.clone();
            let queued = prefix.drain_audio_samples();
            assert!(!queued.is_empty());
            let now = machine.bus.now.get();
            assert!(transfer(&mut machine, 0xa11200, ByteLanes::Lower, Some(0)).is_err());
            assert!(machine.opn2_snapshot().dac_enabled);
            expected.advance_to(now + 28);
            transfer(&mut machine, 0xa11200, ByteLanes::Both, Some(0)).unwrap();
            expected.reset_opn2(now + 56);
            assert_eq!(machine.bus.audio, expected);
            assert_eq!(machine.bus.audio.opn2, Opn2::default());
            assert!(machine.sound_reset_asserted());
            assert!(machine.drain_audio_samples().starts_with(&queued));
        }
    }
}

#[test]
fn debugger_sound_cpu_reset_preserves_the_chip_and_audio_timeline() {
    for region in [Region::NtscUs, Region::Pal] {
        for concurrent in [false, true] {
            let mut machine = fixture(region, concurrent, &[0x4e71, 0x4e71]);
            sounding(&mut machine);
            let audio = machine.bus.audio.clone();
            let now = machine.bus.now;
            machine.reset_sound_cpu();
            assert_eq!(machine.bus.audio, audio);
            assert_eq!(machine.bus.now, now);
            assert!(machine.opn2_snapshot().dac_enabled);
        }
    }
}

#[test]
fn failed_main_restart_restores_the_complete_audio_ring() {
    for region in [Region::NtscUs, Region::Pal] {
        for concurrent in [false, true] {
            let mut machine = fixture(region, concurrent, &[0x4e71, 0x4e71]);
            sounding(&mut machine);
            machine.bus.cartridge.initial.pc = 0xc00010;
            let saved = machine.clone();
            assert!(machine.reset_cpu().is_err());
            assert_eq!(machine, saved);
            let mut saved = saved;
            assert_eq!(machine.drain_audio_samples(), saved.drain_audio_samples());
        }
    }
}

#[test]
fn failed_concurrent_prime_rolls_back_a_published_dac_write() {
    for region in [Region::NtscUs, Region::Pal] {
        let mut machine = fixture(region, true, &[0x4e71, 0x4e71]);
        machine.bus.sound.set_reset(false);
        machine.bus.sound.ram[..5].copy_from_slice(&[0x3e, 0x99, 0x32, 1, 0x40]);
        machine.bus.audio.write_opn2(56, 0, 0x2a);
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
            &mut Audio::new(region),
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
        assert_eq!(witness.opn2_snapshot().dac, 0x99);
        assert_eq!(machine.opn2_snapshot().dac, 0x80);
        assert!(machine.reset_cpu().is_err());
        assert_eq!(machine, saved);
    }
}

#[test]
fn frozen_main_fault_cannot_repeat_a_dac_write_or_advance_audio() {
    for region in [Region::NtscUs, Region::Pal] {
        for concurrent in [false, true] {
            let mut machine = fixture(
                region,
                concurrent,
                &[0x13fc, 0x0099, 0x00a0, 0x4001, 0x3010, 0x4e71, 0x4e71],
            );
            grant(&mut machine);
            machine.bus.audio.write_opn2(machine.bus.now.get(), 0, 0x2a);
            machine.cpu.registers.a[0] = 0xc00010;
            machine.step_instruction().unwrap();
            assert_eq!(machine.opn2_snapshot().dac, 0x99);
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
        }
    }
}
