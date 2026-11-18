use super::*;
use crate::hardware::bus::ownership::{Requester, Reservation};

fn fixture_whole(region: Region, concurrent_sound: bool, unrelated: bool) -> Emulator {
    let base = fixture(region, concurrent_sound);
    let mut rom = base.rom_bytes().to_vec();
    rom[0x1000..0x1004].copy_from_slice(&[0x3a, 0, 0x40, 0x76]);
    let mut irom = vec![0; 1024];
    irom[0x3fc] = 0x400;
    let mut machine = Emulator::new_functional_svp(
        rom,
        LoadConfig {
            region,
            concurrent_sound,
        },
        irom,
        FunctionalProfile::new(0x507, 1, 3, [1, 1, 64, 2, 1, 1]).unwrap(),
    )
    .unwrap();
    if unrelated {
        machine.install_rom_patches(&[patch(0x1100, 0xa5)]).unwrap();
    }
    machine.bus.sound.set_reset(false);
    machine.bus.sound.ram[0x200] = 0x33;
    machine.sound_cpu = zeff_z80::Cpu::new_with_reset(zeff_z80::ResetState::new(0x9000, 0x1ffe));
    let now = machine.bus.now.get();
    let mut pending = crate::hardware::sound::schedule::PendingSound::prepare(
        &machine.sound_cpu,
        &machine.bus.sound,
        &machine.bus.cartridge,
        now,
    )
    .unwrap();
    pending
        .reserve_after(&[Reservation {
            requester: Requester::VDP,
            ticks: now..now + 300,
        }])
        .unwrap();
    machine.sound_schedule.pending = Some(pending);
    machine.bus.audio.write_opn2(now, 0, 0x2a);
    machine.bus.audio.write_opn2(now, 1, 0x80);
    machine
}

fn install(machine: &mut Emulator, kind: u8) {
    let patches = match kind {
        0 => vec![patch(0x1001, 1)],
        1 => vec![patch(0x1000, 0)],
        _ => vec![],
    };
    machine.install_rom_patches(&patches).unwrap();
}

fn refresh(machine: &mut Emulator) {
    machine
        .sound_schedule
        .pending
        .as_mut()
        .unwrap()
        .refresh_reads(
            &machine.sound_cpu,
            &machine.bus.sound,
            &machine.bus.cartridge,
            &machine.bus.audio,
        )
        .unwrap();
}

fn publish(machine: &mut Emulator) {
    let end = machine.sound_schedule.pending.as_ref().unwrap().deadline;
    advance(machine, end - machine.bus.now.get());
    if machine.concurrent_sound {
        assert!(machine.sound_schedule.pending.is_none());
        return;
    }
    let pending = machine.sound_schedule.pending.take().unwrap();
    machine.sound_schedule.state = pending.state;
    pending.publish(
        &mut machine.sound_cpu,
        &mut machine.bus.sound,
        &mut machine.bus.ram,
        &mut machine.bus.audio,
        &mut machine.bus.cartridge,
    );
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn whole_original_rom_peripheral_refresh_roundtrips_under_receiver_masks_all_wrappers() {
    for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
        for sound in [false, true] {
            for unrelated in [false, true] {
                let donor = fixture_whole(region, sound, unrelated);
                assert!(donor.sound_schedule.has_svp_history());
                let pending = donor.sound_schedule.pending.as_ref().unwrap();
                assert!(pending.has_read_replay());
                let expected = donor.bus.audio.opn2_status_at(pending.deadline);
                assert_ne!(expected & 0x80, 0);
                let interval = (pending.start, pending.deadline);
                let raw = donor.encode_state().unwrap();
                assert!(raw.windows(8).any(|part| part == b"ZMDSVPA2"));
                assert!(raw.windows(8).any(|part| part == b"ZSINST01"));
                for kind in 0..3 {
                    for wrapper in 0..3 {
                        let mut receiver = fixture_whole(region, sound, false);
                        install(&mut receiver, kind);
                        let overlay = receiver.bus.cartridge.rom_overlay.clone();
                        restore(&mut receiver, &donor, wrapper);
                        assert_eq!(receiver.bus.cartridge.rom_overlay, overlay);
                        assert_eq!(receiver.sound_schedule, donor.sound_schedule);
                        let mut reference = continuation_oracle(&donor, wrapper);
                        install(&mut reference, kind);
                        refresh(&mut receiver);
                        refresh(&mut reference);
                        let sampled = receiver.sound_schedule.pending.as_ref().unwrap();
                        assert_eq!((sampled.start, sampled.deadline), interval);
                        assert_eq!(sampled.cpu.regs().a, expected);
                        assert_eq!(sampled.instruction.opcode, 0x3a);
                        assert!(!sampled.has_read_replay());
                        assert_eq!(sampled.memory.ram[0x200], 0x33);
                        assert_eq!(receiver.sound_schedule, reference.sound_schedule);
                        let mut roundtrip = fixture_whole(region, sound, false);
                        install(&mut roundtrip, kind);
                        restore(&mut roundtrip, &receiver, wrapper);
                        assert_eq!(roundtrip.bus.cartridge.rom_overlay, overlay);
                        assert_eq!(roundtrip.sound_schedule, receiver.sound_schedule);
                        publish(&mut receiver);
                        publish(&mut reference);
                        assert_eq!(receiver.sound_cpu.regs().a, expected);
                        assert_eq!(receiver.sound_cpu.regs().pc, 0x9003);
                        assert_eq!(receiver.bus.sound.ram[0x200], 0x33);
                        assert!(
                            receiver.encode_frontend_state().unwrap()
                                == reference.encode_frontend_state().unwrap(),
                            "refresh continuation: {region:?} sound={sound} wrapper={wrapper} mask={kind}"
                        );
                    }
                }
            }
        }
    }
}
