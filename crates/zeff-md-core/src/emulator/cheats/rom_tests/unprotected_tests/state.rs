use super::super::sound_tests::{advance_sound, pending_rom_read};
use super::*;

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn unprotected_public_import_restores_hardware_queue_but_not_historical_source_or_external_set() {
    for time in PROFILES {
        for (timing, concurrent) in ROUTES {
            let mut donor = machine(time, timing, concurrent, &[0x4ef9, 1, 0x0100, 0x4e71]);
            donor.install_rom_patches(&set(7, 0xa5)).unwrap();
            donor.step_instruction().unwrap();
            let raw = donor.encode_state().unwrap();
            let front = donor.encode_frontend_state().unwrap();
            assert_eq!(
                &raw[..12],
                &[b'Z', b'M', b'D', b'R', b'A', b'W', b'0', b'1', 1, 0, 0, 0]
            );
            assert_eq!(
                &front[..12],
                &[b'Z', b'M', b'D', b'F', b'R', b'O', b'N', b'T', 1, 0, 0, 0]
            );
            for current in [Some(9), None] {
                for frontend in [false, true] {
                    let mut receiver =
                        machine(time, timing, concurrent, &[0x4ef9, 1, 0x0100, 0x4e71]);
                    if let Some(value) = current {
                        receiver.install_rom_patches(&set(value, 0x5a)).unwrap();
                    }
                    receiver.step_instruction().unwrap();
                    receiver.step_instruction().unwrap();
                    receiver.debug_write_ram(&[(0xff0300, 0x9a)]).unwrap();
                    assert_ne!(receiver.encode_state().unwrap(), raw);
                    let overlay = receiver.bus.cartridge.rom_overlay.clone();
                    if frontend {
                        receiver.decode_frontend_state(&front).unwrap();
                        assert_eq!(receiver.encode_frontend_state().unwrap(), front);
                    } else {
                        receiver.decode_state(&raw).unwrap();
                        assert_eq!(receiver.encode_state().unwrap(), raw);
                    }
                    assert_eq!(receiver.bus.cartridge.rom_overlay, overlay);
                    assert_eq!(receiver.queued_words(), Some([0x7007, 0x0640]));
                    for input in receiver.cpu.queued_instruction_inputs().unwrap() {
                        assert_eq!(input.origin, [InstructionByteOrigin::UNKNOWN; 2]);
                    }
                    receiver.step_instruction().unwrap();
                    assert_eq!(receiver.registers().d[0], 7);
                    receiver.step_instruction().unwrap();
                    assert_eq!(receiver.registers().d[0], 10);
                    receiver.step_instruction().unwrap();
                    receiver.step_instruction().unwrap();
                    assert_eq!(receiver.registers().d[0], u32::from(current.unwrap_or(1)));
                    assert_eq!(
                        receiver.peek(DATA),
                        Some(if current.is_some() { 0x5a } else { 0x12 })
                    );
                    let before = receiver.clone();
                    let inputs = receiver.cpu.queued_instruction_inputs();
                    assert!(receiver.decode_state(&raw[..raw.len() - 1]).is_err());
                    assert!(
                        receiver
                            .decode_frontend_state(&front[..front.len() - 1])
                            .is_err()
                    );
                    assert_eq!(receiver, before);
                    assert_eq!(receiver.cpu.queued_instruction_inputs(), inputs);
                }
            }
            let mut different = donor.rom_bytes().to_vec();
            different[DATA as usize] ^= 1;
            let mut receiver = Emulator::new_with_timing(
                different,
                LoadConfig {
                    region: Region::NtscUs,
                    concurrent_sound: concurrent,
                },
                timing,
            )
            .unwrap();
            receiver.install_rom_patches(&set(9, 0x5a)).unwrap();
            let before = receiver.clone();
            let inputs = receiver.cpu.queued_instruction_inputs();
            assert!(receiver.decode_state(&raw).is_err());
            assert!(receiver.decode_frontend_state(&front).is_err());
            assert_eq!(receiver, before);
            assert_eq!(receiver.cpu.queued_instruction_inputs(), inputs);
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn unprotected_sound_prepared_rom_byte_restores_before_new_reads_take_current_set() {
    for time in PROFILES {
        let mut donor = machine(time, TimingModel::Standard, true, &[0x60fe, 0x4e71]);
        donor.bus.sound.set_reset(false);
        donor.bus.sound.bank = 3;
        donor.bus.sound.ram[..7].copy_from_slice(&[0x3a, 0, 0x80, 0x3a, 0, 0x80, 0x76]);
        donor.install_rom_patches(&set(7, 0xa5)).unwrap();
        pending_rom_read(&mut donor);
        let pending = donor.sound_schedule.pending.as_ref().unwrap().clone();
        assert_eq!(pending.cpu.regs().a, 0xa5);
        assert_ne!(donor.sound_cpu.regs().a, 0xa5);
        let deadline = donor.sound_deadline().unwrap().get();
        assert!(donor.bus.now.get() < deadline);
        let raw = donor.encode_state().unwrap();
        let front = donor.encode_frontend_state().unwrap();
        for current in [Some(0x5a), None] {
            for frontend in [false, true] {
                let mut receiver = donor.clone();
                if let Some(value) = current {
                    receiver.install_rom_patches(&set(9, value)).unwrap();
                } else {
                    receiver.clear_rom_patches();
                }
                advance_sound(&mut receiver, deadline - donor.bus.now.get());
                assert_eq!(receiver.sound_cpu.regs().a, 0xa5);
                receiver.debug_write_ram(&[(0xff0300, 0x9a)]).unwrap();
                assert_ne!(receiver.encode_state().unwrap(), raw);
                let overlay = receiver.bus.cartridge.rom_overlay.clone();
                if frontend {
                    receiver.decode_frontend_state(&front).unwrap();
                    assert_eq!(receiver.encode_frontend_state().unwrap(), front);
                } else {
                    receiver.decode_state(&raw).unwrap();
                    assert_eq!(receiver.encode_state().unwrap(), raw);
                }
                assert_eq!(receiver.bus.cartridge.rom_overlay, overlay);
                assert_eq!(receiver.sound_schedule.pending.as_ref().unwrap(), &pending);
                assert_eq!(receiver.sound_deadline().unwrap().get(), deadline);
                let remaining = deadline - receiver.bus.now.get();
                advance_sound(&mut receiver, remaining - 1);
                assert_ne!(receiver.sound_cpu.regs().a, 0xa5);
                advance_sound(&mut receiver, 1);
                assert_eq!(receiver.sound_cpu.regs().a, 0xa5);
                pending_rom_read(&mut receiver);
                let deadline = receiver.sound_deadline().unwrap().get();
                let remaining = deadline - receiver.bus.now.get();
                advance_sound(&mut receiver, remaining);
                assert_eq!(receiver.sound_cpu.regs().a, current.unwrap_or(0x12));
            }
        }
    }
}
