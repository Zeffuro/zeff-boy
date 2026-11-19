use super::super::sound_tests::{advance_sound, pending_rom_read};
use super::*;

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn fixed_register_rom_sound_pending_state_keeps_owner_sampling_and_current_set() {
    for profile in PROFILES {
        if profile == 9 {
            consumer::sound();
            continue;
        }
        let mut donor = machine(profile, true, &[0x60fe, 0x4e71]);
        donor.bus.sound.set_reset(false);
        donor.bus.sound.bank = 3;
        donor.bus.sound.ram[..7].copy_from_slice(&[0x3a, 0, 0x80, 0x3a, 0, 0x80, 0x76]);
        donor.install_rom_patches(&set(7, 0xa5)).unwrap();
        assert_eq!(donor.bus.cartridge.mode_sensitive_read(DATA), profile == 9);
        pending_rom_read(&mut donor);
        let pending = donor.sound_schedule.pending.as_ref().unwrap().clone();
        assert_eq!(pending.cpu.regs().a, 0xa5);
        assert_ne!(donor.sound_cpu.regs().a, 0xa5);
        let deadline = donor.sound_deadline().unwrap().get();
        assert!(donor.bus.now.get() < deadline);
        let raw = donor.encode_state().unwrap();
        let front = donor.encode_frontend_state().unwrap();
        let owned = donor.encode_debugger_checkpoint().unwrap();
        for current in [Some(0xa5), Some(0x5a), None] {
            for format in 0..3 {
                let mut receiver = donor.clone();
                if let Some(value) = current {
                    receiver.install_rom_patches(&set(9, value)).unwrap();
                } else {
                    receiver.clear_rom_patches();
                }
                assert_eq!(receiver.sound_schedule.pending.as_ref(), Some(&pending));
                advance_sound(&mut receiver, deadline - donor.bus.now.get());
                assert_eq!(
                    receiver.sound_cpu.regs().a,
                    if profile == 9 {
                        current.unwrap_or(0x12)
                    } else {
                        0xa5
                    }
                );
                receiver.debug_write_ram(&[(0xff0300, 0x9a)]).unwrap();
                assert_ne!(receiver.encode_state().unwrap(), raw);
                let overlay = receiver.bus.cartridge.rom_overlay.clone();
                match format {
                    0 => {
                        receiver.decode_state(&raw).unwrap();
                        assert_eq!(receiver.encode_state().unwrap(), raw);
                    }
                    1 => {
                        receiver.decode_frontend_state(&front).unwrap();
                        assert_eq!(receiver.encode_frontend_state().unwrap(), front);
                    }
                    2 => {
                        receiver.decode_debugger_checkpoint(&owned).unwrap();
                        assert_eq!(receiver.encode_debugger_checkpoint().unwrap(), owned);
                    }
                    _ => unreachable!(),
                }
                assert_eq!(receiver.bus.cartridge.rom_overlay, overlay);
                assert_eq!(receiver.sound_schedule.pending.as_ref(), Some(&pending));
                assert_eq!(receiver.sound_deadline().unwrap().get(), deadline);
                let remaining = deadline - receiver.bus.now.get();
                advance_sound(&mut receiver, remaining - 1);
                assert_ne!(receiver.sound_cpu.regs().a, 0xa5);
                advance_sound(&mut receiver, 1);
                assert_eq!(
                    receiver.sound_cpu.regs().a,
                    if profile == 9 {
                        current.unwrap_or(0x12)
                    } else {
                        0xa5
                    }
                );
                pending_rom_read(&mut receiver);
                let remaining = receiver.sound_deadline().unwrap().get() - receiver.bus.now.get();
                advance_sound(&mut receiver, remaining);
                assert_eq!(receiver.sound_cpu.regs().a, current.unwrap_or(0x12));
            }
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn fixed_register_rom_dma_fifo_restore_keeps_sampled_word_and_next_word_uses_current_set() {
    use super::super::sound_tests::{assert_dma_fifo_retains_sampled_word, dma_fifo_program};
    for profile in PROFILES {
        let mut program = dma_fifo_program();
        if profile == 9 {
            let source = program.iter_mut().find(|word| **word == 0x96c0).unwrap();
            *source = 0x9680;
        }
        let mut machine = machine(profile, true, &program);
        if profile == 9 {
            machine
                .bus
                .cartridge
                .registers
                .as_mut()
                .unwrap()
                .write_bank(3);
        }
        if profile == 9 {
            consumer::dma(machine);
        } else {
            assert_dma_fifo_retains_sampled_word(machine, Region::NtscUs);
        }
    }
}
