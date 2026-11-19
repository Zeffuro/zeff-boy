use super::*;
use crate::hardware::sound::SoundFault;
use crate::hardware::sound::schedule::SoundSchedule;

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
fn large_sound_pending_data_restores_and_samples_receiver_set_at_retirement() {
    for _family in [Family::Extended] {
        for region in REGIONS {
            for page in [16, 17] {
                let mut machine = machine(&[0x60fe, 0x4e71], region, true);
                select(&mut machine, 1, page);
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
                        select(&mut receiver, 1, 1);
                        let overlay = receiver.bus.cartridge.rom_overlay.clone();
                        super::super::state::restore(&mut receiver, &raw, &front, frontend);
                        assert_eq!(receiver.bus.cartridge.rom_overlay, overlay);
                        assert_eq!(receiver.sound_schedule.pending.as_ref(), Some(&pending));
                        assert_eq!(
                            receiver.peek(WINDOW),
                            Some(current.unwrap_or(if page == 16 { 0x70 } else { 17 }))
                        );
                        retire(&mut receiver);
                        assert_eq!(receiver.sound_fault(), None);
                        assert_eq!(
                            receiver.sound_cpu.regs().a,
                            current.unwrap_or(if page == 16 { 0x70 } else { 17 })
                        );
                    }
                }
            }
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn large_sound_fetch_keeps_opcode_guard_and_refreshes_changed_operand() {
    for _family in [Family::Extended] {
        for region in REGIONS {
            let mut machine = machine(&[0x60fe, 0x4e71], region, true);
            select(&mut machine, 1, 16);
            machine
                .install_rom_patches(&patches("ROM:00800001:22"))
                .unwrap();
            arm(&mut machine, &[0xc3, 0, 0x80]);
            pending_read(&mut machine, 0x80000);
            assert!(machine.sound_schedule.state.next_segment_reads_opcode());
            let raw = machine.encode_state().unwrap();
            let front = machine.encode_frontend_state().unwrap();
            for (set, expected) in [
                ("ROM:00800001:22", 0x22),
                ("ROM:00800001:33", 0x33),
                ("", 0x11),
            ] {
                for frontend in [false, true] {
                    let mut receiver = machine.clone();
                    if set.is_empty() {
                        receiver.clear_rom_patches();
                    } else {
                        receiver.install_rom_patches(&patches(set)).unwrap();
                    }
                    super::super::state::restore(&mut receiver, &raw, &front, frontend);
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
                changed
                    .install_rom_patches(&patches("ROM:00800000:76"))
                    .unwrap();
                super::super::state::restore(&mut changed, &raw, &front, frontend);
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
