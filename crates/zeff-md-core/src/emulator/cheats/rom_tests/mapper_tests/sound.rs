use super::*;
use crate::hardware::audio::Audio;
use crate::hardware::bus::ownership::{Requester, Reservation};
use crate::hardware::sound::schedule::{SoundInterval, SoundSchedule};
use crate::hardware::sound::{SoundFault, SoundMemory};
use zeff_emu_common::save_state::{StateReader, StateWriter};

fn arm(machine: &mut Emulator, program: &[u8]) {
    machine.sound_cpu = zeff_z80::Cpu::new();
    machine.sound_schedule = SoundSchedule::default();
    machine.bus.sound.set_reset(false);
    machine.bus.sound.bank = 0x10;
    machine.bus.sound.ram[..program.len()].copy_from_slice(program);
}

fn pending_read(machine: &mut Emulator, address: u32) {
    for _ in 0..1000 {
        advance(machine, 1);
        assert_eq!(machine.sound_fault(), None);
        if machine
            .sound_schedule
            .pending
            .as_ref()
            .is_some_and(|pending| {
                pending.bank_access.is_some_and(|access| {
                    access.address == address && access.direction == Direction::Read
                })
            })
        {
            return;
        }
    }
    panic!("mapped SOUND read not reached: {address:06x}");
}

fn retire(machine: &mut Emulator) {
    let remaining = machine.sound_deadline().unwrap().get() - machine.bus.now.get();
    assert!(remaining != 0);
    advance(machine, remaining);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn mapper_sound_pending_data_restores_and_samples_receiver_set_at_retirement() {
    for family in FAMILIES {
        for region in REGIONS {
            for page in [8, 9] {
                let mut machine = mapper(family, &[0x60fe, 0x4e71], region, true);
                select(&mut machine, family, 1, page);
                machine
                    .install_rom_patches(&[patch(source(page, WITHIN), 0xa5)])
                    .unwrap();
                arm(&mut machine, &[0x3a, 0, 0x90, 0x76]);
                pending_read(&mut machine, WINDOW);
                let pending = machine.sound_schedule.pending.as_ref().unwrap().clone();
                assert_eq!(pending.cpu.regs().a, 0xa5);
                assert_eq!(machine.sound_cpu.regs().a, 0);
                let raw = machine.encode_state().unwrap();
                let front = machine.encode_frontend_state().unwrap();
                for current in [Some(0xa5), Some(0x5a), None] {
                    for frontend in [false, true] {
                        let mut receiver = machine.clone();
                        receiver.clear_rom_patches();
                        if let Some(value) = current {
                            receiver
                                .install_rom_patches(&[patch(source(page, WITHIN), value)])
                                .unwrap();
                        }
                        select(&mut receiver, family, 1, 1);
                        let overlay = receiver.bus.cartridge.rom_overlay.clone();
                        state::restore(&mut receiver, &raw, &front, frontend);
                        assert_eq!(receiver.bus.cartridge.rom_overlay, overlay);
                        assert_eq!(receiver.sound_schedule.pending.as_ref(), Some(&pending));
                        assert_eq!(receiver.peek(WINDOW), Some(current.unwrap_or(page)));
                        retire(&mut receiver);
                        assert_eq!(receiver.sound_fault(), None);
                        assert_eq!(receiver.sound_cpu.regs().a, current.unwrap_or(page));
                    }
                }
            }
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn mapper_sound_fetch_keeps_opcode_guard_and_refreshes_changed_operand() {
    for family in FAMILIES {
        for region in REGIONS {
            let mut machine = mapper(family, &[0x60fe, 0x4e71], region, true);
            replace_page(&mut machine, family, 8, &[0x3e, 0x11, 0x76]);
            select(&mut machine, family, 1, 8);
            machine.install_rom_patches(&patches("400001:22")).unwrap();
            arm(&mut machine, &[0xc3, 0, 0x80]);
            pending_read(&mut machine, 0x80000);
            assert!(machine.sound_schedule.state.next_segment_reads_opcode());
            let raw = machine.encode_state().unwrap();
            let front = machine.encode_frontend_state().unwrap();
            for (set, expected) in [("400001:22", 0x22), ("400001:33", 0x33), ("", 0x11)] {
                for frontend in [false, true] {
                    let mut receiver = machine.clone();
                    if set.is_empty() {
                        receiver.clear_rom_patches();
                    } else {
                        receiver.install_rom_patches(&patches(set)).unwrap();
                    }
                    state::restore(&mut receiver, &raw, &front, frontend);
                    retire(&mut receiver);
                    assert_eq!(receiver.sound_fault(), None);
                    pending_read(&mut receiver, 0x80001);
                    retire(&mut receiver);
                    assert_eq!(receiver.sound_fault(), None);
                    assert_eq!(receiver.sound_cpu.regs().a, expected);
                }
            }
            for frontend in [false, true] {
                let mut changed = machine.clone();
                changed.install_rom_patches(&patches("400000:76")).unwrap();
                state::restore(&mut changed, &raw, &front, frontend);
                let cpu = changed.sound_cpu.clone();
                retire(&mut changed);
                assert!(matches!(
                    changed.sound_fault(),
                    Some(SoundFault::UnstableCartridgeRead { .. })
                ));
                assert_eq!(changed.sound_cpu, cpu);
            }
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn mapper_sound_reserved_data_refresh_survives_main_vdp_wait_and_schedule_restore() {
    for family in FAMILIES {
        for requester in [Requester::MAIN, Requester::VDP] {
            let mut machine = mapper(family, &[0x60fe, 0x4e71], Region::NtscUs, true);
            select(&mut machine, family, 1, 8);
            machine.install_rom_patches(&patches("401000:A5")).unwrap();
            let mut memory = SoundMemory::default();
            memory.set_reset(false);
            memory.bank = 0x10;
            memory.ram[..4].copy_from_slice(&[0x3a, 0, 0x90, 0x76]);
            let mut cpu = zeff_z80::Cpu::new();
            let mut schedule = SoundSchedule::default();
            let mut audio = Audio::default();
            schedule.advance(
                &mut cpu,
                &mut memory,
                &mut machine.bus.cartridge,
                &mut [],
                SoundInterval::new(0..151, &mut audio),
            );
            assert_eq!(schedule.fault, None);
            assert_eq!(cpu.regs().a, 0);
            let occupied = [Reservation {
                requester,
                ticks: 150..1500,
            }];
            schedule.advance(
                &mut cpu,
                &mut memory,
                &mut machine.bus.cartridge,
                &mut [],
                SoundInterval {
                    ticks: 151..1544,
                    occupied: &occupied,
                    audio: &mut audio,
                    sound_irq: None,
                },
            );
            assert_eq!(cpu.regs().a, 0);
            machine.install_rom_patches(&patches("401000:5A")).unwrap();
            let mut writer = StateWriter::new();
            schedule.write_state(&cpu, &mut writer).unwrap();
            let mut restored = SoundSchedule::default();
            let mut restored_cpu = zeff_z80::Cpu::new();
            restored
                .read_state(
                    &mut restored_cpu,
                    &mut StateReader::new(&writer.into_bytes()),
                )
                .unwrap();
            restored.validate_cartridge(&machine.bus.cartridge).unwrap();
            let mut restored_memory = memory.clone();
            let mut restored_cart = machine.bus.cartridge.clone();
            let mut restored_audio = audio.clone();
            schedule.advance(
                &mut cpu,
                &mut memory,
                &mut machine.bus.cartridge,
                &mut [],
                SoundInterval::new(1544..1545, &mut audio),
            );
            restored.advance(
                &mut restored_cpu,
                &mut restored_memory,
                &mut restored_cart,
                &mut [],
                SoundInterval::new(1544..1545, &mut restored_audio),
            );
            assert_eq!(schedule.fault, None);
            assert_eq!(restored.fault, None);
            assert_eq!(cpu.regs().a, 0x5a);
            assert_eq!(cpu, restored_cpu);
            assert_eq!(memory, restored_memory);
            assert!(schedule == restored);
        }
    }
}
