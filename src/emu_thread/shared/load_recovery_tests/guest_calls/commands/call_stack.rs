use super::*;
use sha2::{Digest, Sha256};
use zeff_emu_common::cheats::{CheatPatch, CheatValue};
use zeff_emu_common::debug::WatchType;
use zeff_emu_common::save_state::StateReader;
use zeff_md_core::emulator::{CallStackFrame, CallStackStatus};

fn owner(timing: TimingModel) -> Owner {
    let mut backend = super::super::call_stack::backend(timing);
    machine_mut(&mut backend).set_call_stack_recording(true);
    let owner = Owner::new(backend);
    assert_eq!(captured(&owner).len(), 2);
    owner
}

fn captured(owner: &Owner) -> Vec<CallStackFrame> {
    machine(&owner.backend).call_stack().copied().collect()
}

fn owned(owner: &Owner) -> Vec<u8> {
    machine(&owner.backend)
        .encode_debugger_checkpoint()
        .unwrap()
}

fn resume(owner: &mut Owner) {
    let mut actions = DebugUiActions::none();
    actions
        .remove_breakpoints
        .push(machine(&owner.backend).registers().pc & 0xffffff);
    let mut config = BackendRuntimeConfig::new(&actions);
    config.opcode_log_enabled = true;
    config.debug_continue = true;
    owner.backend.apply_runtime_config(config);
    assert!(!owner.backend.is_suspended());
}

fn old_record(bytes: &[u8]) -> Vec<u8> {
    let mut reader = StateReader::new(&bytes[..bytes.len() - 32]);
    reader.set_position(76);
    for _ in 0..2 {
        reader.read_slice(usize::MAX).unwrap();
    }
    if reader.read_bool().unwrap() {
        reader.read_slice(usize::MAX).unwrap();
    }
    let mut old = bytes[..reader.position()].to_vec();
    old[8..12].copy_from_slice(&1_u32.to_le_bytes());
    let hash = Sha256::digest(&old);
    old.extend_from_slice(&hash);
    old
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn md_call_stack_common_guest_undo_and_failed_final_rts_restore_frames_and_previous_undo() {
    for timing in [TimingModel::Standard, TimingModel::Precise] {
        let mut owner = owner(timing);
        let initial = owned(&owner);
        let initial_frames = captured(&owner);
        let host = session(&mut owner.backend);
        let token = owner.call();
        assert_eq!(captured(&owner), initial_frames);
        let stack = machine(&owner.backend)
            .registers()
            .stack_pointer()
            .wrapping_sub(4)
            & 0xffffff;
        let mut actions = DebugUiActions::none();
        actions.add_watchpoint = Some((stack, stack + 3, WatchType::Read));
        configure(&mut owner.backend, &actions);
        let before = owned(&owner);
        let effects = owner.dispatch(EmuCommand::ExecuteGuestCall(request(0x1000, 8)));
        assert!(!effects.potentially_dirty);
        let Some(EmuResponse::GuestCallFailed { error, .. }) = effects.response else {
            panic!("final RTS watch did not fail")
        };
        assert!(error.contains("debugger stop") && error.contains("state restored"));
        assert_eq!(owned(&owner), before);
        assert_eq!(captured(&owner), initial_frames);
        let mut actions = DebugUiActions::none();
        actions
            .remove_watchpoints
            .push((stack, stack + 3, WatchType::Read));
        configure(&mut owner.backend, &actions);
        let mut faults = Vec::new();
        for (index, value) in [0x30, 0x39, 0, 0xb0, 0, 0, 0x4e, 0x75]
            .into_iter()
            .enumerate()
        {
            faults.push(CheatPatch::WideRomWrite {
                address: 0x1000 + index as u32,
                value: CheatValue::Constant(value),
            });
        }
        machine_mut(&mut owner.backend)
            .install_rom_patches(&faults)
            .unwrap();
        let before = owned(&owner);
        let effects = owner.dispatch(EmuCommand::ExecuteGuestCall(request(0x1000, 8)));
        assert!(!effects.potentially_dirty);
        let Some(EmuResponse::GuestCallFailed { error, .. }) = effects.response else {
            panic!("callee read fault did not fail")
        };
        assert!(error.contains("machine fault") && error.contains("state restored"));
        assert_eq!(owned(&owner), before);
        assert_eq!(captured(&owner), initial_frames);
        owner.undo(token);
        assert_eq!(owned(&owner), initial);
        assert_eq!(captured(&owner), initial_frames);
        assert_eq!(session(&mut owner.backend), host);
        assert_eq!(machine(&owner.backend).peek(0x1000), Some(0x30));
        assert!(owner.backend.is_suspended());
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn md_call_stack_owned_rewind_and_legacy_acceptance_preserve_audio_and_current_set_guards() {
    for timing in [TimingModel::Standard, TimingModel::Precise] {
        let mut owner = owner(timing);
        let original_frames = captured(&owner);
        resume(&mut owner);
        let saved = owner.backend.encode_rewind_checkpoint().unwrap();
        let old = old_record(&saved);
        let mut history = RewindBuffer::new(1, 1);
        EmuThread::capture_rewind_snapshot(&owner.backend, &mut history, true, 1);
        owner.backend.debug_suspend();
        owner.call();
        resume(&mut owner);
        owner.backend.step_frame();
        assert!(captured(&owner).is_empty());
        machine_mut(&mut owner.backend)
            .install_rom_patches(&[CheatPatch::WideRomWrite {
                address: 0x1800,
                value: CheatValue::Constant(0x5a),
            }])
            .unwrap();
        assert!(matches!(
            EmuThread::handle_rewind(&mut owner.backend, &mut history, &owner.framebuffer, 1),
            EmuResponse::RewindOk { .. }
        ));
        assert_eq!(captured(&owner), original_frames);
        assert_eq!(owner.backend.encode_rewind_checkpoint().unwrap(), saved);
        assert_eq!(machine(&owner.backend).peek(0x1800), Some(0x5a));
        assert!(!owner.backend.is_suspended());
        owner
            .backend
            .restore_rewind_checkpoint(old.clone())
            .unwrap();
        assert!(captured(&owner).is_empty());
        assert_eq!(
            machine(&owner.backend).call_stack_status(),
            CallStackStatus::Unknown
        );
        owner.backend.debug_suspend();
        let token = owner.call();
        assert_eq!(
            machine(&owner.backend).call_stack_status(),
            CallStackStatus::Observed { truncated: false }
        );
        let current = captured(&owner);
        owner.backend.set_sample_rate(44_100);
        let before = owned(&owner);
        for bytes in [&saved, &old] {
            assert!(
                owner
                    .backend
                    .restore_rewind_checkpoint(bytes.to_vec())
                    .is_err()
            );
            assert_eq!(owned(&owner), before);
            assert_eq!(captured(&owner), current);
            assert!(owner.backend.is_suspended());
        }
        owner.backend.set_sample_rate(48_000);
        owner.undo(token);
        assert_eq!(
            machine(&owner.backend).call_stack_status(),
            CallStackStatus::Unknown
        );
        assert!(captured(&owner).is_empty());
    }
}
