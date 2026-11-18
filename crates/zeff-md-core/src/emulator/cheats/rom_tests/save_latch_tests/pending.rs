use super::super::sound_tests::advance_sound;
use super::*;

fn pending_read(machine: &mut Emulator, target: u32) {
    for _ in 0..1000 {
        advance_sound(machine, 1);
        assert_eq!(machine.sound_fault(), None);
        if machine
            .sound_schedule
            .pending
            .as_ref()
            .is_some_and(|pending| {
                pending.bank_access.is_some_and(|access| {
                    access.address == target && access.direction == zeff_m68k::bus::Direction::Read
                })
            })
        {
            return;
        }
    }
    panic!("banked ROM read not reached");
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn save_latch_rom_sound_pending_read_keeps_historical_a_and_next_read_uses_current_set() {
    for (bank, target) in [(3, 0x18000), (0x43, 0x218000)] {
        let mut donor = machine(true, &[0x60fe, 0x4e71]);
        donor.bus.sound.set_reset(false);
        donor.bus.sound.bank = bank;
        donor.bus.sound.ram[..7].copy_from_slice(&[0x3a, 0, 0x80, 0x3a, 0, 0x80, 0x76]);
        donor.install_rom_patches(&set(7, 0xa5)).unwrap();
        assert!(!donor.bus.cartridge.mode_sensitive_read(target));
        pending_read(&mut donor, target);
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
                assert_eq!(receiver.sound_cpu.regs().a, 0xa5);
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
                assert_eq!(receiver.sound_cpu.regs().a, 0xa5);
                pending_read(&mut receiver, target);
                let remaining = receiver.sound_deadline().unwrap().get() - receiver.bus.now.get();
                advance_sound(&mut receiver, remaining);
                assert_eq!(receiver.sound_cpu.regs().a, current.unwrap_or(0x12));
            }
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn save_latch_rom_dma_fifo_preserves_sampled_a_for_lower_and_upper_mirror() {
    use super::super::sound_tests::{assert_dma_fifo_retains_sampled_word, dma_fifo_program};
    for mirror in [false, true] {
        let mut program = dma_fifo_program();
        assert_eq!(program[25], 0x9700);
        if mirror {
            program[25] = 0x9710;
        }
        assert_dma_fifo_retains_sampled_word(machine(true, &program), Region::NtscUs);
    }
    for mode in [0, 1, 3] {
        for (text, rom_word) in [
            ("001800:A55A + 001802:B66B", [0xa5, 0x5a, 0xb6, 0x6b]),
            ("001800:C77C + 001802:D88D", [0xc7, 0x7c, 0xd8, 0x8d]),
        ] {
            let mut dma = dma_fifo_program();
            assert_eq!(dma[21], 0x96c0);
            assert_eq!(dma[25], 0x9700);
            dma[21] = 0x960c;
            dma[25] = 0x9710;
            let mut program = vec![
                0x13fc, 0x77, 0x0020, 0x1801, 0x13fc, 0x88, 0x0020, 0x1803, 0x13fc, mode, 0x00a1,
                0x30f0,
            ];
            program.extend_from_slice(&dma[..32]);
            program.extend([0x33fc, 0x0080, 0x00c0, 4, 0x60fe, 0x4e71]);
            let mut source = machine(true, &program);
            source.install_rom_patches(&patches(text)).unwrap();
            let expected = if mode == 0 {
                rom_word
            } else {
                [0xff, 0x77, 0xff, 0x88]
            };
            for _ in 0..256 {
                source.step_instruction().unwrap();
                if source.vdp().memory().vram()[0x400..0x404] == expected {
                    break;
                }
            }
            assert_eq!(&source.vdp().memory().vram()[0x400..0x404], &expected);
            assert_eq!(source.sound_fault(), None);
            assert_eq!(&source.bus.cartridge.bytes[0x1800..0x1804], &[0; 4]);
        }
    }
}
