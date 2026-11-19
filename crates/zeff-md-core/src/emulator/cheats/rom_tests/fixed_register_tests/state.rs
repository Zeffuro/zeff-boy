use super::*;

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn fixed_register_rom_restores_saved_main_samples_and_only_owned_provenance_with_current_set() {
    let program = [0x4ef9, 1, 0x0100, 0x4e71];
    for profile in PROFILES {
        for concurrent in [false, true] {
            if profile == 9 {
                consumer::queued_main(concurrent);
                continue;
            }
            let mut donor = machine(profile, concurrent, &program);
            donor.install_rom_patches(&set(7, 0xa5)).unwrap();
            donor.step_instruction().unwrap();
            let queued = donor.cpu.queued_instruction_inputs().unwrap();
            assert_eq!(queued[0].value, 0x7007);
            assert_eq!(offsets(queued[0]), [Some(0x10100), Some(0x10101)]);
            let raw = donor.encode_state().unwrap();
            let front = donor.encode_frontend_state().unwrap();
            let owned = donor.encode_debugger_checkpoint().unwrap();
            assert_eq!(
                &raw[..12],
                &[b'Z', b'M', b'D', b'R', b'A', b'W', b'0', b'1', 1, 0, 0, 0]
            );
            assert_eq!(
                &front[..12],
                &[b'Z', b'M', b'D', b'F', b'R', b'O', b'N', b'T', 1, 0, 0, 0]
            );
            for current in [Some(9), None] {
                let mut retained = donor.clone();
                if let Some(value) = current {
                    retained.install_rom_patches(&set(value, 0x5a)).unwrap();
                } else {
                    retained.clear_rom_patches();
                }
                assert_eq!(retained.encode_frontend_state().unwrap(), front);
                assert_eq!(retained.cpu.queued_instruction_inputs().unwrap(), queued);
                let mut capture = Capture::default();
                retained.step_instruction_traced(&mut capture).unwrap();
                assert_eq!(capture.0, [(CODE, queued[0])]);
                assert_eq!(retained.registers().d[0], 7);
                for _ in 0..3 {
                    retained.step_instruction().unwrap();
                }
                assert_eq!(retained.registers().d[0], u32::from(current.unwrap_or(1)));
                for format in 0..3 {
                    let mut receiver = machine(profile, concurrent, &program);
                    if let Some(value) = current {
                        receiver.install_rom_patches(&set(value, 0x5a)).unwrap();
                    }
                    receiver.step_instruction().unwrap();
                    receiver.step_instruction().unwrap();
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
                    assert_eq!(receiver.queued_words(), Some([0x7007, 0x0640]));
                    if format == 2 {
                        assert_eq!(receiver.cpu.queued_instruction_inputs().unwrap(), queued);
                    } else {
                        assert!(
                            receiver
                                .cpu
                                .queued_instruction_inputs()
                                .unwrap()
                                .iter()
                                .all(|input| input.origin == [InstructionByteOrigin::UNKNOWN; 2])
                        );
                    }
                    receiver.debug_write_ram(&[(0xff0300, 0x5c)]).unwrap();
                    let before = receiver.clone();
                    let before_owned = receiver.encode_debugger_checkpoint().unwrap();
                    for bad in 0..3 {
                        let failed = match bad {
                            0 => receiver.decode_state(&raw[..raw.len() - 1]),
                            1 => receiver.decode_frontend_state(&front[..front.len() - 1]),
                            2 => receiver.decode_debugger_checkpoint(&owned[..owned.len() - 1]),
                            _ => unreachable!(),
                        };
                        assert!(failed.is_err());
                        assert_eq!(receiver, before);
                        assert_eq!(receiver.encode_debugger_checkpoint().unwrap(), before_owned);
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
                }
            }
            let initial = donor.bus.cartridge.initial;
            donor
                .install_rom_patches(&patches("000004:8001 + 000006:0100 + 010101:0B"))
                .unwrap();
            donor.reset();
            assert_eq!(donor.bus.cartridge.initial, initial);
            assert_eq!(donor.registers().pc, 0x80010100);
            assert_eq!(donor.registers().ssp, 0x00fffffc);
            assert_eq!(donor.queued_words(), Some([0x700b, 0x0640]));
            let mut capture = Capture::default();
            donor.step_instruction_traced(&mut capture).unwrap();
            assert_eq!(capture.0[0].0, 0x80010100);
            assert_eq!(offsets(capture.0[0].1), [Some(0x10100), Some(0x10101)]);
            assert_eq!(donor.registers().d[0], 11);
            donor.clear_rom_patches();
            donor.reset();
            assert_eq!(donor.registers().pc, 0x200);
            assert_eq!(donor.queued_words(), Some([0x4ef9, 1]));
        }
    }
}
