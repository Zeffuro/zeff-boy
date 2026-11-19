use super::*;
use crate::emulator::state::tests::layout;
use zeff_emu_common::save_state::StateReader;

fn corrupt(raw: &[u8], frontend: &[u8], front: bool, case: usize) -> Vec<u8> {
    let presence = layout::download_offset(raw);
    assert_eq!(&raw[presence..presence + 6], &[1, 1, 0, 0, 0x40, 0]);
    let mut bad = if front {
        frontend.to_vec()
    } else {
        raw.to_vec()
    };
    let core = if front {
        let mut reader = StateReader::new(frontend);
        reader.set_position(12);
        let at = reader.position() + 4;
        assert_eq!(
            reader
                .read_slice(crate::constants::MAX_STATE_BYTES)
                .unwrap(),
            raw
        );
        at
    } else {
        0
    };
    match case {
        0 => {
            bad.pop().unwrap();
        }
        1 => bad.push(0),
        2 => bad[core + 12] ^= 1,
        3 => bad[core + 44] ^= 1,
        4 => bad[core + presence] = 0,
        5 => bad[core + presence + 2] ^= 1,
        6 => bad[core + presence + 1] = 2,
        7 => bad[core + presence] = 2,
        8 => bad[core + 8..core + 12]
            .copy_from_slice(&(crate::save_state::STATE_VERSION + 1).to_le_bytes()),
        _ => unreachable!(),
    }
    bad
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn download_masked_raw_and_frontend_rejections_preserve_machine_and_future_pcm() {
    let bytes = backing_image();
    for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
        for concurrent_sound in [false, true] {
            let source = backing_source(
                &bytes,
                LoadConfig {
                    region,
                    concurrent_sound,
                },
            );
            let raw = source.encode_state().unwrap();
            let frontend = source.encode_frontend_state().unwrap();
            for mask in [
                vec![patch(0x1234, 0x77), patch(0x1235, 0x88)],
                vec![patch(0x1234, 0x67), patch(0x1235, 0x80)],
                vec![],
            ] {
                for front in [false, true] {
                    let mut receiver = source.clone();
                    for _ in 0..4 {
                        receiver.step_instruction().unwrap();
                    }
                    receiver.install_rom_patches(&mask).unwrap();
                    assert_ne!(
                        receiver.bus.cartridge.download,
                        source.bus.cartridge.download
                    );
                    let before = receiver.clone();
                    let raw_before = receiver.encode_state().unwrap();
                    let frontend_before = receiver.encode_frontend_state().unwrap();
                    let queue_before = receiver.cpu.queued_instruction_inputs();
                    for case in 0..9 {
                        let bad = corrupt(&raw, &frontend, front, case);
                        let failure = if front {
                            receiver.decode_frontend_state(&bad)
                        } else {
                            receiver.decode_state(&bad)
                        };
                        assert!(failure.is_err(), "front={front} case={case}");
                        assert_eq!(receiver, before);
                        assert_eq!(receiver.encode_state().unwrap(), raw_before);
                        assert_eq!(receiver.encode_frontend_state().unwrap(), frontend_before);
                        assert_eq!(receiver.cpu.queued_instruction_inputs(), queue_before);
                        assert_eq!(
                            receiver.bus.cartridge.download,
                            before.bus.cartridge.download
                        );
                    }
                    let mut expected = before.clone();
                    future_field(&mut receiver, &mut expected);
                    receiver.clear_rom_patches();
                    assert_eq!(receiver.peek(0x1234), Some(0xcc));
                    assert_eq!(receiver.peek(0x1235), Some(0xdd));
                }
            }
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn download_owned_checkpoint_undo_restores_backing_control_queue_and_receiver_mask() {
    let bytes = backing_image();
    for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
        for concurrent_sound in [false, true] {
            let mut donor = backing_source(
                &bytes,
                LoadConfig {
                    region,
                    concurrent_sound,
                },
            );
            donor.set_call_stack_recording(true);
            let raw = donor.encode_state().unwrap();
            let frontend = donor.encode_frontend_state().unwrap();
            let checkpoint = donor.encode_debugger_checkpoint().unwrap();
            let frames = donor.call_stack().copied().collect::<Vec<_>>();
            for (mask, expected) in [
                (vec![patch(0x1234, 0x77), patch(0x1235, 0x88)], 0x7788),
                (vec![patch(0x1234, 0x67), patch(0x1235, 0x80)], 0x6780),
                (vec![], 0xa55a),
            ] {
                let mut receiver = donor.clone();
                for _ in 0..4 {
                    receiver.step_instruction().unwrap();
                }
                receiver.install_rom_patches(&mask).unwrap();
                assert_ne!(
                    receiver.bus.cartridge.download,
                    donor.bus.cartridge.download
                );
                let before = receiver.clone();
                let owned_before = receiver.encode_debugger_checkpoint().unwrap();
                for target in [0xc00010, 0x3ffffe] {
                    assert!(matches!(
                        receiver.check_debug_call(target),
                        Err(crate::emulator::guest_call::GuestCallError::Rejected(
                            "target prefetch is outside qualified executable memory"
                        ))
                    ));
                    assert!(receiver.begin_debug_call(target).is_err());
                }
                assert_eq!(receiver, before);
                assert_eq!(receiver.encode_debugger_checkpoint().unwrap(), owned_before);
                receiver.decode_debugger_checkpoint(&checkpoint).unwrap();
                saved_download(&receiver, &donor, &raw);
                assert_eq!(receiver.encode_frontend_state().unwrap(), frontend);
                assert_eq!(receiver.encode_debugger_checkpoint().unwrap(), checkpoint);
                assert_eq!(receiver.call_stack_status(), donor.call_stack_status());
                assert_eq!(receiver.call_stack().copied().collect::<Vec<_>>(), frames);
                let mut oracle = receiver_oracle(&donor, &mask, true);
                same_continuation(&receiver, &oracle);
                receiver.step_instruction().unwrap();
                oracle.step_instruction().unwrap();
                assert_eq!(receiver.registers().d[0] & 0xffff, expected);
                for _ in 0..3 {
                    receiver.step_instruction().unwrap();
                    oracle.step_instruction().unwrap();
                }
                same_continuation(&receiver, &oracle);
                future_field(&mut receiver, &mut oracle);
                receiver.clear_rom_patches();
                assert_eq!(receiver.peek(0x1234), Some(0xcc));
                assert_eq!(receiver.peek(0x1235), Some(0xdd));
            }
        }
    }
}
