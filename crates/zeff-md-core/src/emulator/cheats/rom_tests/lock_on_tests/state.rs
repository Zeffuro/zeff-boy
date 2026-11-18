use super::*;

fn code_set(value: Option<(u16, u16)>) -> Vec<CheatPatch> {
    let mut set = Vec::new();
    for base in [0x200000, 0x300000] {
        for (index, opcode) in [0x303c, 0, 0x3439, 0x0031, 0, 0x4e75, 0x4e71]
            .into_iter()
            .enumerate()
        {
            if index != 1 {
                word(&mut set, base + index as u32 * 2, opcode);
            }
        }
        if let Some((operand, data)) = value {
            word(&mut set, base + 2, operand);
            word(&mut set, base + 0x10000, data);
        }
    }
    set
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn lock_on_overlay_v1_restores_real_queue_and_keeps_receiver_set_and_late_failure_atomicity() {
    let caller = [
        0x13fc, 0x80, 0xc0, 0x11, 0x13fc, 0x10, 0xc0, 0x11, 0x13fc, 0x90, 0xc0, 0x11, 0x60fe,
        0x4e71,
    ];
    for region in REGIONS {
        for concurrent in [false, true] {
            for mode in [LockOn::MiB, LockOn::Patch, LockOn::TwoMiB] {
                let mut source = machine(mode, region, concurrent, &caller);
                for _ in 0..3 {
                    source.step_instruction().unwrap();
                }
                consumer::overlay(&mut source, &code_set(Some((0x1122, 0xa55a))));
                let return_pc = source.registers().pc;
                let return_sp = source.registers().stack_pointer();
                let _context = if mode == LockOn::TwoMiB {
                    consumer::begin(&mut source, 0x300000)
                } else {
                    source.begin_debug_call(0x300000).unwrap()
                };
                assert_eq!(source.queued_words(), Some([0x303c, 0x1122]));
                let raw = source.encode_state().unwrap();
                let front = source.encode_frontend_state().unwrap();
                assert_eq!(&raw[..12], b"ZMDRAW01\x01\0\0\0");
                assert_eq!(&front[..12], b"ZMDFRONT\x01\0\0\0");
                let original_data = if matches!(mode, LockOn::Patch | LockOn::TwoMiB) {
                    0x2c2c
                } else {
                    0x2828
                };
                for (current, expected) in [
                    (Some((0x1122, 0xa55a)), 0xa55a),
                    (Some((0x3344, 0x5aa5)), 0x5aa5),
                    (None, original_data),
                ] {
                    for frontend in [false, true] {
                        let mut receiver = source.clone();
                        receiver.step_instruction().unwrap();
                        receiver.debug_write_ram(&[(0xff0300, 0x9a)]).unwrap();
                        consumer::overlay(&mut receiver, &code_set(current));
                        assert_ne!(receiver.encode_state().unwrap(), raw);
                        let before = receiver.encode_frontend_state().unwrap();
                        for bad in [
                            [raw.as_slice(), &[0]].concat(),
                            raw[..raw.len() - 1].to_vec(),
                        ] {
                            assert!(receiver.decode_state(&bad).is_err());
                            assert_eq!(receiver.encode_frontend_state().unwrap(), before);
                        }
                        for bad in [
                            [front.as_slice(), &[0]].concat(),
                            front[..front.len() - 1].to_vec(),
                        ] {
                            assert!(receiver.decode_frontend_state(&bad).is_err());
                            assert_eq!(receiver.encode_frontend_state().unwrap(), before);
                        }
                        let operand = receiver.peek(0x300002);
                        if frontend {
                            receiver.decode_frontend_state(&front).unwrap();
                        } else {
                            receiver.decode_state(&raw).unwrap();
                        }
                        assert_eq!(receiver.encode_state().unwrap(), raw);
                        assert_eq!(receiver.queued_words(), Some([0x303c, 0x1122]));
                        assert_eq!(receiver.peek(0x300002), operand);
                        let mut reference = source.clone();
                        consumer::overlay(&mut reference, &code_set(current));
                        for _ in 0..3 {
                            receiver.step_instruction().unwrap();
                            reference.step_instruction().unwrap();
                        }
                        if mode == LockOn::TwoMiB {
                            consumer::return_to_caller(&mut receiver);
                            consumer::return_to_caller(&mut reference);
                        }
                        assert_eq!(receiver.registers().d[0] as u16, 0x1122);
                        assert_eq!(receiver.registers().d[2] as u16, expected);
                        assert_eq!(receiver.registers().pc, return_pc);
                        assert_eq!(receiver.registers().stack_pointer(), return_sp);
                        assert_eq!(
                            receiver.encode_state().unwrap(),
                            reference.encode_state().unwrap()
                        );
                        if frontend {
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
                }
            }
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn lock_on_guest_call_return_prefetch_follows_callee_control_change() {
    for region in REGIONS {
        for concurrent in [false, true] {
            for mode in [LockOn::MiB, LockOn::Patch] {
                let mut machine = machine(mode, region, concurrent, &[0x4ef9, 0x30, 0, 0x4e71]);
                let mut set = Vec::new();
                for (base, opcodes) in [(0x200000, [0x7001, 0x7202]), (0x300000, [0x7005, 0x7206])]
                {
                    word(&mut set, base, opcodes[0]);
                    word(&mut set, base + 2, opcodes[1]);
                }
                machine.install_rom_patches(&set).unwrap();
                machine.step_instruction().unwrap();
                assert_eq!(machine.registers().pc, 0x300000);
                let old = machine.queued_words();
                let value = u16::from(mode == LockOn::MiB);
                write_words(
                    &mut machine.bus.ram,
                    0x400,
                    &[0x13fc, value, 0xa1, 0x30f1, 0x4e75, 0x4e71],
                );
                let sp = machine.registers().stack_pointer();
                let context = machine.begin_debug_call(0xff0400).unwrap();
                for _ in 0..2 {
                    machine.step_instruction().unwrap();
                }
                assert!(machine.debug_call_returned(&context));
                machine.finish_debug_call(context).unwrap();
                assert_eq!(machine.registers().stack_pointer(), sp);
                assert_eq!(machine.registers().pc, 0x300000);
                assert_ne!(machine.queued_words(), old);
                let expected = if mode == LockOn::MiB {
                    [0x7005, 0x7206]
                } else {
                    [0x7001, 0x7202]
                };
                assert_eq!(machine.queued_words(), Some(expected));
                assert_eq!(
                    machine.bus.cartridge.lock_on,
                    Some(if mode == LockOn::MiB {
                        LockOn::Patch
                    } else {
                        LockOn::MiB
                    })
                );
                machine.step_instruction().unwrap();
                assert_eq!(
                    machine.registers().d[0],
                    if mode == LockOn::MiB { 5 } else { 1 }
                );
            }
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn lock_on_dma_keeps_accepted_word_while_later_sampling_uses_current_source_overlay() {
    for region in REGIONS {
        for mode in [LockOn::MiB, LockOn::Patch, LockOn::TwoMiB] {
            let mut program = Vec::new();
            for value in [
                0x8114, 0x8f02, 0x9302, 0x9400, 0x9500, 0x9600, 0x9718, 0x4400,
            ] {
                program.extend([0x33fc, value, 0xc0, 4]);
            }
            program.extend([0x60fe, 0x4e71]);
            let mut machine = machine(mode, region, true, &program);
            consumer::overlay(
                &mut machine,
                &patches("200000:A55A+200002:B66B+300000:A55A+300002:B66B"),
            );
            for _ in 0..8 {
                machine.step_instruction().unwrap();
            }
            machine
                .bus
                .vdp
                .write_control_from(
                    0x0080,
                    region,
                    VdpDmaSource::from_cartridge(&machine.bus.cartridge, &machine.bus.ram),
                )
                .unwrap();
            for _ in 0..2000 {
                if machine
                    .vdp()
                    .fifo()
                    .front()
                    .is_some_and(|entry| entry.data == 0xa55a)
                {
                    break;
                }
                advance(&mut machine, 1);
            }
            assert_eq!(machine.vdp().fifo().front().unwrap().data, 0xa55a);
            let queue = machine.queued_words();
            consumer::overlay(
                &mut machine,
                &patches("200000:C77C+200002:D88D+300000:C77C+300002:D88D"),
            );
            assert_eq!(machine.vdp().fifo().front().unwrap().data, 0xa55a);
            assert_eq!(machine.queued_words(), queue);
            for _ in 0..4000 {
                if machine.vdp().memory().vram()[0x400..0x404] == [0xa5, 0x5a, 0xd8, 0x8d] {
                    break;
                }
                advance(&mut machine, 1);
            }
            assert_eq!(
                &machine.vdp().memory().vram()[0x400..0x404],
                &[0xa5, 0x5a, 0xd8, 0x8d]
            );
        }
    }
}
