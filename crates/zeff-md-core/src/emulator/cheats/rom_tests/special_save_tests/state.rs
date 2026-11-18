use super::*;

fn callable_set(opcode: u8, data: u8) -> Vec<CheatPatch> {
    patches(&format!(
        "010100:70{opcode:02X}+010102:0640+010104:0003+010106:4E75+018000:{data:02X}"
    ))
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn admitted_expansion_and_split_restore_real_callee_samples_and_undo_with_current_b() {
    let caller = [
        0x13fc, 0x80, 0xc0, 0x11, 0x13fc, 0x10, 0xc0, 0x11, 0x13fc, 0x90, 0xc0, 0x11, 0x60fe,
        0x4e71,
    ];
    for profile in [Profile::Expansion, Profile::Split] {
        for concurrent in [false, true] {
            let mut source = machine(profile, &caller, concurrent);
            for _ in 0..3 {
                source.step_instruction().unwrap();
            }
            source.install_rom_patches(&callable_set(7, 0xa5)).unwrap();
            let undo = source.encode_debugger_checkpoint().unwrap();
            let old_queue = source.cpu.queued_instruction_inputs();
            let return_pc = source.registers().pc;
            let return_sp = source.registers().stack_pointer();
            let context = source.begin_debug_call(CODE).unwrap();
            let queued = source.cpu.queued_instruction_inputs().unwrap();
            assert_eq!(queued[0].value, 0x7007);
            assert_eq!(offsets(queued[0]), [Some(0x10100), Some(0x10101)]);
            let raw = source.encode_state().unwrap();
            let front = source.encode_frontend_state().unwrap();
            let owned = source.encode_debugger_checkpoint().unwrap();
            for format in 0..3 {
                let mut receiver = machine(profile, &caller, concurrent);
                receiver.step_instruction().unwrap();
                receiver
                    .install_rom_patches(&callable_set(9, 0x5a))
                    .unwrap();
                receiver.debug_write_ram(&[(0xff0300, 0x9a)]).unwrap();
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
                    _ => {
                        receiver.decode_debugger_checkpoint(&owned).unwrap();
                        assert_eq!(receiver.encode_debugger_checkpoint().unwrap(), owned);
                    }
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
                let before = receiver.clone();
                for bad in 0..3 {
                    let failed = match bad {
                        0 => receiver.decode_state(&raw[..raw.len() - 1]),
                        1 => receiver.decode_frontend_state(&front[..front.len() - 1]),
                        _ => receiver.decode_debugger_checkpoint(&owned[..owned.len() - 1]),
                    };
                    assert!(failed.is_err());
                    assert_eq!(receiver, before);
                }
                let mut reference = source.clone();
                reference
                    .install_rom_patches(&callable_set(9, 0x5a))
                    .unwrap();
                let mut capture = Capture::default();
                for _ in 0..3 {
                    receiver.step_instruction_traced(&mut capture).unwrap();
                    reference.step_instruction().unwrap();
                }
                assert!(receiver.debug_call_returned(&context));
                assert_eq!(
                    (
                        receiver.registers().pc,
                        receiver.registers().stack_pointer()
                    ),
                    (return_pc, return_sp)
                );
                assert_eq!(receiver.registers().d[0], 10);
                assert_eq!(capture.0[0].1.value, 0x7007);
                assert_eq!(
                    capture.0[0].1.origin,
                    if format == 2 {
                        queued[0].origin
                    } else {
                        [InstructionByteOrigin::UNKNOWN; 2]
                    }
                );
                assert_eq!(capture.0.last().unwrap().0, CODE + 6);
                assert_eq!(receiver.peek(CODE + 1), Some(9));
                assert_eq!(receiver.peek(DATA), Some(0x5a));
                assert_eq!(
                    receiver.encode_state().unwrap(),
                    reference.encode_state().unwrap()
                );
                if format != 0 {
                    receiver.step_field().unwrap();
                    reference.step_field().unwrap();
                    assert_eq!(receiver, reference);
                    let actual = receiver.drain_audio_samples();
                    let expected = reference.drain_audio_samples();
                    assert!(actual.iter().any(|sample| *sample != 0.0));
                    assert!(
                        actual
                            .iter()
                            .map(|sample| sample.to_bits())
                            .eq(expected.iter().map(|sample| sample.to_bits()))
                    );
                }
            }
            source.install_rom_patches(&callable_set(9, 0x5a)).unwrap();
            for _ in 0..3 {
                source.step_instruction().unwrap();
            }
            assert!(source.debug_call_returned(&context));
            source.finish_debug_call(context).unwrap();
            source.decode_debugger_checkpoint(&undo).unwrap();
            assert_eq!(source.cpu.queued_instruction_inputs(), old_queue);
            assert_eq!(source.peek(CODE + 1), Some(9));
            let context = source.begin_debug_call(CODE).unwrap();
            for _ in 0..3 {
                source.step_instruction().unwrap();
            }
            assert!(source.debug_call_returned(&context));
            source.finish_debug_call(context).unwrap();
            assert_eq!(source.registers().d[0], 12);
            let initial = source.bus.cartridge.initial;
            source
                .install_rom_patches(&patches("000004:8001+000006:0100+010100:700B+010102:0640"))
                .unwrap();
            source.reset();
            assert_eq!(source.bus.cartridge.initial, initial);
            assert_eq!(
                (source.registers().pc, source.registers().ssp),
                (0x80010100, 0x00fffffc)
            );
            assert_eq!(source.queued_words(), Some([0x700b, 0x0640]));
            source.step_instruction().unwrap();
            assert_eq!(source.registers().d[0], 11);
            source.clear_rom_patches();
            source.reset();
            assert_eq!(
                (source.registers().pc, source.registers().ssp),
                (0x200, 0x00fffffc)
            );
        }
    }
}
