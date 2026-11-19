use super::*;

fn fill_program() -> Vec<u16> {
    let mut words = Vec::new();
    for word in [
        0x8114, 0x8f01, 0x9380, 0x9400, 0x9500, 0x9600, 0x9780, 0x4400, 0x0080,
    ] {
        words.extend([0x33fc, word, 0x00c0, 4]);
    }
    words.extend([0x33fc, 0, 0x00c0, 0, 0x4e75, 0x4e71]);
    words
}

fn dma_actions() -> DebugUiActions {
    let mut actions = DebugUiActions::none();
    actions
        .event_breakpoint_changes
        .push((DebugEvent::Dma, true));
    actions
}

fn rollback(owner: &mut Owner, call: GuestCallRequest, reason: &str) {
    let before = frontend(&owner.backend);
    let host = session(&mut owner.backend);
    let markers = owner.markers.clone();
    let effects = owner.dispatch(EmuCommand::ExecuteGuestCall(call));
    assert!(!effects.potentially_dirty);
    let Some(EmuResponse::GuestCallFailed { error, .. }) = effects.response else {
        panic!("DMA event failed to abort the call");
    };
    assert!(
        error.contains(reason) && error.contains("state restored"),
        "{error}"
    );
    assert_eq!(frontend(&owner.backend), before);
    assert_eq!(session(&mut owner.backend), host);
    assert_eq!(owner.markers, markers);
    assert!(owner.backend.take_runtime_fault().is_none());
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn md_guest_call_dma_callee_stop_rolls_back_and_preserves_previous_undo() {
    let callee = fill_program();
    for timing in [TimingModel::Standard, TimingModel::Precise] {
        let mut owner = Owner::new(fixture(
            b'U',
            timing,
            ControllerKind::ThreeButton,
            &[(0x1700, &callee)],
        ));
        let original = frontend(&owner.backend);
        let original_host = session(&mut owner.backend);
        let token = owner.call();
        configure(&mut owner.backend, &dma_actions());
        rollback(&mut owner, request(0x1700, 11), "debugger stop");
        owner.undo(token);
        assert_eq!(frontend(&owner.backend), original);
        assert_eq!(session(&mut owner.backend), original_host);
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn md_guest_call_dma_synthetic_entry_stop_restores_original_paused_transaction() {
    let callee = fill_program();
    for timing in [TimingModel::Standard, TimingModel::Precise] {
        let mut owner = Owner::new(fixture(
            b'U',
            timing,
            ControllerKind::ThreeButton,
            &[(0x1700, &callee)],
        ));
        let original = frontend(&owner.backend);
        let original_host = session(&mut owner.backend);
        let effects = owner.dispatch(EmuCommand::ExecuteGuestCall(request(0x1700, 11)));
        let Some(EmuResponse::GuestCallCompleted {
            instructions,
            undo_state,
            ..
        }) = effects.response
        else {
            panic!("arming call failed");
        };
        assert_eq!(instructions, 11);
        let mut found = false;
        for _ in 0..64 {
            let mut probe = machine(&owner.backend).clone();
            let epoch = probe.debug_dma_activity_epoch();
            let entry = probe.begin_debug_call(0x1000).unwrap();
            assert!(!probe.debug_call_returned(&entry));
            if probe.debug_dma_activity_epoch() != epoch {
                found = true;
                break;
            }
            machine_mut(&mut owner.backend).step_instruction().unwrap();
        }
        assert!(found, "no real service during bounded synthetic entry");
        configure(&mut owner.backend, &dma_actions());
        rollback(&mut owner, request(0x1000, 8), "during entry");
        owner.undo(undo_state);
        assert_eq!(frontend(&owner.backend), original);
        assert_eq!(session(&mut owner.backend), original_host);
    }
}
