use super::*;
use crate::hardware::audio::Audio;
use crate::hardware::bus::ownership::{Requester, Reservation};
use crate::hardware::sound::schedule::{SoundInterval, SoundSchedule};
use crate::hardware::sound::{SoundFault, SoundMemory};
use zeff_emu_common::save_state::{StateReader, StateWriter};

fn arm(m: &mut Emulator, target: u32, code: &[u8]) {
    m.sound_cpu = zeff_z80::Cpu::new();
    m.sound_schedule = SoundSchedule::default();
    m.bus.sound.set_reset(false);
    m.bus.sound.bank = (target >> 15) as u16;
    m.bus.sound.ram[..code.len()].copy_from_slice(code);
}

fn pending(m: &mut Emulator, target: u32) {
    for _ in 0..1000 {
        advance(m, 1);
        assert_eq!(m.sound_fault(), None);
        if m.sound_schedule.pending.as_ref().is_some_and(|p| {
            p.bank_access
                .is_some_and(|a| a.address == target && a.direction == Direction::Read)
        }) {
            return;
        }
    }
    panic!("SOUND read not reached: {target:x}");
}

fn retire(m: &mut Emulator) {
    let remaining = m.sound_deadline().unwrap().get() - m.bus.now.get();
    assert!(remaining > 0);
    advance(m, remaining);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn save_and_sf004_pending_sound_data_refreshes_receiver_overlay_at_deadline() {
    for profile in [Profile::Sf004, Profile::Switched8] {
        for region in REGIONS {
            let sf004 = matches!(profile, Profile::Sf004);
            let target = if sf004 { 0x1000 } else { PORT };
            let source = if sf004 { BANK_SOURCE } else { PORT };
            let window = (0x8000 | (target & 0x7fff)) as u16;
            let [low, high] = window.to_le_bytes();
            let mut m = machine(profile, &[0x60fe], region, true);
            if sf004 {
                select_sf004(&mut m, 6, false);
            }
            m.install_rom_patches(&[patch(source, 0xa5)]).unwrap();
            arm(&mut m, target, &[0x3a, low, high, 0x76]);
            pending(&mut m, target);
            let prepared = m.sound_schedule.pending.as_ref().unwrap().clone();
            assert_eq!(prepared.cpu.regs().a, 0xa5);
            assert_eq!(m.sound_cpu.regs().a, 0);
            let raw = m.encode_state().unwrap();
            let front = m.encode_frontend_state().unwrap();
            let original = m.bus.cartridge.bytes[source as usize];
            for current in [Some(0xa5), Some(0x5a), None] {
                for frontend in [false, true] {
                    let mut receiver = m.clone();
                    receiver.clear_rom_patches();
                    if let Some(value) = current {
                        receiver
                            .install_rom_patches(&[patch(source, value)])
                            .unwrap();
                    }
                    restore(&mut receiver, &raw, &front, frontend);
                    assert_eq!(receiver.sound_schedule.pending.as_ref(), Some(&prepared));
                    let remaining =
                        receiver.sound_deadline().unwrap().get() - receiver.bus.now.get();
                    advance(&mut receiver, remaining - 1);
                    assert_eq!(receiver.sound_cpu.regs().a, 0);
                    advance(&mut receiver, 1);
                    assert_eq!(receiver.sound_fault(), None);
                    assert_eq!(receiver.sound_cpu.regs().a, current.unwrap_or(original));
                }
            }
            if !sf004 {
                let mut hidden = m.clone();
                hidden.bus.cartridge.save.as_mut().unwrap().enabled = true;
                hidden.bus.cartridge.save.as_mut().unwrap().bytes[0] = 0xc7;
                retire(&mut hidden);
                assert_eq!(hidden.sound_fault(), None);
                assert_eq!(hidden.sound_cpu.regs().a, 0xc7);
            }
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn ordinary_eeprom_rom_sound_read_keeps_historical_sample_until_next_read() {
    let mut m = machine(Profile::Serial128, &[0x60fe], Region::NtscUs, true);
    m.install_rom_patches(&[patch(0x1000, 0xa5)]).unwrap();
    arm(&mut m, 0x1000, &[0x3a, 0, 0x90, 0x3a, 0, 0x90, 0x76]);
    pending(&mut m, 0x1000);
    let raw = m.encode_state().unwrap();
    let front = m.encode_frontend_state().unwrap();
    for current in [Some(0xa5), Some(0x5a), None] {
        for frontend in [false, true] {
            let mut receiver = m.clone();
            receiver.clear_rom_patches();
            if let Some(value) = current {
                receiver
                    .install_rom_patches(&[patch(0x1000, value)])
                    .unwrap();
            }
            restore(&mut receiver, &raw, &front, frontend);
            retire(&mut receiver);
            assert_eq!(receiver.sound_cpu.regs().a, 0xa5);
            pending(&mut receiver, 0x1000);
            retire(&mut receiver);
            assert_eq!(receiver.sound_cpu.regs().a, current.unwrap_or(0x70));
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn sf004_sound_opcode_change_faults_and_operand_change_refreshes() {
    for region in REGIONS {
        let mut rom = save_image(Profile::Sf004);
        rom[BANK_SOURCE as usize..BANK_SOURCE as usize + 3].copy_from_slice(&[0x3e, 0x11, 0x76]);
        let mut m = Emulator::new(rom, config(region, true)).unwrap();
        select_sf004(&mut m, 6, false);
        m.install_rom_patches(&[patch(BANK_SOURCE + 1, 0x22)])
            .unwrap();
        arm(&mut m, 0x1000, &[0xc3, 0, 0x90]);
        pending(&mut m, 0x1000);
        assert!(m.sound_schedule.state.next_segment_reads_opcode());
        let raw = m.encode_state().unwrap();
        let front = m.encode_frontend_state().unwrap();
        for current in [Some(0x22), Some(0x33), None] {
            for frontend in [false, true] {
                let mut receiver = m.clone();
                receiver.clear_rom_patches();
                if let Some(value) = current {
                    receiver
                        .install_rom_patches(&[patch(BANK_SOURCE + 1, value)])
                        .unwrap();
                }
                restore(&mut receiver, &raw, &front, frontend);
                retire(&mut receiver);
                assert_eq!(receiver.sound_fault(), None);
                pending(&mut receiver, 0x1001);
                retire(&mut receiver);
                assert_eq!(receiver.sound_cpu.regs().a, current.unwrap_or(0x11));
            }
        }
        for frontend in [false, true] {
            let mut changed = m.clone();
            changed
                .install_rom_patches(&[patch(BANK_SOURCE, 0x76)])
                .unwrap();
            restore(&mut changed, &raw, &front, frontend);
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

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn save_and_sf004_sound_reservation_restore_keeps_current_overlay() {
    for profile in [Profile::Sf004, Profile::Switched8] {
        for requester in [Requester::MAIN, Requester::VDP] {
            let sf004 = matches!(profile, Profile::Sf004);
            let target = if sf004 { 0x1000 } else { PORT };
            let source = if sf004 { BANK_SOURCE } else { PORT };
            let [low, high] = (0x8000 | (target & 0x7fff) as u16).to_le_bytes();
            let mut m = machine(profile, &[0x60fe], Region::NtscUs, true);
            if sf004 {
                select_sf004(&mut m, 6, false);
            }
            m.install_rom_patches(&[patch(source, 0xa5)]).unwrap();
            let mut memory = SoundMemory::default();
            memory.set_reset(false);
            memory.bank = (target >> 15) as u16;
            memory.ram[..4].copy_from_slice(&[0x3a, low, high, 0x76]);
            let mut cpu = zeff_z80::Cpu::new();
            let mut schedule = SoundSchedule::default();
            let mut audio = Audio::default();
            schedule.advance(
                &mut cpu,
                &mut memory,
                &mut m.bus.cartridge,
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
                &mut m.bus.cartridge,
                &mut [],
                SoundInterval {
                    ticks: 151..1544,
                    occupied: &occupied,
                    audio: &mut audio,
                    sound_irq: None,
                },
            );
            assert_eq!(cpu.regs().a, 0);
            m.install_rom_patches(&[patch(source, 0x5a)]).unwrap();
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
            restored.validate_cartridge(&m.bus.cartridge).unwrap();
            let mut restored_memory = memory.clone();
            let mut restored_cart = m.bus.cartridge.clone();
            let mut restored_audio = audio.clone();
            schedule.advance(
                &mut cpu,
                &mut memory,
                &mut m.bus.cartridge,
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
