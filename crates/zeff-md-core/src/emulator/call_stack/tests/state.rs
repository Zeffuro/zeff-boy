use super::*;
use sha2::{Digest, Sha256};
use zeff_emu_common::save_state::StateReader;

fn origins(
    machine: &Emulator,
) -> (
    Option<[zeff_m68k::timing::InstructionInput; 2]>,
    Option<Vec<u8>>,
) {
    (
        machine.cpu.queued_instruction_inputs(),
        machine
            .bus
            .capture_board_origins(machine.rom_bytes().len() as u64)
            .unwrap(),
    )
}

fn stack_range(bytes: &[u8]) -> std::ops::Range<usize> {
    let mut reader = StateReader::new(&bytes[..bytes.len() - 32]);
    reader.set_position(76);
    for _ in 0..2 {
        reader.read_slice(usize::MAX).unwrap();
    }
    if reader.read_bool().unwrap() {
        reader.read_slice(usize::MAX).unwrap();
    }
    let length = reader.read_u32().unwrap() as usize;
    let start = reader.position();
    assert_eq!(start + length, bytes.len() - 32);
    start..start + length
}

fn seal(record: &mut Vec<u8>) {
    let hash = Sha256::digest(&*record);
    record.extend_from_slice(&hash);
}

fn legacy(bytes: &[u8]) -> Vec<u8> {
    let range = stack_range(bytes);
    let mut record = bytes[..range.start - 4].to_vec();
    record[8..12].copy_from_slice(&1_u32.to_le_bytes());
    seal(&mut record);
    record
}

fn changed(timing: TimingModel, concurrent: bool) -> Emulator {
    let mut receiver = fixture(timing, concurrent);
    receiver.set_call_stack_recording(true);
    receiver.install_rom_patches(&[patch(0x5a)]).unwrap();
    for _ in 0..3 {
        receiver.step_instruction().unwrap();
    }
    receiver.debug_write_ram(&[(0xff0300, 0x9a)]).unwrap();
    receiver
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn owned_v2_restores_bound_observed_frames_and_v1_frontend_fallback_is_canonical_unknown() {
    for (timing, concurrent) in PROFILES {
        let mut donor = fixture(timing, concurrent);
        donor.install_rom_patches(&[patch(0xa5)]).unwrap();
        enter(&mut donor);
        let front = donor.encode_frontend_state().unwrap();
        let saved = donor.encode_debugger_checkpoint().unwrap();
        assert_eq!(&saved[..8], b"ZMDDBG01");
        assert_eq!(&saved[8..12], &2_u32.to_le_bytes());
        let old = legacy(&saved);
        let captured = frames(&donor);
        let tags = origins(&donor);
        for (bytes, known) in [(&saved, true), (&old, false)] {
            let mut receiver = changed(timing, concurrent);
            assert_ne!(receiver.encode_frontend_state().unwrap(), front);
            receiver.decode_debugger_checkpoint(bytes).unwrap();
            assert_eq!(receiver.encode_frontend_state().unwrap(), front);
            assert_eq!(origins(&receiver), tags);
            assert_eq!(receiver.peek(0x1800), Some(0x5a));
            if known {
                assert_eq!(frames(&receiver), captured);
                assert_eq!(
                    receiver.call_stack_status(),
                    CallStackStatus::Observed { truncated: false }
                );
                assert_eq!(receiver.encode_debugger_checkpoint().unwrap(), saved);
            } else {
                assert!(frames(&receiver).is_empty());
                assert_eq!(receiver.call_stack_status(), CallStackStatus::Unknown);
            }
            assert!(receiver.cpu.control_flow_recording());
            assert!(
                receiver
                    .cpu
                    .clone()
                    .drain_control_flow_events()
                    .events()
                    .next()
                    .is_none()
            );
            receiver.step_instruction().unwrap();
            receiver.step_instruction().unwrap();
            assert_eq!(frames(&receiver).len(), if known { 1 } else { 0 });
        }
        let mut receiver = changed(timing, concurrent);
        receiver.decode_debugger_checkpoint(&front).unwrap();
        assert!(frames(&receiver).is_empty());
        assert_eq!(receiver.call_stack_status(), CallStackStatus::Unknown);
        for input in receiver.cpu.queued_instruction_inputs().unwrap() {
            assert_eq!(
                input.origin,
                [zeff_m68k::timing::InstructionByteOrigin::UNKNOWN; 2]
            );
        }
        let mut independent = fixture(timing, concurrent);
        for _ in 0..3 {
            independent.step_instruction().unwrap();
        }
        for frontend in [false, true] {
            let bytes = if frontend {
                independent.encode_frontend_state().unwrap()
            } else {
                independent.encode_state().unwrap()
            };
            let mut receiver = donor.clone();
            if frontend {
                receiver.decode_frontend_state(&bytes).unwrap();
            } else {
                receiver.decode_state(&bytes).unwrap();
            }
            assert_eq!(receiver.call_stack_status(), CallStackStatus::Unknown);
            assert!(frames(&receiver).is_empty());
            receiver.step_instruction().unwrap();
            assert_eq!(frames(&receiver).len(), 1);
            assert_eq!(frames(&receiver)[0].kind, ControlFlowEntryKind::Jsr);
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn owned_versions_reject_corruption_trailing_noncanonical_frames_and_audio_mismatch_atomically() {
    for (timing, concurrent) in PROFILES {
        let mut donor = fixture(timing, concurrent);
        enter(&mut donor);
        let good = donor.encode_debugger_checkpoint().unwrap();
        let old = legacy(&good);
        let range = stack_range(&good);
        let mut bad = vec![
            good[..good.len() - 1].to_vec(),
            old[..old.len() - 1].to_vec(),
        ];
        for original in [&good, &old] {
            let mut trailing = original[..original.len() - 32].to_vec();
            trailing.push(0);
            seal(&mut trailing);
            bad.push(trailing);
            let mut wrong = original.to_vec();
            wrong[8..12].copy_from_slice(&3_u32.to_le_bytes());
            let end = wrong.len() - 32;
            let hash = Sha256::digest(&wrong[..end]);
            wrong[end..].copy_from_slice(&hash);
            bad.push(wrong);
        }
        for (offset, value) in [(0, 2), (2, 1), (3, 2), (6, 0xff), (7, 1), (19, 2)] {
            let mut bytes = good.clone();
            bytes[range.start + offset] = value;
            let end = bytes.len() - 32;
            let hash = Sha256::digest(&bytes[..end]);
            bytes[end..].copy_from_slice(&hash);
            bad.push(bytes);
        }
        let mut wrong_slot = good.clone();
        wrong_slot[range.start + 24..range.start + 28].copy_from_slice(&0_u32.to_le_bytes());
        let end = wrong_slot.len() - 32;
        let hash = Sha256::digest(&wrong_slot[..end]);
        wrong_slot[end..].copy_from_slice(&hash);
        bad.push(wrong_slot);
        let mut too_many = good.clone();
        too_many[range.start + 4..range.start + 6].copy_from_slice(&u16::MAX.to_le_bytes());
        let end = too_many.len() - 32;
        let hash = Sha256::digest(&too_many[..end]);
        too_many[end..].copy_from_slice(&hash);
        bad.push(too_many);
        for bytes in bad {
            let mut receiver = changed(timing, concurrent);
            enter_after_setup(&mut receiver);
            let before = receiver.encode_debugger_checkpoint().unwrap();
            let captured = frames(&receiver);
            let tags = origins(&receiver);
            assert!(receiver.decode_debugger_checkpoint(&bytes).is_err());
            assert_eq!(receiver.encode_debugger_checkpoint().unwrap(), before);
            assert_eq!(frames(&receiver), captured);
            assert_eq!(origins(&receiver), tags);
        }
        for bytes in [&good, &old] {
            let mut receiver = changed(timing, concurrent);
            enter_after_setup(&mut receiver);
            receiver.set_sample_rate(44_100);
            let before = receiver.encode_debugger_checkpoint().unwrap();
            assert!(receiver.decode_debugger_checkpoint(bytes).is_err());
            assert_eq!(receiver.encode_debugger_checkpoint().unwrap(), before);
        }
        for frontend in [false, true] {
            let mut receiver = donor.clone();
            let before = receiver.encode_debugger_checkpoint().unwrap();
            let bytes = if frontend {
                donor.encode_frontend_state().unwrap()
            } else {
                donor.encode_state().unwrap()
            };
            let result = if frontend {
                receiver.decode_frontend_state(&bytes[..bytes.len() - 1])
            } else {
                receiver.decode_state(&bytes[..bytes.len() - 1])
            };
            assert!(result.is_err());
            assert_eq!(receiver.encode_debugger_checkpoint().unwrap(), before);
            assert_eq!(frames(&receiver), frames(&donor));
        }
    }
}

fn enter_after_setup(machine: &mut Emulator) {
    for _ in 0..2 {
        machine.step_instruction().unwrap();
    }
    assert_eq!(frames(machine).len(), 2);
}
