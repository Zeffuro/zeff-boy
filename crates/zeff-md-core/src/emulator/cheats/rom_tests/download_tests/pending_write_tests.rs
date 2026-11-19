use super::super::sound_tests::advance_sound;
use super::*;
use crate::hardware::bus::ownership::{Requester, Reservation};
use zeff_m68k::bus::Direction;

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn download_pending_sound_store_restores_hidden_backing_and_fourth_megabyte() {
    for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
        for address in [DATA, 0x300000] {
            for writable in [false, true] {
                let bytes =
                    tone_image(&[0x13fc, u16::from(!writable), 0xa1, 0x30f1, 0x60fe, 0x4e71]);
                let mut source = Emulator::new(
                    bytes,
                    LoadConfig {
                        region,
                        concurrent_sound: true,
                    },
                )
                .unwrap();
                source.set_sample_rate(48_000);
                for _ in 0..4 {
                    source.step_instruction().unwrap();
                }
                source.install_rom_patches(&[patch(DATA, 0x44)]).unwrap();
                source.bus.sound.set_reset(false);
                source.bus.sound.bank = (address >> 15) as u16;
                source.bus.sound.ram[..6].copy_from_slice(&[0x3e, 0x5a, 0x32, 0, 0x80, 0x76]);
                let mut reached = false;
                for _ in 0..1000 {
                    advance_sound(&mut source, 1);
                    assert_eq!(source.sound_fault(), None);
                    if source
                        .sound_schedule
                        .pending
                        .as_ref()
                        .is_some_and(|pending| {
                            pending.bank_access.is_some_and(|access| {
                                access.address == address && access.direction == Direction::Write
                            })
                        })
                    {
                        reached = true;
                        break;
                    }
                }
                assert!(reached);
                let prepared = source.sound_schedule.pending.as_mut().unwrap();
                assert_eq!(prepared.instruction.cycles, 3);
                assert_eq!(prepared.deadline - prepared.start, 45);
                let original_start = prepared.start;
                let original_deadline = prepared.deadline;
                prepared
                    .reserve_after(&[Reservation {
                        requester: Requester::VDP,
                        ticks: original_start..original_start + 90,
                    }])
                    .unwrap();
                assert_eq!(prepared.start, original_start + 90);
                assert_eq!(prepared.deadline, original_deadline + 90);
                assert_eq!(prepared.instruction.cycles, 9);
                let pending = prepared.clone();
                assert_eq!(pending.instruction.pc, 2);
                assert_eq!(source.sound_cpu.regs().a, 0x5a);
                assert_eq!(source.sound_cpu.instruction_bytes(), &[0x32, 0, 0x80]);
                assert_eq!(pending.bank_access.unwrap().value, 0x5a);
                let deadline = source.sound_deadline().unwrap().get();
                assert!(deadline > source.bus.now.get() + 1);
                let original = source.bus.cartridge.bytes.clone();
                let digest = source.bus.cartridge.rom_digest();
                let initial = if address == DATA { 0 } else { 0xff };
                assert_eq!(
                    source
                        .bus
                        .cartridge
                        .download
                        .as_ref()
                        .unwrap()
                        .read(address),
                    Some(initial)
                );
                let raw = source.encode_state().unwrap();
                let frontend = source.encode_frontend_state().unwrap();
                for current in [Some(0), Some(0x99), None] {
                    let mask = current.map_or_else(Vec::new, |value| vec![patch(DATA, value)]);
                    for restore in 0..3 {
                        let mut receiver = source.clone();
                        receiver.install_rom_patches(&mask).unwrap();
                        match restore {
                            0 => {}
                            1 => receiver.decode_state(&raw).unwrap(),
                            2 => {
                                receiver.decode_frontend_state(&frontend).unwrap();
                                assert!(receiver.encode_frontend_state().unwrap() == frontend);
                            }
                            _ => unreachable!(),
                        }
                        assert!(receiver.encode_state().unwrap() == raw);
                        assert_eq!(receiver.sound_schedule.pending.as_ref(), Some(&pending));
                        let cpu = receiver.sound_cpu.clone();
                        let memory = receiver.bus.sound.clone();
                        let backing = receiver.bus.cartridge.download.clone();
                        let remaining = deadline - receiver.bus.now.get();
                        advance_sound(&mut receiver, remaining - 1);
                        assert_eq!(receiver.sound_cpu, cpu);
                        assert_eq!(receiver.bus.sound, memory);
                        assert_eq!(receiver.bus.cartridge.download, backing);
                        advance_sound(&mut receiver, 1);
                        assert_eq!(receiver.sound_fault(), None);
                        assert_eq!(receiver.bus.now.get(), deadline);
                        assert_eq!(receiver.sound_cpu.regs().pc, 5);
                        let expected = if writable { 0x5a } else { initial };
                        assert_eq!(
                            receiver
                                .bus
                                .cartridge
                                .download
                                .as_ref()
                                .unwrap()
                                .read(address),
                            Some(expected)
                        );
                        assert_eq!(
                            receiver.peek(address),
                            Some(if address == DATA {
                                current.unwrap_or(expected)
                            } else {
                                expected
                            })
                        );
                        let mut oracle = receiver_oracle(&source, &mask, restore != 1);
                        advance_sound(&mut oracle, remaining);
                        same_continuation(&receiver, &oracle);
                        receiver.clear_rom_patches();
                        oracle.clear_rom_patches();
                        assert_eq!(receiver.peek(address), Some(expected));
                        assert!(Arc::ptr_eq(&original, &receiver.bus.cartridge.bytes));
                        assert_eq!(receiver.bus.cartridge.rom_digest(), digest);
                        future_field(&mut receiver, &mut oracle);
                    }
                }
            }
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn download_malformed_pending_sound_store_rejects_without_publication() {
    let mut source = Emulator::new(
        download_image(&[0x13fc, 0, 0xa1, 0x30f1, 0x60fe, 0x4e71]),
        LoadConfig {
            region: Region::NtscUs,
            concurrent_sound: true,
        },
    )
    .unwrap();
    source.step_instruction().unwrap();
    source.bus.sound.set_reset(false);
    source.bus.sound.bank = 3;
    source.bus.sound.ram[..6].copy_from_slice(&[0x3e, 0x5a, 0x32, 0, 0x80, 0x76]);
    for _ in 0..1000 {
        advance_sound(&mut source, 1);
        assert_eq!(source.sound_fault(), None);
        if source
            .sound_schedule
            .pending
            .as_ref()
            .is_some_and(|pending| {
                pending
                    .bank_access
                    .is_some_and(|access| access.direction == Direction::Write)
            })
        {
            break;
        }
    }
    assert!(
        source
            .sound_schedule
            .pending
            .as_ref()
            .unwrap()
            .bank_access
            .is_some_and(|access| access.address == DATA && access.value == 0x5a)
    );
    assert_eq!(
        source.bus.cartridge.download.as_ref().unwrap().read(DATA),
        Some(0)
    );
    for mode in 0..3 {
        let mut forged = source.clone();
        let pending = forged.sound_schedule.pending.as_mut().unwrap();
        match mode {
            0 => pending.bank_access.as_mut().unwrap().value = 0x99,
            1 => pending.bank_access.as_mut().unwrap().address += 1,
            2 => pending.bank_access = None,
            _ => unreachable!(),
        }
        let before = forged.clone();
        assert!(forged.encode_state().is_err());
        assert!(forged.encode_frontend_state().is_err());
        assert_eq!(forged, before);
        if mode != 2 {
            let cpu = forged.sound_cpu.clone();
            let memory = forged.bus.sound.clone();
            let cart = forged.bus.cartridge.clone();
            let state = forged.sound_schedule.state;
            let deadline = forged.sound_deadline().unwrap().get();
            let remaining = deadline - forged.bus.now.get();
            advance_sound(&mut forged, remaining);
            assert!(matches!(
                forged.sound_fault(),
                Some(crate::hardware::sound::SoundFault::UnsupportedAccess {
                    address: 2,
                    direction: Direction::Write,
                })
            ));
            assert_eq!(forged.sound_cpu, cpu);
            assert_eq!(forged.bus.sound, memory);
            assert_eq!(forged.bus.cartridge, cart);
            assert_eq!(forged.sound_schedule.state, state);
            assert_eq!(forged.bus.now.get(), deadline);
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn download_failed_internal_prime_rolls_back_a_retired_sound_store() {
    for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
        let mut source = Emulator::new(
            download_image(&[0x13fc, 0, 0xa1, 0x30f1, 0x60fe, 0x4e71]),
            LoadConfig {
                region,
                concurrent_sound: true,
            },
        )
        .unwrap();
        source.step_instruction().unwrap();
        source.bus.sound.set_reset(false);
        source.bus.sound.bank = 3;
        source.bus.sound.ram[..6].copy_from_slice(&[0x3e, 0x5a, 0x32, 0, 0x80, 0x76]);
        for _ in 0..1000 {
            advance_sound(&mut source, 1);
            assert_eq!(source.sound_fault(), None);
            if source
                .sound_schedule
                .pending
                .as_ref()
                .is_some_and(|pending| {
                    pending
                        .bank_access
                        .is_some_and(|access| access.direction == Direction::Write)
                })
            {
                break;
            }
        }
        let deadline = source.sound_deadline().unwrap().get();
        let remaining = deadline - source.bus.now.get();
        advance_sound(&mut source, remaining - 1);
        assert_eq!(
            source.bus.cartridge.download.as_ref().unwrap().read(DATA),
            Some(0)
        );
        let mut success = source.clone();
        success.restart_cpu().unwrap();
        assert_eq!(
            success.bus.cartridge.download.as_ref().unwrap().read(DATA),
            Some(0x5a)
        );
        source.bus.cartridge.initial.pc = 0x800000;
        let before = source.clone();
        let raw = source.encode_state().unwrap();
        let frontend = source.encode_frontend_state().unwrap();
        assert!(source.restart_cpu().is_err());
        assert_eq!(source, before);
        assert!(source.encode_state().unwrap() == raw);
        assert!(source.encode_frontend_state().unwrap() == frontend);
        assert_eq!(
            source.bus.cartridge.download.as_ref().unwrap().read(DATA),
            Some(0)
        );
        assert_eq!(source.sound_schedule, before.sound_schedule);
        assert_eq!(source.sound_cpu.instruction_bytes(), &[0x32, 0, 0x80]);
    }
}
